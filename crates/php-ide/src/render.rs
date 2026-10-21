//! Rendering declarations as PHP source snippets and PHPDoc as Markdown.

use php_hir::phpdoc::DocBlock;
use php_hir::symbols::{ClassConstDef, ClassDef, ConstDef, EnumCaseDef, FunctionDef, ParamDef, PropertyDef};
use php_hir::Type;
use php_syntax::ast::ClassLikeKind;

pub(crate) fn param(p: &ParamDef) -> String {
    let mut s = String::new();
    if let Some(v) = p.promoted {
        s.push_str(v.as_str());
        s.push(' ');
        if p.readonly {
            s.push_str("readonly ");
        }
    }
    if let Some(t) = p.effective_type() {
        s.push_str(&type_str(&t));
        s.push(' ');
    }
    if p.by_ref {
        s.push('&');
    }
    if p.variadic {
        s.push_str("...");
    }
    s.push('$');
    s.push_str(&p.name);
    if let Some(d) = &p.default {
        s.push_str(" = ");
        s.push_str(d);
    }
    s
}

/// Short type text: unqualified class names.
pub(crate) fn type_str(t: &Type) -> String {
    t.to_string()
}

pub(crate) fn function_signature(f: &FunctionDef, is_method: bool) -> String {
    let mut s = String::new();
    if is_method {
        if f.is_abstract {
            s.push_str("abstract ");
        }
        if f.is_final {
            s.push_str("final ");
        }
        s.push_str(f.visibility.as_str());
        s.push(' ');
        if f.is_static {
            s.push_str("static ");
        }
    }
    s.push_str("function ");
    if f.by_ref {
        s.push('&');
    }
    s.push_str(&f.name);
    s.push('(');
    let params: Vec<String> = f.params.iter().map(param).collect();
    if params.iter().map(|p| p.len()).sum::<usize>() > 60 && params.len() > 1 {
        s.push_str("\n    ");
        s.push_str(&params.join(",\n    "));
        s.push('\n');
    } else {
        s.push_str(&params.join(", "));
    }
    s.push(')');
    if let Some(t) = f.effective_return_type() {
        s.push_str(": ");
        s.push_str(&type_str(&t));
    }
    s
}

pub(crate) fn class_signature(c: &ClassDef) -> String {
    let mut s = String::new();
    if c.is_abstract && c.kind == ClassLikeKind::Class {
        s.push_str("abstract ");
    }
    if c.is_final {
        s.push_str("final ");
    }
    if c.is_readonly {
        s.push_str("readonly ");
    }
    s.push_str(match c.kind {
        ClassLikeKind::Class => "class ",
        ClassLikeKind::Interface => "interface ",
        ClassLikeKind::Trait => "trait ",
        ClassLikeKind::Enum => "enum ",
    });
    s.push_str(c.fqn.short());
    if let Some(b) = &c.backing_type {
        s.push_str(": ");
        s.push_str(&b.to_string());
    }
    if let Some(p) = &c.parent {
        s.push_str(" extends ");
        s.push_str(p.as_str());
    }
    if !c.interfaces.is_empty() {
        s.push_str(if c.kind == ClassLikeKind::Interface { " extends " } else { " implements " });
        let names: Vec<&str> = c.interfaces.iter().map(|i| i.as_str()).collect();
        s.push_str(&names.join(", "));
    }
    s
}

pub(crate) fn property_signature(p: &PropertyDef) -> String {
    let mut s = String::new();
    s.push_str(p.visibility.as_str());
    s.push(' ');
    if p.is_static {
        s.push_str("static ");
    }
    if p.is_readonly {
        s.push_str("readonly ");
    }
    if let Some(t) = p.effective_type() {
        s.push_str(&type_str(&t));
        s.push(' ');
    }
    s.push('$');
    s.push_str(&p.name);
    if let Some(d) = &p.default {
        s.push_str(" = ");
        s.push_str(d);
    }
    s
}

pub(crate) fn class_const_signature(c: &ClassConstDef) -> String {
    let mut s = String::new();
    if c.is_final {
        s.push_str("final ");
    }
    s.push_str(c.visibility.as_str());
    s.push_str(" const ");
    if let Some(t) = &c.ty {
        s.push_str(&t.to_string());
        s.push(' ');
    }
    s.push_str(&c.name);
    if let Some(v) = &c.value {
        s.push_str(" = ");
        s.push_str(v);
    }
    s
}

pub(crate) fn const_signature(c: &ConstDef) -> String {
    match &c.value {
        Some(v) => format!("const {} = {}", c.fqn.short(), v),
        None => format!("const {}", c.fqn.short()),
    }
}

pub(crate) fn case_signature(c: &EnumCaseDef) -> String {
    match &c.value {
        Some(v) => format!("case {} = {}", c.name, v),
        None => format!("case {}", c.name),
    }
}

/// `<?php` code block, optionally with a namespace line.
pub(crate) fn code_block(namespace: Option<&str>, code: &str) -> String {
    let mut s = String::from("```php\n<?php\n");
    if let Some(ns) = namespace.filter(|n| !n.is_empty()) {
        s.push_str(&format!("namespace {ns};\n"));
    }
    s.push_str(code);
    s.push_str("\n```");
    s
}

/// Summary, description and the interesting tags as Markdown.
pub(crate) fn doc_markdown(doc: &DocBlock, include_params: bool) -> String {
    let mut parts = Vec::new();
    if !doc.summary.is_empty() {
        parts.push(convert_inline(&doc.summary));
    }
    if !doc.description.is_empty() {
        parts.push(convert_inline(&doc.description));
    }
    let mut tags = Vec::new();
    for t in &doc.tags {
        let name = t.name.to_ascii_lowercase();
        let rendered = match name.as_str() {
            "param" if include_params => {
                let (ty, rest) = php_hir::phpdoc::split_type(&t.body);
                if ty.starts_with('$') {
                    format!("_@param_ `{}` {}", ty, rest)
                } else {
                    let (var, desc) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                    format!("_@param_ `{ty} {var}` {desc}")
                }
            }
            "return" | "throws" | "deprecated" | "since" | "see" | "link" | "example" | "uses"
            | "internal" => {
                let (ty, rest) = php_hir::phpdoc::split_type(&t.body);
                if matches!(name.as_str(), "return" | "throws") && !ty.is_empty() {
                    format!("_@{name}_ `{ty}` {rest}")
                } else {
                    format!("_@{name}_ {}", t.body)
                }
            }
            _ => continue,
        };
        tags.push(rendered.trim_end().to_string());
    }
    if !tags.is_empty() {
        parts.push(tags.join("  \n"));
    }
    parts.join("\n\n")
}

/// `{@link url}` -> link, `{@see X}` -> code.
fn convert_inline(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{@") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &rest[start + 2..start + end];
        let (tag, arg) = inner.split_once(char::is_whitespace).unwrap_or((inner, ""));
        match tag {
            "link" | "url" => {
                let (url, label) = arg.split_once(char::is_whitespace).unwrap_or((arg, arg));
                out.push_str(&format!("[{label}]({url})"));
            }
            _ => out.push_str(&format!("`{}`", arg.trim())),
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}
