//! Document links (include / require targets, URLs in comments) and edits
//! for renamed / moved files.

use std::path::{Path, PathBuf};

use php_syntax::ast::{self, AstNode};
use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange, TextSize};

use crate::completion::TextEdit;
use crate::rename::SourceChange;
use crate::sema::Sema;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentLink {
    pub range: TextRange,
    /// A workspace key (`file://...`) or an URL.
    pub target: String,
}

fn file_dir(sema: &Sema) -> Option<PathBuf> {
    let key = &sema.ws.file(sema.file)?.path;
    Some(Path::new(key.strip_prefix("file://")?).parent()?.to_path_buf())
}

/// Evaluates simple path expressions: string literals, `__DIR__`,
/// `dirname(__FILE__)` and concatenations of those.
fn eval_path(expr: &SyntaxNode, dir: &Path) -> Option<String> {
    match expr.kind() {
        LITERAL => {
            let t = expr.first_token().filter(|t| t.kind() == STRING)?;
            Some(crate::plugins::unquote(t.text()))
        }
        MAGIC_CONST_EXPR => {
            let t = expr.first_token()?;
            match t.kind() {
                DIR_C_KW => Some(dir.display().to_string()),
                _ => None,
            }
        }
        CALL_EXPR => {
            let callee = expr.first_child().map(|c| ast::text_without_trivia(&c))?;
            if !callee.trim_start_matches('\\').eq_ignore_ascii_case("dirname") {
                return None;
            }
            let arg = expr
                .children()
                .find(|c| c.kind() == ARG_LIST)?
                .children()
                .find(|c| c.kind() == ARG)?
                .first_child()?;
            if arg.first_token().is_some_and(|t| t.kind() == FILE_C_KW) {
                return Some(dir.display().to_string());
            }
            None
        }
        BIN_EXPR => {
            let mut parts = expr.children();
            let a = parts.next()?;
            let b = parts.next()?;
            if !expr.children_with_tokens().any(|e| e.kind() == DOT) {
                return None;
            }
            Some(format!("{}{}", eval_path(&a, dir)?, eval_path(&b, dir)?))
        }
        PAREN_EXPR => eval_path(&expr.first_child()?, dir),
        _ => None,
    }
}

pub(crate) fn document_links(sema: &Sema) -> Vec<DocumentLink> {
    let mut out = Vec::new();
    let dir = file_dir(sema);
    for node in sema.root.descendants().filter(|n| n.kind() == INCLUDE_EXPR) {
        let (Some(dir), Some(arg)) =
            (dir.as_ref(), node.children().find(|c| crate::infer::is_expr(c.kind())))
        else {
            continue;
        };
        let Some(path) = eval_path(&arg, dir) else { continue };
        let p = Path::new(&path);
        let full = if p.is_absolute() { p.to_path_buf() } else { dir.join(p) };
        let full = normalize(&full);
        if full.exists() {
            out.push(DocumentLink { range: arg.text_range(), target: format!("file://{}", full.display()) });
        }
    }
    // URLs in comments
    for tok in sema.root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        if !matches!(tok.kind(), COMMENT | DOC_COMMENT) {
            continue;
        }
        let text = tok.text();
        let mut search = 0;
        while let Some(i) = text[search..].find("http") {
            let start = search + i;
            let rest = &text[start..];
            if !(rest.starts_with("http://") || rest.starts_with("https://")) {
                search = start + 4;
                continue;
            }
            let len = rest
                .find(|c: char| c.is_whitespace() || matches!(c, '>' | ')' | '"' | '\'' | '}' | ']'))
                .unwrap_or(rest.len());
            let url = rest[..len].trim_end_matches(['.', ',', ';']);
            let base = tok.text_range().start() + TextSize::from(start as u32);
            out.push(DocumentLink {
                range: TextRange::at(base, TextSize::from(url.len() as u32)),
                target: url.to_string(),
            });
            search = start + len.max(1);
        }
    }
    out
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// Edits for renaming / moving a PHP file: the class named like the file
/// is renamed, and its namespace follows PSR-4 (composer.json) including
/// `use` statements and fully qualified references in the workspace.
pub(crate) fn file_rename_edits(sema: &Sema, new_key: &str) -> SourceChange {
    let mut change = SourceChange::default();
    let Some(old_key) = sema.ws.file(sema.file).map(|f| f.path.to_string()) else { return change };
    let stem = |k: &str| Path::new(k).file_stem().and_then(|s| s.to_str()).map(str::to_string);
    let (Some(old_stem), Some(new_stem)) = (stem(&old_key), stem(new_key)) else { return change };
    // the class declared under the file name
    let Some(class) = sema
        .root
        .descendants()
        .filter(|n| matches!(n.kind(), CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL))
        .find(|n| n.children().find_map(ast::Name::cast).is_some_and(|name| name.text() == old_stem))
    else {
        return change;
    };
    let name = class.children().find_map(ast::Name::cast).unwrap();
    let old_fqn = sema.class_fqn(&class);
    // 1. class name
    if old_stem != new_stem && crate::rename::valid_identifier(&new_stem) {
        if let Ok(c) = crate::rename::rename(sema, name.syntax().text_range().start(), &new_stem) {
            change = c;
        }
    }
    // 2. namespace (PSR-4)
    let (Some(old_fqn), Some(new_ns)) = (old_fqn, crate::composer::expected_namespace(new_key)) else {
        return change;
    };
    let old_ns = old_fqn.namespace().unwrap_or("").to_string();
    if old_ns.eq_ignore_ascii_case(&new_ns) {
        return change;
    }
    if let Some(ns_name) = sema
        .root
        .descendants()
        .find(|n| n.kind() == NAMESPACE_DECL)
        .and_then(|n| n.children().find(|c| c.kind() == QUALIFIED_NAME))
    {
        change.push(sema.file, TextEdit { range: ns_name.text_range(), new_text: new_ns.clone() });
    }
    // qualified references elsewhere: `use Old\Ns\Foo;`, `\Old\Ns\Foo`
    let class_name = if old_stem != new_stem { new_stem.clone() } else { old_stem.clone() };
    let new_fqn = if new_ns.is_empty() { class_name.clone() } else { format!("{new_ns}\\{class_name}") };
    let old_lower = old_fqn.as_str().to_ascii_lowercase();
    for (id, f) in
        sema.ws.files().filter(|(id, f)| *id != sema.file && f.kind == php_index::FileKind::Workspace)
    {
        if !f.text.to_ascii_lowercase().contains(&old_lower) {
            continue;
        }
        let Some(parse) = sema.ws.parse(id) else { continue };
        for qn in parse.syntax_node().descendants().filter(|n| n.kind() == QUALIFIED_NAME) {
            let text = ast::text_without_trivia(&qn);
            if !text.trim_start_matches('\\').eq_ignore_ascii_case(old_fqn.as_str()) || !text.contains('\\') {
                continue;
            }
            let lead = if text.starts_with('\\') { "\\" } else { "" };
            // replaces any class-rename edit inside this name
            let range = qn.text_range();
            for (_, edits) in change.edits.iter_mut().filter(|(f, _)| *f == id) {
                edits.retain(|e| !range.contains_range(e.range));
            }
            change.push(id, TextEdit { range: qn.text_range(), new_text: format!("{lead}{new_fqn}") });
        }
    }
    change
}
