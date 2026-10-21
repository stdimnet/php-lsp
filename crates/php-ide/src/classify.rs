//! Classifies the identifier at a position: what does it refer to?

use php_hir::Fqn;
use php_syntax::ast::{self, AstNode, UseKind};
use php_syntax::{SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange, TextSize};

use crate::sema::Sema;

/// A named program element.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Definition {
    Class(Fqn),
    Function(Fqn),
    Constant(Fqn),
    Method {
        class: Fqn,
        name: String,
    },
    Property {
        class: Fqn,
        name: String,
    },
    ClassConstant {
        class: Fqn,
        name: String,
    },
    EnumCase {
        class: Fqn,
        name: String,
    },
    /// A local variable; `scope` is the range of the enclosing function /
    /// closure / file in the same file.
    Variable {
        name: String,
        scope: TextRange,
    },
    Namespace(String),
}

impl Definition {
    /// Whether two definitions denote the same element (class and function
    /// names are case insensitive).
    pub fn same_as(&self, other: &Definition) -> bool {
        use Definition::*;
        match (self, other) {
            (Class(a), Class(b)) | (Function(a), Function(b)) => a.eq_ignore_case(b),
            (Constant(a), Constant(b)) => a.as_str() == b.as_str(),
            (Method { class: c1, name: n1 }, Method { class: c2, name: n2 }) => {
                c1.eq_ignore_case(c2) && n1.eq_ignore_ascii_case(n2)
            }
            (Property { class: c1, name: n1 }, Property { class: c2, name: n2 })
            | (ClassConstant { class: c1, name: n1 }, ClassConstant { class: c2, name: n2 })
            | (EnumCase { class: c1, name: n1 }, EnumCase { class: c2, name: n2 }) => {
                c1.eq_ignore_case(c2) && n1 == n2
            }
            (Variable { name: n1, scope: s1 }, Variable { name: n2, scope: s2 }) => n1 == n2 && s1 == s2,
            (Namespace(a), Namespace(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }

    /// The bare name used to find candidate tokens when searching references.
    pub fn search_name(&self) -> String {
        match self {
            Definition::Class(f) | Definition::Function(f) | Definition::Constant(f) => f.short().to_string(),
            Definition::Method { name, .. }
            | Definition::ClassConstant { name, .. }
            | Definition::EnumCase { name, .. } => name.clone(),
            Definition::Property { name, .. } | Definition::Variable { name, .. } => name.clone(),
            Definition::Namespace(n) => n.rsplit('\\').next().unwrap_or(n).to_string(),
        }
    }
}

const BUILTIN_TYPES: &[&str] = &[
    "int", "integer", "float", "double", "string", "bool", "boolean", "array", "callable", "iterable",
    "object", "mixed", "void", "null", "never", "false", "true", "resource",
];

/// Returns the definition and the range of the identifier at `offset`.
pub(crate) fn classify(sema: &Sema, offset: TextSize) -> Option<(Definition, TextRange)> {
    let token = sema.token_at(offset)?;
    let def = classify_token(sema, &token)?;
    Some((def, token.text_range()))
}

pub(crate) fn classify_token(sema: &Sema, token: &SyntaxToken) -> Option<Definition> {
    let parent = token.parent()?;
    if token.kind() == VARIABLE {
        return classify_variable(sema, token, &parent);
    }
    if token.kind() != IDENT && !token.kind().is_keyword() {
        return None;
    }
    match parent.kind() {
        QUALIFIED_NAME => classify_qualified_name(sema, &parent),
        NAME => classify_decl_name(sema, &parent),
        MEMBER_NAME => classify_member_name(sema, &parent),
        NAMED_TYPE => None, // array / callable / static keywords
        VARIABLE_EXPR if token.kind() == IDENT => {
            // `${name}` inside strings
            let scope = sema.variable_scope(&parent);
            Some(Definition::Variable { name: token.text().to_string(), scope: scope.text_range() })
        }
        _ => None,
    }
}

fn classify_variable(sema: &Sema, token: &SyntaxToken, parent: &SyntaxNode) -> Option<Definition> {
    let name = token.text().trim_start_matches('$').to_string();
    match parent.kind() {
        PROPERTY_ELEMENT => Some(Definition::Property { class: sema.enclosing_class(parent)?, name }),
        PARAM => {
            let param = ast::Param::cast(parent.clone())?;
            if param.modifiers().is_some() {
                return Some(Definition::Property { class: sema.enclosing_class(parent)?, name });
            }
            let scope = sema.variable_scope(parent);
            Some(Definition::Variable { name, scope: scope.text_range() })
        }
        MEMBER_NAME => {
            let fetch = parent.parent()?;
            if matches!(fetch.kind(), STATIC_PROPERTY_FETCH_EXPR | STATIC_CALL_EXPR) {
                let lhs = fetch.first_child()?;
                let classes = static_lhs_classes(sema, &lhs);
                return Some(property_def(sema, &classes, &name));
            }
            None
        }
        CLOSURE_USE_VAR => {
            let closure = parent.ancestors().find(|n| n.kind() == CLOSURE_EXPR)?;
            let outer = sema.variable_scope(&closure.parent()?);
            Some(Definition::Variable { name, scope: outer.text_range() })
        }
        _ => {
            if name == "this" {
                return sema.enclosing_class(parent).map(Definition::Class);
            }
            let scope = sema.variable_scope(parent);
            Some(Definition::Variable { name, scope: scope.text_range() })
        }
    }
}

fn classify_qualified_name(sema: &Sema, qn: &SyntaxNode) -> Option<Definition> {
    let text = ast::text_without_trivia(qn);
    let ctx = qn.parent()?;
    let offset = qn.text_range().start();
    match ctx.kind() {
        NAMESPACE_DECL => Some(Definition::Namespace(text)),
        USE_CLAUSE => classify_use_clause(sema, &ctx),
        NAME_EXPR => {
            let grand = ctx.parent()?;
            let is_first = grand.first_child().as_ref() == Some(&ctx);
            match grand.kind() {
                CALL_EXPR if is_first => {
                    let r = sema.scope_at(offset).resolve_function(&text);
                    match sema.resolve_function(&text, offset) {
                        Some((_, f)) => Some(Definition::Function(f.fqn.clone())),
                        None => Some(Definition::Function(r.name)),
                    }
                }
                NEW_EXPR | INSTANCEOF_EXPR => sema.resolve_class_name(&text, qn).map(Definition::Class),
                STATIC_CALL_EXPR | CLASS_CONST_FETCH_EXPR | STATIC_PROPERTY_FETCH_EXPR if is_first => {
                    sema.resolve_class_name(&text, qn).map(Definition::Class)
                }
                _ => {
                    if matches!(text.to_ascii_lowercase().trim_start_matches('\\'), "true" | "false" | "null")
                    {
                        return None;
                    }
                    let r = sema.scope_at(offset).resolve_constant(&text);
                    match sema.resolve_constant(&text, offset) {
                        Some((_, c)) => Some(Definition::Constant(c.fqn.clone())),
                        None => Some(Definition::Constant(r.name)),
                    }
                }
            }
        }
        NAMED_TYPE => {
            if BUILTIN_TYPES.contains(&text.to_ascii_lowercase().as_str()) {
                return None;
            }
            sema.resolve_class_name(&text, qn).map(Definition::Class)
        }
        EXTENDS_CLAUSE | IMPLEMENTS_CLAUSE | CATCH_TYPES | TRAIT_USE | ATTRIBUTE | TRAIT_PRECEDENCE
        | TRAIT_ALIAS => sema.resolve_class_name(&text, qn).map(Definition::Class),
        _ => None,
    }
}

fn classify_use_clause(sema: &Sema, clause: &SyntaxNode) -> Option<Definition> {
    let c = ast::UseClause::cast(clause.clone())?;
    let name = c.name()?.text();
    // group: `use A\{B}` -> prefix from the outer clause
    let (full, outer) = match clause.parent().filter(|p| p.kind() == USE_GROUP).and_then(|g| g.parent()) {
        Some(outer) => {
            let prefix = ast::UseClause::cast(outer.clone())
                .and_then(|o| o.name())
                .map(|n| n.text())
                .unwrap_or_default();
            (format!("{}\\{}", prefix.trim_start_matches('\\'), name), Some(outer))
        }
        None => (name, None),
    };
    if c.group().is_some() {
        return Some(Definition::Namespace(full));
    }
    let decl = outer.as_ref().unwrap_or(clause).ancestors().find_map(ast::UseDecl::cast)?;
    let kind = c.own_kind().unwrap_or(decl.kind());
    let fqn = Fqn::new(&full);
    let _ = sema;
    Some(match kind {
        UseKind::Class => Definition::Class(fqn),
        UseKind::Function => Definition::Function(fqn),
        UseKind::Const => Definition::Constant(fqn),
    })
}

fn classify_decl_name(sema: &Sema, name: &SyntaxNode) -> Option<Definition> {
    let ctx = name.parent()?;
    let text = ast::text_without_trivia(name);
    match ctx.kind() {
        CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL => sema.class_fqn(&ctx).map(Definition::Class),
        FUNCTION_DECL => {
            let scope = sema.scope_at(ctx.text_range().start());
            Some(Definition::Function(Fqn::join(scope.namespace.as_ref(), &text)))
        }
        CONST_ELEMENT if ctx.parent().is_some_and(|p| p.kind() == CONST_DECL) => {
            let scope = sema.scope_at(ctx.text_range().start());
            Some(Definition::Constant(Fqn::join(scope.namespace.as_ref(), &text)))
        }
        USE_ALIAS => classify_use_clause(sema, &ctx.parent()?),
        _ => None,
    }
}

fn classify_member_name(sema: &Sema, member: &SyntaxNode) -> Option<Definition> {
    let ctx = member.parent()?;
    let name = ast::MemberName::cast(member.clone())?.text()?;
    match ctx.kind() {
        METHOD_DECL => Some(Definition::Method { class: sema.enclosing_class(&ctx)?, name }),
        CONST_ELEMENT => Some(Definition::ClassConstant { class: sema.enclosing_class(&ctx)?, name }),
        ENUM_CASE => Some(Definition::EnumCase { class: sema.enclosing_class(&ctx)?, name }),
        METHOD_CALL_EXPR | PROPERTY_FETCH_EXPR => {
            let receiver = ctx.first_child()?;
            let ty = sema.infer().expr_type(&receiver);
            let classes = ty.class_names();
            if ctx.kind() == METHOD_CALL_EXPR {
                Some(method_def(sema, &classes, &name))
            } else {
                Some(property_def(sema, &classes, &name))
            }
        }
        STATIC_CALL_EXPR => {
            let classes = static_lhs_classes(sema, &ctx.first_child()?);
            Some(method_def(sema, &classes, &name))
        }
        CLASS_CONST_FETCH_EXPR => {
            let classes = static_lhs_classes(sema, &ctx.first_child()?);
            if name.eq_ignore_ascii_case("class") {
                return classes.into_iter().next().map(Definition::Class);
            }
            for c in &classes {
                if let Some(case) = sema.index().find_case(c, &name) {
                    return Some(Definition::EnumCase { class: case.owner.fqn.clone(), name });
                }
                if let Some(k) = sema.index().find_class_constant(c, &name) {
                    return Some(Definition::ClassConstant { class: k.owner.fqn.clone(), name });
                }
            }
            Some(Definition::ClassConstant { class: classes.into_iter().next()?, name })
        }
        TRAIT_ALIAS | TRAIT_PRECEDENCE => {
            let class = ctx
                .children()
                .find(|c| c.kind() == QUALIFIED_NAME)
                .and_then(|q| sema.resolve_class_name(&ast::text_without_trivia(&q), &q))
                .or_else(|| {
                    // `foo as bar` - search the used traits
                    let use_node = ctx.ancestors().find(|n| n.kind() == TRAIT_USE)?;
                    let tu = ast::TraitUse::cast(use_node.clone())?;
                    tu.names()
                        .filter_map(|n| sema.resolve_class_name(&n.text(), n.syntax()))
                        .find(|t| sema.index().find_method(t, &name).is_some())
                })?;
            Some(method_def(sema, &[class], &name))
        }
        _ => None,
    }
}

/// Classes referenced by the left side of `X::`.
pub(crate) fn static_lhs_classes(sema: &Sema, lhs: &SyntaxNode) -> Vec<Fqn> {
    if lhs.kind() == NAME_EXPR {
        let text = ast::text_without_trivia(lhs);
        return sema.resolve_class_name(&text, lhs).into_iter().collect();
    }
    let ty = sema.infer().expr_type(lhs);
    let mut out = ty.class_names();
    for m in ty.members() {
        if let php_hir::Type::ClassString(Some(inner)) = m {
            out.extend(inner.class_names());
        }
    }
    out
}

fn method_def(sema: &Sema, classes: &[Fqn], name: &str) -> Definition {
    for c in classes {
        if let Some(m) = sema.index().find_method(c, name) {
            return Definition::Method { class: m.owner.fqn.clone(), name: m.def.name.clone() };
        }
    }
    Definition::Method {
        class: classes.first().cloned().unwrap_or_else(|| Fqn::new("")),
        name: name.to_string(),
    }
}

fn property_def(sema: &Sema, classes: &[Fqn], name: &str) -> Definition {
    for c in classes {
        if let Some(p) = sema.index().find_property(c, name) {
            return Definition::Property { class: p.owner.fqn.clone(), name: name.to_string() };
        }
    }
    Definition::Property {
        class: classes.first().cloned().unwrap_or_else(|| Fqn::new("")),
        name: name.to_string(),
    }
}
