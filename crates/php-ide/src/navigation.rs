//! Go to definition / type definition / implementation, find references and
//! document highlights.

use php_index::{FileId, FileKind};
use php_syntax::{SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange, TextSize};
use rayon::prelude::*;

use crate::classify::{classify, classify_token, Definition};
use crate::infer::is_expr;
use crate::sema::Sema;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    pub file: FileId,
    /// The whole declaration.
    pub range: TextRange,
    /// The name of the declaration.
    pub focus: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReferenceKind {
    Declaration,
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reference {
    pub file: FileId,
    pub range: TextRange,
    pub kind: ReferenceKind,
}

/// `$_GET` etc.: their declaration in the superglobals stub.
fn superglobal_declaration(sema: &Sema, name: &str) -> Option<Location> {
    let (id, _) = sema.ws.files().find(|(_, f)| f.path.ends_with("superglobals/_superglobals.php"))?;
    let root = sema.ws.parse(id)?.syntax_node();
    let tok = root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == VARIABLE && &t.text()[1..] == name)?;
    let stmt =
        tok.parent_ancestors().find(|n| n.kind() == EXPR_STMT).map_or(tok.text_range(), |n| n.text_range());
    Some(Location { file: id, range: stmt, focus: tok.text_range() })
}

/// A variable token that (re)defines the variable: assignment target,
/// parameter, `foreach` / `catch` / `global` / `static` binding.
fn is_definition_token(t: &php_syntax::SyntaxToken) -> bool {
    let Some(var) = t.parent() else { return false };
    let Some(p) = var.parent() else { return false };
    match p.kind() {
        ASSIGN_EXPR => p.first_child().as_ref() == Some(&var),
        PARAM | FOREACH_VALUE | FOREACH_KEY | CATCH_CLAUSE | GLOBAL_STMT | STATIC_VAR | CLOSURE_USE_VAR => {
            true
        }
        _ => matches!(var.kind(), PARAM | CATCH_CLAUSE | STATIC_VAR),
    }
}

pub(crate) fn declarations(sema: &Sema, def: &Definition, at: Option<TextSize>) -> Vec<Location> {
    let index = sema.index();
    let loc = |file, range, focus| Location { file, range, focus };
    match def {
        Definition::Class(fqn) => {
            index.classes(fqn).into_iter().map(|(f, c)| loc(f, c.range, c.name_range)).collect()
        }
        Definition::Function(fqn) => {
            index.functions(fqn).into_iter().map(|(f, d)| loc(f, d.range, d.name_range)).collect()
        }
        Definition::Constant(fqn) => {
            index.constants(fqn).into_iter().map(|(f, d)| loc(f, d.range, d.name_range)).collect()
        }
        Definition::Method { class, name } => index
            .find_method(class, name)
            .map(|m| loc(m.file, m.def.range, m.def.name_range))
            .into_iter()
            .collect(),
        Definition::Property { class, name } => index
            .find_property(class, name)
            .map(|p| loc(p.file, p.def.range, p.def.name_range))
            .into_iter()
            .collect(),
        Definition::ClassConstant { class, name } => index
            .find_class_constant(class, name)
            .map(|c| loc(c.file, c.def.range, c.def.name_range))
            .into_iter()
            .collect(),
        Definition::EnumCase { class, name } => index
            .find_case(class, name)
            .map(|c| loc(c.file, c.def.range, c.def.name_range))
            .into_iter()
            .collect(),
        Definition::Variable { name, scope } => {
            let Some(scope_node) = node_with_range(&sema.root, *scope) else { return Vec::new() };
            if crate::infer::is_superglobal(name) {
                return superglobal_declaration(sema, name).into_iter().collect();
            }
            // the closest definition before the use (as Intelephense), else the first
            let candidates = variable_tokens(&scope_node, name)
                .into_iter()
                .filter(|t| at.is_none_or(|a| t.text_range().start() <= a))
                .filter(|t| classify_token(sema, t).is_some_and(|d| d.same_as(def)))
                .filter(|t| at.is_none() || is_definition_token(t));
            let first = if at.is_some() { candidates.last() } else { candidates.into_iter().next() };
            let first = first.or_else(|| {
                variable_tokens(&scope_node, name)
                    .into_iter()
                    .filter(|t| at.is_none_or(|a| t.text_range().start() <= a))
                    .find(|t| classify_token(sema, t).is_some_and(|d| d.same_as(def)))
            });
            first
                .map(|t| {
                    let range = t
                        .parent()
                        .and_then(|p| {
                            p.ancestors().find(|a| {
                                matches!(
                                    a.kind(),
                                    ASSIGN_EXPR | PARAM | FOREACH_STMT | CATCH_CLAUSE | STATIC_VAR
                                )
                            })
                        })
                        .map(|n| {
                            if n.kind() == FOREACH_STMT || n.kind() == CATCH_CLAUSE {
                                t.text_range()
                            } else {
                                n.text_range()
                            }
                        })
                        .unwrap_or(t.text_range());
                    loc(sema.file, range, t.text_range())
                })
                .into_iter()
                .collect()
        }
        Definition::Namespace(_) => Vec::new(),
    }
}

