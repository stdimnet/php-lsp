//! Inlay hints: parameter names at call sites and inferred return types.

use php_hir::Type;
use php_syntax::ast::{self, AstNode};
use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange, TextSize};

use crate::infer::is_expr;
use crate::sema::Sema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InlayHintKind {
    Parameter,
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHint {
    pub offset: TextSize,
    pub label: String,
    pub kind: InlayHintKind,
    pub padding_left: bool,
    pub padding_right: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHintsConfig {
    pub parameter_names: bool,
    pub return_types: bool,
    pub parameter_types: bool,
}

impl Default for InlayHintsConfig {
    fn default() -> Self {
        InlayHintsConfig { parameter_names: true, return_types: true, parameter_types: true }
    }
}

pub(crate) fn inlay_hints(sema: &Sema, range: TextRange) -> Vec<InlayHint> {
    let config = &sema.ws.config().inlay_hints;
    let mut out = Vec::new();
    for node in sema.root.descendants() {
        if !node.text_range().intersect(range).is_some_and(|r| !r.is_empty() || range.is_empty()) {
            continue;
        }
        match node.kind() {
            ARG_LIST if config.parameter_names => parameter_names(sema, &node, &mut out),
            FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | ARROW_FN_EXPR if config.return_types => {
                return_type(sema, &node, &mut out)
            }
            _ => {}
        }
        if config.parameter_types && matches!(node.kind(), CLOSURE_EXPR | ARROW_FN_EXPR) {
            closure_param_types(sema, &node, &mut out);
        }
    }
    out.retain(|h| range.contains_inclusive(h.offset));
    out.sort_by_key(|h| h.offset);
    out
}

/// The last name in an argument expression (`$name`, `$this->name`, `getName()`).
fn arg_name(expr: &SyntaxNode) -> Option<String> {
    match expr.kind() {
        VARIABLE_EXPR => ast::VariableExpr::cast(expr.clone())?.name(),
        PROPERTY_FETCH_EXPR
        | STATIC_PROPERTY_FETCH_EXPR
        | METHOD_CALL_EXPR
        | STATIC_CALL_EXPR
        | CLASS_CONST_FETCH_EXPR => {
            let m = expr.children().find_map(ast::MemberName::cast)?.text()?;
            Some(m.trim_start_matches("get").to_string())
        }
        NAME_EXPR => Some(ast::text_without_trivia(expr)),
        _ => None,
    }
}

fn parameter_names(sema: &Sema, list: &SyntaxNode, out: &mut Vec<InlayHint>) {
    let Some(call) = list.parent() else { return };
    let Some(params) = crate::signature_help::callee_params(sema, &call) else { return };
    for (i, arg) in list.children().filter(|c| c.kind() == ARG).enumerate() {
        if arg.children().any(|c| c.kind() == NAME) || arg.children_with_tokens().any(|e| e.kind() == DOT3) {
            break; // named or spread arguments: positions are no longer meaningful
        }
        let Some(param) = params.get(i).or_else(|| params.last().filter(|p| p.variadic)) else { break };
        if param.variadic && i >= params.len() {
            break;
        }
        let Some(expr) = arg.children().find(|c| is_expr(c.kind())) else { continue };
        if arg_name(&expr).is_some_and(|n| {
            n.eq_ignore_ascii_case(&param.name)
                || n.to_ascii_lowercase().ends_with(&param.name.to_ascii_lowercase())
        }) {
            continue;
        }
        let mut label = String::new();
        if param.by_ref {
            label.push('&');
        }
        if param.variadic {
            label.push_str("...");
        }
        label.push_str(&param.name);
        label.push(':');
        out.push(InlayHint {
            offset: arg.text_range().start(),
            label,
            kind: InlayHintKind::Parameter,
            padding_left: false,
            padding_right: true,
        });
    }
}

/// Union of the types of all `return` statements of a function body.
pub(crate) fn inferred_return_type(sema: &Sema, func: &SyntaxNode) -> Option<Type> {
    if func.kind() == ARROW_FN_EXPR {
        let body = func.children().filter(|c| is_expr(c.kind())).last()?;
        return Some(sema.infer().expr_type(&body));
    }
    let body = func.children().find(|c| c.kind() == BLOCK)?;
    let mut types = Vec::new();
    let mut is_generator = false;
    let mut stack: Vec<SyntaxNode> = body.children().collect();
    while let Some(n) = stack.pop() {
        match n.kind() {
            FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR | ARROW_FN_EXPR | CLASS_DECL | ANON_CLASS => continue,
            YIELD_EXPR | YIELD_FROM_EXPR => is_generator = true,
            RETURN_STMT => match n.children().find(|c| is_expr(c.kind())) {
                Some(e) => types.push(sema.infer().expr_type(&e)),
                None => types.push(Type::Void),
            },
            _ => {}
        }
        stack.extend(n.children());
    }
    if is_generator {
        return Some(Type::named("Generator"));
    }
    if types.is_empty() {
        return Some(Type::Void);
    }
    Some(Type::union(types))
}

fn return_type(sema: &Sema, func: &SyntaxNode, out: &mut Vec<InlayHint>) {
    if func.children().any(|c| c.kind() == RETURN_TYPE) {
        return;
    }
    let name = func
        .children()
        .find(|c| matches!(c.kind(), NAME | MEMBER_NAME))
        .map(|n| ast::text_without_trivia(&n));
    if name
        .as_deref()
        .is_some_and(|n| n.eq_ignore_ascii_case("__construct") || n.eq_ignore_ascii_case("__destruct"))
    {
        return;
    }
    // abstract / interface methods have no body to infer from
    if func.kind() == METHOD_DECL && !func.children().any(|c| c.kind() == BLOCK) {
        return;
    }
    let Some(params) = func.children().find(|c| c.kind() == PARAM_LIST) else { return };
    let Some(ty) = inferred_return_type(sema, func) else { return };
    if ty.is_mixed() {
        return;
    }
    // after `use (...)` for closures
    let anchor = func.children().find(|c| c.kind() == CLOSURE_USE).unwrap_or(params);
    out.push(InlayHint {
        offset: anchor.text_range().end(),
        label: format!(": {ty}"),
        kind: InlayHintKind::Type,
        padding_left: false,
        padding_right: false,
    });
}

/// Types of untyped closure parameters when the closure is passed to a
/// parameter declared as `callable(T): R` / `Closure(T): R`.
fn closure_param_types(sema: &Sema, closure: &SyntaxNode, out: &mut Vec<InlayHint>) {
    let Some(arg) = closure.parent().filter(|p| p.kind() == ARG) else { return };
    let Some(list) = arg.parent() else { return };
    let Some(call) = list.parent() else { return };
    let Some(params) = crate::signature_help::callee_params(sema, &call) else { return };
    let index = list.children().filter(|c| c.kind() == ARG).position(|a| a == arg);
    let Some(target) = index.and_then(|i| params.get(i)) else { return };
    let Some(Type::Closure(arg_types, _)) = target.effective_type().map(|t| t.without_null()) else { return };
    let Some(pl) = closure.children().find_map(ast::ParamList::cast) else { return };
    for (p, t) in pl.params().zip(arg_types.iter()) {
        if p.ty().is_some() || t.is_mixed() || matches!(t, Type::Template(_)) {
            continue;
        }
        let Some(var) = p.variable() else { continue };
        out.push(InlayHint {
            offset: var.text_range().start(),
            label: t.to_string(),
            kind: InlayHintKind::Type,
            padding_left: false,
            padding_right: true,
        });
    }
}
