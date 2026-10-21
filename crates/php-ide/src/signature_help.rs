//! Signature help for calls.

use php_hir::symbols::{FunctionDef, ParamDef};
use php_syntax::ast;
use php_syntax::{SyntaxKind::*, SyntaxNode, TextSize};

use crate::render;
use crate::sema::Sema;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterInfo {
    /// The parameter as rendered in the signature label.
    pub label: String,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureInfo {
    pub label: String,
    pub documentation: Option<String>,
    pub parameters: Vec<ParameterInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelp {
    pub signatures: Vec<SignatureInfo>,
    pub active_signature: usize,
    pub active_parameter: Option<usize>,
}

/// The function / method called by a call-like node.
pub(crate) fn callee<'a>(sema: &Sema<'a>, call: &SyntaxNode) -> Option<&'a FunctionDef> {
    let index = sema.index();
    match call.kind() {
        CALL_EXPR => {
            let callee = call.first_child()?;
            if callee.kind() == NAME_EXPR {
                let text = ast::text_without_trivia(&callee);
                return sema.resolve_function(&text, callee.text_range().start()).map(|(_, f)| f);
            }
            let ty = sema.infer().expr_type(&callee);
            ty.class_names().into_iter().find_map(|c| index.find_method(&c, "__invoke").map(|m| m.def))
        }
        METHOD_CALL_EXPR | STATIC_CALL_EXPR => {
            let name =
                call.children().find_map(|c| ast::AstNode::cast(c).map(|m: ast::MemberName| m))?.text()?;
            let lhs = call.first_child()?;
            let classes = if call.kind() == STATIC_CALL_EXPR {
                crate::classify::static_lhs_classes(sema, &lhs)
            } else {
                sema.infer().expr_type(&lhs).class_names()
            };
            classes.into_iter().find_map(|c| index.find_method(&c, &name).map(|m| m.def))
        }
        NEW_EXPR => {
            let class = call.children().find(|c| c.kind() == NAME_EXPR)?;
            let fqn = sema.resolve_class_name(&ast::text_without_trivia(&class), &class)?;
            index.find_method(&fqn, "__construct").map(|m| m.def)
        }
        ATTRIBUTE => {
            let name = call.children().find(|c| c.kind() == QUALIFIED_NAME)?;
            let fqn = sema.resolve_class_name(&ast::text_without_trivia(&name), &name)?;
            index.find_method(&fqn, "__construct").map(|m| m.def)
        }
        _ => None,
    }
}

pub(crate) fn callee_params<'a>(sema: &Sema<'a>, call: &SyntaxNode) -> Option<&'a [ParamDef]> {
    callee(sema, call).map(|f| f.params.as_slice())
}

pub(crate) fn signature_help(sema: &Sema, offset: TextSize) -> Option<SignatureHelp> {
    // the significant token left of the cursor (trailing whitespace of an
    // unfinished call belongs to an outer node)
    let mut token = sema.root.token_at_offset(offset).left_biased()?;
    while token.kind().is_trivia() {
        token = token.prev_token()?;
    }
    // innermost argument list whose parentheses contain the cursor
    let arg_list = token.parent_ancestors().find(|n| {
        n.kind() == ARG_LIST && {
            let r = n.text_range();
            let closed = n.last_token().is_some_and(|t| t.kind() == R_PAREN);
            r.start() < offset && (offset < r.end() || !closed)
        }
    })?;
    let call = arg_list.parent()?;
    let f = callee(sema, &call)?;

    // active parameter: commas before the cursor, or a named argument
    let mut active = 0usize;
    let mut named: Option<String> = None;
    for el in arg_list.children_with_tokens() {
        if el.text_range().start() >= offset {
            break;
        }
        match el {
            php_syntax::NodeOrToken::Token(t) if t.kind() == COMMA => active += 1,
            php_syntax::NodeOrToken::Node(n)
                if n.kind() == ARG && n.text_range().contains_inclusive(offset) =>
            {
                named = n.children().find(|c| c.kind() == NAME).map(|c| ast::text_without_trivia(&c));
            }
            _ => {}
        }
    }
    let mut label = String::from(&*f.name);
    label.push('(');
    let mut parameters = Vec::new();
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let text = render::param(p);
        label.push_str(&text);
        parameters.push(ParameterInfo { label: text, documentation: p.description.clone() });
    }
    label.push(')');
    if let Some(t) = f.effective_return_type() {
        label.push_str(": ");
        label.push_str(&t.to_string());
    }
    let active_parameter = match named {
        Some(n) => f.params.iter().position(|p| p.name == n),
        None => {
            let last = f.params.len().saturating_sub(1);
            if f.params.last().is_some_and(|p| p.variadic) && active > last {
                Some(last)
            } else {
                Some(active)
            }
        }
    };
    let documentation = f.doc.as_ref().map(|d| d.summary.clone()).filter(|s| !s.is_empty());
    Some(SignatureHelp {
        signatures: vec![SignatureInfo { label, documentation, parameters }],
        active_signature: 0,
        active_parameter,
    })
}