fn node_with_range(root: &SyntaxNode, range: TextRange) -> Option<SyntaxNode> {
    root.descendants().find(|n| {
        n.text_range() == range
            && matches!(n.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | PROPERTY_HOOK | SOURCE_FILE)
    })
}

fn variable_tokens(scope: &SyntaxNode, name: &str) -> Vec<SyntaxToken> {
    scope
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| {
            (t.kind() == VARIABLE && &t.text()[1..] == name)
                || (t.kind() == IDENT
                    && t.text() == name
                    && t.parent().is_some_and(|p| p.kind() == VARIABLE_EXPR))
        })
        .collect()
}

pub(crate) fn goto_definition(sema: &Sema, offset: TextSize) -> Vec<Location> {
    // `include 'file.php'` could be resolved here in the future
    match classify(sema, offset) {
        Some((def, _)) => {
            // `$x` in a doc comment documents a parameter declared after it
            let in_doc = sema.token_at(offset).is_some_and(|t| t.kind() == DOC_COMMENT);
            let mut locs = declarations(sema, &def, if in_doc { None } else { Some(offset) });
            // for variables at their own declaration, still return it
            locs.dedup();
            locs
        }
        None => Vec::new(),
    }
}

pub(crate) fn goto_type_definition(sema: &Sema, offset: TextSize) -> Vec<Location> {
    let Some(token) = sema.token_at(offset) else { return Vec::new() };
    let Some(node) = token.parent() else { return Vec::new() };
    let ty = match node.ancestors().find(|n| is_expr(n.kind())) {
        Some(expr) => sema.infer().expr_type(&expr),
        None => match classify(sema, offset) {
            Some((Definition::Variable { name, .. }, _)) => sema.infer().variable_type(&name, &node),
            Some((d @ Definition::Class(_), _)) => return declarations(sema, &d, None),
            _ => return Vec::new(),
        },
    };
    ty.class_names().into_iter().flat_map(|c| declarations(sema, &Definition::Class(c), None)).collect()
}

