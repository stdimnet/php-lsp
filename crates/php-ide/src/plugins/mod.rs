//! Plugin API for framework support.
//!
//! Plugins contribute types for calls and magic properties and candidates
//! for string arguments (completion and go to definition of config keys,
//! routes, templates, hook names, ...). Built in: Laravel, Symfony and
//! WordPress. Applications embedding [`crate::Workspace`] can register their
//! own plugins with [`crate::Workspace::register_plugin`].

pub mod laravel;
pub mod symfony;
pub mod wordpress;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use php_hir::{Fqn, Type};
use php_index::{FileId, FileKind};
use php_syntax::ast::{self, AstNode};
use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange};

use crate::{CompletionKind, Workspace};

/// What is being called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callee {
    Function(Fqn),
    Method { class: Fqn, name: String, is_static: bool },
}

impl Callee {
    /// `true` for the global function `name` (case insensitive).
    pub fn is_function(&self, name: &str) -> bool {
        matches!(self, Callee::Function(f) if f.as_str().eq_ignore_ascii_case(name) || f.short().eq_ignore_ascii_case(name) && f.namespace().is_none())
    }

    pub fn method_name(&self) -> Option<&str> {
        match self {
            Callee::Method { name, .. } => Some(name),
            _ => None,
        }
    }
}

/// A call argument as far as plugins care.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgInfo {
    /// `Foo::class`
    ClassString(Fqn),
    /// A string literal (without quotes).
    String(String),
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallInfo {
    pub callee: Callee,
    pub args: Vec<ArgInfo>,
}

/// A string literal argument the cursor is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringArg {
    pub callee: Callee,
    pub index: usize,
    /// Full content of the string literal.
    pub value: String,
}

/// A value a string argument can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringCandidate {
    pub value: String,
    pub detail: Option<String>,
    pub kind: CompletionKind,
    /// Where the value is defined: a workspace key (`file://...`) and line.
    pub location: Option<(String, u32)>,
}

/// Context handed to plugins.
pub struct PluginCx<'a> {
    pub ws: &'a Workspace,
    pub file: FileId,
}

impl PluginCx<'_> {
    /// The project root (nearest directory with composer.json) of the file.
    pub fn project_root(&self) -> Option<PathBuf> {
        let key = self.ws.file(self.file)?.path.clone();
        let mut dir = Path::new(key.strip_prefix("file://")?).parent();
        while let Some(d) = dir {
            if d.join("composer.json").exists() {
                return Some(d.to_path_buf());
            }
            dir = d.parent();
        }
        None
    }

    pub fn is_subtype(&self, class: &Fqn, of: &str) -> bool {
        self.ws.index().is_subtype(class, &Fqn::new(of))
    }

    /// Workspace (and vendor) files whose key starts with `prefix`.
    pub fn files_under<'s>(
        &'s self,
        prefix: &'s str,
    ) -> impl Iterator<Item = (FileId, &'s crate::SourceFile)> + 's {
        self.ws.files().filter(move |(_, f)| f.path.starts_with(prefix) && f.kind != FileKind::Stub)
    }
}

/// A framework plugin. All hooks have empty default implementations.
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;

    /// Overrides the return type of a function / method call.
    fn call_return_type(&self, _cx: &PluginCx, _call: &CallInfo) -> Option<Type> {
        None
    }

    /// Type of a property that is not declared (magic properties).
    fn property_type(&self, _cx: &PluginCx, _class: &Fqn, _name: &str) -> Option<Type> {
        None
    }

    /// Possible values of a string argument.
    fn string_candidates(&self, _cx: &PluginCx, _arg: &StringArg) -> Vec<StringCandidate> {
        Vec::new()
    }

    /// Type of a variable a template (`cx.file`) receives from its caller.
    fn template_variable_type(&self, _cx: &PluginCx, _name: &str) -> Option<Type> {
        None
    }
}

/// The registered plugins.
#[derive(Clone)]
pub struct Plugins(pub Vec<Arc<dyn Plugin>>);

impl Default for Plugins {
    fn default() -> Self {
        Plugins(vec![
            Arc::new(laravel::Laravel::default()),
            Arc::new(symfony::Symfony::default()),
            Arc::new(wordpress::WordPress::default()),
        ])
    }
}

