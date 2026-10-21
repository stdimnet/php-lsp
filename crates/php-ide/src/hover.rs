//! Hover information.

use php_hir::phpdoc::DocBlock;
use php_hir::Fqn;
use php_syntax::{SyntaxKind::*, TextRange, TextSize};

use crate::classify::{classify, Definition};
use crate::render;
use crate::sema::Sema;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    /// Markdown content.
    pub markdown: String,
    /// The range of the hovered identifier.
    pub range: TextRange,
}

fn with_doc(
    code: String,
    doc: Option<&DocBlock>,
    deprecated: Option<&String>,
    include_params: bool,
) -> String {
    let mut s = code;
    if let Some(d) = deprecated {
        s.push_str("\n\n**Deprecated**");
        if !d.is_empty() {
            s.push_str(": ");
            s.push_str(d);
        }
    }
    if let Some(doc) = doc {
        let md = render::doc_markdown(doc, include_params);
        if !md.is_empty() {
            s.push_str("\n\n");
            s.push_str(&md);
        }
    }
    s
}

fn ns_of(fqn: &Fqn) -> Option<&str> {
    fqn.namespace()
}

pub(crate) fn hover(sema: &Sema, offset: TextSize) -> Option<Hover> {
    let (def, range) = classify(sema, offset)?;
    let markdown = render_definition(sema, &def, offset)?;
    Some(Hover { markdown, range })
}

pub(crate) fn render_definition(sema: &Sema, def: &Definition, offset: TextSize) -> Option<String> {
    let index = sema.index();
    Some(match def {
        Definition::Class(fqn) => {
            let (_, c) = index.class(fqn)?;
            with_doc(
                render::code_block(ns_of(&c.fqn), &format!("{} {{ }}", render::class_signature(c))),
                c.doc.as_deref(),
                c.deprecated.as_ref(),
                false,
            )
        }
        Definition::Function(fqn) => {
            let (_, f) = index.function(fqn)?;
            with_doc(
                render::code_block(ns_of(&f.fqn), &format!("{} {{ }}", render::function_signature(f, false))),
                f.doc.as_deref(),
                f.deprecated.as_ref(),
                true,
            )
        }
        Definition::Constant(fqn) => {
            let (_, c) = index.constant(fqn)?;
            with_doc(
                render::code_block(ns_of(&c.fqn), &render::const_signature(c)),
                c.doc.as_deref(),
                c.deprecated.as_ref(),
                false,
            )
        }
        Definition::Method { class, name } => {
            let m = index.find_method(class, name)?;
            let body = if m.def.is_abstract { ";" } else { " { }" };
            let code = format!("{}{}", render::function_signature(m.def, true), body);
            let mut s = with_doc(
                render::code_block(None, &code),
                m.def.doc.as_deref(),
                m.def.deprecated.as_ref(),
                true,
            );
            s.push_str(&format!("\n\n_{}::{}_", m.owner.fqn, m.def.name));
            s
        }
        Definition::Property { class, name } => {
            let p = index.find_property(class, name)?;
            let mut s = with_doc(
                render::code_block(None, &render::property_signature(p.def)),
                p.def.doc.as_deref(),
                p.def.deprecated.as_ref(),
                false,
            );
            s.push_str(&format!("\n\n_{}::${}_", p.owner.fqn, p.def.name));
            s
        }
        Definition::ClassConstant { class, name } => {
            let c = index.find_class_constant(class, name)?;
            let mut s = with_doc(
                render::code_block(None, &render::class_const_signature(c.def)),
                c.def.doc.as_deref(),
                c.def.deprecated.as_ref(),
                false,
            );
            s.push_str(&format!("\n\n_{}::{}_", c.owner.fqn, c.def.name));
            s
        }
        Definition::EnumCase { class, name } => {
            let c = index.find_case(class, name)?;
            let mut s = with_doc(
                render::code_block(None, &render::case_signature(c.def)),
                c.def.doc.as_deref(),
                None,
                false,
            );
            s.push_str(&format!("\n\n_{}::{}_", c.owner.fqn, c.def.name));
            s
        }
        Definition::Variable { name, .. } => {
            let token = sema.token_at(offset)?;
            let node = token.parent()?;
            let ty = if node.kind() == VARIABLE_EXPR {
                sema.infer().expr_type(&node)
            } else {
                sema.infer().variable_type(name, &node)
            };
            render::code_block(None, &format!("{} ${}", ty, name))
        }
        Definition::Namespace(n) => render::code_block(None, &format!("namespace {n};")),
    })
}