pub(crate) fn goto_implementation(sema: &Sema, offset: TextSize) -> Vec<Location> {
    let Some((def, _)) = classify(sema, offset) else { return Vec::new() };
    let index = sema.index();
    match def {
        Definition::Class(fqn) => index
            .subtypes(&fqn)
            .into_iter()
            .map(|(f, c)| Location { file: f, range: c.range, focus: c.name_range })
            .collect(),
        Definition::Method { class, name } => index
            .subtypes(&class)
            .into_iter()
            .filter_map(|(f, c)| {
                c.method(&name).map(|m| Location { file: f, range: m.range, focus: m.name_range })
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn reference_kind(token: &SyntaxToken) -> ReferenceKind {
    let Some(parent) = token.parent() else { return ReferenceKind::Read };
    match parent.kind() {
        NAME => match parent.parent().map(|p| p.kind()) {
            Some(CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL | FUNCTION_DECL | CONST_ELEMENT) => {
                ReferenceKind::Declaration
            }
            _ => ReferenceKind::Read,
        },
        MEMBER_NAME => match parent.parent().map(|p| p.kind()) {
            Some(METHOD_DECL | CONST_ELEMENT | ENUM_CASE) => ReferenceKind::Declaration,
            Some(PROPERTY_FETCH_EXPR | STATIC_PROPERTY_FETCH_EXPR) => {
                if is_write_target(&parent.parent().unwrap()) {
                    ReferenceKind::Write
                } else {
                    ReferenceKind::Read
                }
            }
            _ => ReferenceKind::Read,
        },
        PROPERTY_ELEMENT | PARAM => ReferenceKind::Declaration,
        VARIABLE_EXPR => {
            if is_write_target(&parent) {
                ReferenceKind::Write
            } else {
                ReferenceKind::Read
            }
        }
        _ => ReferenceKind::Read,
    }
}

fn is_write_target(expr: &SyntaxNode) -> bool {
    let Some(p) = expr.parent() else { return false };
    match p.kind() {
        ASSIGN_EXPR => p.first_child().as_ref() == Some(expr),
        FOREACH_VALUE | FOREACH_KEY | STATIC_VAR => true,
        ARRAY_ELEMENT => p
            .parent()
            .and_then(|a| a.parent())
            .is_some_and(|a| a.kind() == ASSIGN_EXPR && a.first_child() == p.parent()),
        _ => false,
    }
}

fn token_matches(token: &SyntaxToken, name: &str, case_insensitive: bool) -> bool {
    let text = match token.kind() {
        VARIABLE => &token.text()[1..],
        IDENT => token.text(),
        k if k.is_keyword() => token.text(),
        _ => return false,
    };
    if case_insensitive {
        text.eq_ignore_ascii_case(name)
    } else {
        text == name
    }
}

fn file_references(sema: &Sema, def: &Definition, name: &str, include_declaration: bool) -> Vec<Reference> {
    let ci = matches!(
        def,
        Definition::Class(_) | Definition::Function(_) | Definition::Method { .. } | Definition::Namespace(_)
    );
    let root = match def {
        Definition::Variable { scope, .. } => match node_with_range(&sema.root, *scope) {
            Some(n) => n,
            None => return Vec::new(),
        },
        _ => sema.root.clone(),
    };
    let mut out = Vec::new();
    let is_class = matches!(def, Definition::Class(_));
    for token in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        // class names in doc comments
        if is_class
            && token.kind() == DOC_COMMENT
            && token.text().to_ascii_lowercase().contains(&name.to_ascii_lowercase())
        {
            for r in crate::doc_types::doc_type_refs(&token) {
                let last = r.name.rsplit('\\').next().unwrap_or(&r.name);
                if !last.eq_ignore_ascii_case(name) {
                    continue;
                }
                let Some(fqn) = crate::classify::resolve_doc_name(sema, &token, &r.name) else { continue };
                if Definition::Class(fqn).same_as(def) {
                    // only the last segment is the name itself
                    let len = php_syntax::TextSize::from(last.len() as u32);
                    out.push(Reference {
                        file: sema.file,
                        range: TextRange::new(r.range.end() - len, r.range.end()),
                        kind: ReferenceKind::Read,
                    });
                }
            }
            continue;
        }
        if !token_matches(&token, name, ci) {
            continue;
        }
        let Some(d) = classify_token(sema, &token) else { continue };
        if !d.same_as(def) {
            continue;
        }
        let kind = reference_kind(&token);
        if kind == ReferenceKind::Declaration && !include_declaration {
            continue;
        }
        out.push(Reference { file: sema.file, range: token.text_range(), kind });
    }
    out
}

pub(crate) fn references(sema: &Sema, offset: TextSize, include_declaration: bool) -> Vec<Reference> {
    let Some((def, _)) = classify(sema, offset) else { return Vec::new() };
    let name = def.search_name();
    if matches!(def, Definition::Variable { .. }) {
        return file_references(sema, &def, &name, include_declaration);
    }
    let lower = name.to_ascii_lowercase();
    let ws = sema.ws;
    let current = sema.file;
    let candidates: Vec<FileId> = ws
        .files()
        .filter(|(id, f)| *id == current || f.kind == FileKind::Workspace)
        .filter(|(_, f)| f.text.to_ascii_lowercase().contains(&lower))
        .map(|(id, _)| id)
        .collect();
    let mut out: Vec<Reference> = candidates
        .into_par_iter()
        .flat_map_iter(|id| match Sema::new(ws, id) {
            Some(s) => file_references(&s, &def, &name, include_declaration),
            None => Vec::new(),
        })
        .collect();
    // declarations outside the workspace (vendor / stubs)
    if include_declaration {
        for loc in declarations(sema, &def, None) {
            if !out.iter().any(|r| r.file == loc.file && r.range == loc.focus) {
                out.push(Reference { file: loc.file, range: loc.focus, kind: ReferenceKind::Declaration });
            }
        }
    }
    out.sort_by_key(|r| (r.file, r.range.start()));
    out.dedup();
    out
}

pub(crate) fn document_highlights(sema: &Sema, offset: TextSize) -> Vec<Reference> {
    let Some((def, _)) = classify(sema, offset) else { return Vec::new() };
    let name = def.search_name();
    file_references(sema, &def, &name, true)
}