impl std::fmt::Debug for Plugins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.0.iter().map(|p| p.name())).finish()
    }
}

/// A value cached per workspace revision.
#[derive(Default)]
pub struct Cached<T> {
    inner: Mutex<Option<(u64, Arc<T>)>>,
}

impl<T> Cached<T> {
    pub fn get(&self, revision: u64, compute: impl FnOnce() -> T) -> Arc<T> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((rev, v)) = guard.as_ref() {
            if *rev == revision {
                return v.clone();
            }
        }
        let v = Arc::new(compute());
        *guard = Some((revision, v.clone()));
        v
    }
}

// ------------------------------------------------------------------ helpers

/// Content of a string literal token (`'a'` / `"a"`).
pub fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && (s.starts_with('\'') && s.ends_with('\'') || s.starts_with('"') && s.ends_with('"')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.trim_matches(['\'', '"']).to_string()
    }
}

/// Keys of a PHP array literal (nested keys joined with `.`): used for
/// Laravel config / translation files.
pub fn array_keys(array: &SyntaxNode, prefix: &str, out: &mut Vec<(String, TextRange)>) {
    for el in array.children().filter(|c| c.kind() == ARRAY_ELEMENT) {
        let parts: Vec<SyntaxNode> = el.children().filter(|c| crate::infer::is_expr(c.kind())).collect();
        let has_key = el.children_with_tokens().any(|e| e.kind() == FAT_ARROW);
        if !has_key || parts.len() != 2 {
            continue;
        }
        let Some(tok) = parts[0].first_token().filter(|t| t.kind() == STRING) else { continue };
        let key = format!("{prefix}{}", unquote(tok.text()));
        out.push((key.clone(), tok.text_range()));
        if parts[1].kind() == ARRAY_EXPR {
            array_keys(&parts[1], &format!("{key}."), out);
        }
    }
}

/// The array returned by a file (`return [...];`).
pub fn returned_array(root: &SyntaxNode) -> Option<SyntaxNode> {
    root.descendants().find(|n| n.kind() == RETURN_STMT)?.children().find(|c| c.kind() == ARRAY_EXPR)
}

/// String literal arguments of calls to `methods` / `functions` in a tree:
/// `(value, range)` of argument `index`.
pub fn string_args_of_calls(root: &SyntaxNode, names: &[&str], index: usize) -> Vec<(String, TextRange)> {
    let mut out = Vec::new();
    for call in
        root.descendants().filter(|n| matches!(n.kind(), CALL_EXPR | METHOD_CALL_EXPR | STATIC_CALL_EXPR))
    {
        let name = match call.kind() {
            CALL_EXPR => {
                call.first_child().filter(|c| c.kind() == NAME_EXPR).map(|c| ast::text_without_trivia(&c))
            }
            _ => call.children().find_map(ast::MemberName::cast).and_then(|m| m.text()),
        };
        let Some(name) = name else { continue };
        let short = name.trim_start_matches('\\');
        if !names.iter().any(|n| n.eq_ignore_ascii_case(short)) {
            continue;
        }
        let Some(args) = call.children().find(|c| c.kind() == ARG_LIST) else { continue };
        let Some(arg) = args.children().filter(|c| c.kind() == ARG).nth(index) else { continue };
        let Some(lit) = arg.children().find(|c| c.kind() == LITERAL) else { continue };
        let Some(tok) = lit.first_token().filter(|t| t.kind() == STRING) else { continue };
        out.push((unquote(tok.text()), tok.text_range()));
    }
    out
}

/// Files below `dir` with the given suffix, as names relative to `dir`
/// (without the suffix, `/` replaced by `sep`).
pub fn named_files(dir: &Path, suffix: &str, sep: &str) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        let p = entry.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        if !entry.file_type().is_file() || !name.ends_with(suffix) {
            continue;
        }
        let Ok(rel) = p.strip_prefix(dir) else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let rel = &rel[..rel.len() - suffix.len()];
        out.push((rel.replace('/', sep), p.to_path_buf()));
    }
    out.sort();
    out
}

pub(crate) fn key_for(path: &Path) -> String {
    format!("file://{}", path.display())
}
