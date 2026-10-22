//! Per-file semantic context shared by the IDE features.

use std::sync::Arc;

use php_hir::symbols::{ConstDef, FunctionDef};
use php_hir::{FileScopes, Fqn, NamespaceScope};
use php_index::{FileId, Index};
use php_syntax::ast::{self, AstNode};
use php_syntax::{SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange, TextSize, TokenAtOffset};

use crate::infer::Infer;
use crate::Workspace;

pub(crate) struct Sema<'a> {
    pub ws: &'a Workspace,
    pub file: FileId,
    pub root: SyntaxNode,
    pub scopes: Arc<FileScopes>,
    pub text: Arc<str>,
    /// Recursion depth of type inference (shared by all `Infer` instances).
    pub infer_depth: std::cell::Cell<u32>,
    /// Set while loop-carried assignments are evaluated (they are not
    /// followed again from there).
    pub in_loop_carried: std::cell::Cell<bool>,
    /// Inferred types of expressions (by range and kind) and of variables
    /// (by name and position) for this request.
    pub expr_cache:
        std::cell::RefCell<std::collections::HashMap<(TextRange, php_syntax::SyntaxKind), php_hir::Type>>,
    pub var_cache: std::cell::RefCell<std::collections::HashMap<(String, TextSize), php_hir::Type>>,
}

impl<'a> Sema<'a> {
    pub fn new(ws: &'a Workspace, file: FileId) -> Option<Sema<'a>> {
        // templates without PHP code (see `twig.rs`)
        if crate::twig::is_twig(&ws.file(file)?.path) {
            return None;
        }
        let parse = ws.parse(file)?;
        let scopes = ws.scopes(file, &parse);
        let root = parse.syntax_node();
        let text = ws.file(file)?.text.clone();
        Some(Sema {
            ws,
            file,
            root,
            scopes,
            text,
            infer_depth: std::cell::Cell::new(0),
            in_loop_carried: std::cell::Cell::new(false),
            expr_cache: Default::default(),
            var_cache: Default::default(),
        })
    }

    pub fn index(&self) -> &'a Index {
        self.ws.index()
    }

    pub fn infer(&self) -> Infer<'_, 'a> {
        Infer::new(self)
    }

    /// The most relevant token at `offset` (identifiers and variables win over
    /// punctuation and trivia when the cursor is between two tokens).
    pub fn token_at(&self, offset: TextSize) -> Option<SyntaxToken> {
        let offset = offset.min(self.root.text_range().end());
        match self.root.token_at_offset(offset) {
            TokenAtOffset::None => None,
            TokenAtOffset::Single(t) => Some(t),
            TokenAtOffset::Between(l, r) => {
                let score = |t: &SyntaxToken| match t.kind() {
                    IDENT | VARIABLE => 3,
                    k if k.is_keyword() => 2,
                    k if k.is_trivia() => 0,
                    _ => 1,
                };
                Some(if score(&r) > score(&l) { r } else { l })
            }
        }
    }

    pub fn scope_at(&self, offset: TextSize) -> &NamespaceScope {
        self.scopes.at(offset)
    }

    /// The class-like declaration enclosing `node`.
    pub fn enclosing_class_node(&self, node: &SyntaxNode) -> Option<SyntaxNode> {
        node.ancestors().find(|n| match n.kind() {
            CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL => true,
            // `new class($this->x) { ... }`: the arguments belong to the outer scope
            ANON_CLASS => n
                .children()
                .find(|c| c.kind() == MEMBER_LIST)
                .is_some_and(|m| m.text_range().contains_range(node.text_range())),
            _ => false,
        })
    }

    pub fn class_fqn(&self, class_node: &SyntaxNode) -> Option<Fqn> {
        if class_node.kind() == ANON_CLASS {
            let fqn = php_hir::symbols::anon_class_fqn(class_node);
            return self.index().class(&fqn).is_some().then_some(fqn);
        }
        let name = class_node.children().find_map(ast::Name::cast)?;
        let scope = self.scope_at(class_node.text_range().start());
        Some(Fqn::join(scope.namespace.as_ref(), &name.text()))
    }

    pub fn enclosing_class(&self, node: &SyntaxNode) -> Option<Fqn> {
        let c = self.enclosing_class_node(node)?;
        self.class_fqn(&c)
    }

    /// The parent class of the class enclosing `node`.
    pub fn enclosing_parent(&self, node: &SyntaxNode) -> Option<Fqn> {
        let c = self.enclosing_class_node(node)?;
        if let Some(fqn) = self.class_fqn(&c) {
            if let Some(p) = self.index().parent_of(&fqn) {
                return Some(p);
            }
        }
        // anonymous classes / not yet indexed: read the extends clause
        let ext = c.children().find_map(ast::ExtendsClause::cast)?;
        let name = ext.names().next()?;
        Some(self.scope_at(c.text_range().start()).resolve_class(&name.text()))
    }

    /// Resolves a class reference as written at `node` (handles
    /// `self`/`static`/`parent`).
    pub fn resolve_class_name(&self, name: &str, node: &SyntaxNode) -> Option<Fqn> {
        match name.to_ascii_lowercase().as_str() {
            "self" | "static" | "$this" => self.enclosing_class(node),
            "parent" => self.enclosing_parent(node),
            _ => Some(self.scope_at(node.text_range().start()).resolve_class(name)),
        }
    }

    /// Resolves a function call name with the global fallback.
    pub fn resolve_function(&self, name: &str, offset: TextSize) -> Option<(FileId, &'a FunctionDef)> {
        let r = self.scope_at(offset).resolve_function(name);
        self.index().function(&r.name).or_else(|| r.fallback.and_then(|f| self.index().function(&f)))
    }

    pub fn resolve_constant(&self, name: &str, offset: TextSize) -> Option<(FileId, &'a ConstDef)> {
        let r = self.scope_at(offset).resolve_constant(name);
        self.index().constant(&r.name).or_else(|| r.fallback.and_then(|f| self.index().constant(&f)))
    }

    /// Name of the variable scope (function, method, closure or file)
    /// enclosing `node`. Arrow functions are transparent.
    pub fn variable_scope(&self, node: &SyntaxNode) -> SyntaxNode {
        node.ancestors()
            .find(|n| {
                matches!(n.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | PROPERTY_HOOK | SOURCE_FILE)
            })
            .unwrap_or_else(|| self.root.clone())
    }
}

/// Whether `token` belongs directly to scope `scope` (not to a nested
/// function or closure).
pub(crate) fn in_variable_scope(token: &SyntaxToken, scope: &SyntaxNode) -> bool {
    let Some(parent) = token.parent() else { return false };
    let nearest = parent.ancestors().find(|n| {
        matches!(n.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | PROPERTY_HOOK | SOURCE_FILE)
    });
    nearest.as_ref() == Some(scope)
}
