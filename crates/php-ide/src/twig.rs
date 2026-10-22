//! Twig templates: a small lexer for `{{ }}`, `{% %}` and `{# #}`, folding,
//! outline and highlighting of tags, navigation to templates, blocks and the
//! PHP members behind attributes, and completion / hover with the types of
//! the variables passed by `render()` calls of controllers.
//!
//! Twig is not PHP, so unlike Blade there is no virtual PHP code: the
//! features work on the token list of each tag. Templates get no
//! diagnostics.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use php_hir::{Fqn, Type};
use php_index::{FileId, FileKind};
use php_syntax::{SyntaxKind::*, TextRange, TextSize};

use crate::completion::{CompletionItem, CompletionKind, CompletionList};
use crate::folding::{Fold, FoldKind};
use crate::hover::Hover;
use crate::links::DocumentLink;
use crate::navigation::Location;
use crate::semantic_tokens::{SemanticToken, SemanticTokenType};
use crate::symbols::{Symbol, SymbolKind};
use crate::Workspace;

pub(crate) fn is_twig(path: &str) -> bool {
    path.ends_with(".twig")
}

// ------------------------------------------------------------------ lexer

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegionKind {
    /// `{{ ... }}`
    Output,
    /// `{% ... %}`
    Tag,
    /// `{# ... #}`
    Comment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokKind {
    Name,
    Number,
    Str,
    Punct,
}

#[derive(Debug, Clone, Copy)]
struct Tok {
    kind: TokKind,
    range: TextRange,
}

#[derive(Debug, Clone)]
struct Region {
    kind: RegionKind,
    /// The whole region with its delimiters.
    range: TextRange,
    open: TextRange,
    close: Option<TextRange>,
    toks: Vec<Tok>,
}

struct Template<'a> {
    text: &'a str,
    regions: Vec<Region>,
}

fn range(from: usize, to: usize) -> TextRange {
    TextRange::new(TextSize::from(from as u32), TextSize::from(to as u32))
}

const MULTI_OPS: &[&str] = &["...", "..", "**", "//", "==", "!=", "<=", ">=", "??", "?:", "=>", "?."];

impl<'a> Template<'a> {
    fn new(text: &'a str) -> Template<'a> {
        let bytes = text.as_bytes();
        let mut regions = Vec::new();
        let mut i = 0;
        while let Some(rel) = text[i..].find('{') {
            let start = i + rel;
            let kind = match bytes.get(start + 1) {
                Some(b'{') => RegionKind::Output,
                Some(b'%') => RegionKind::Tag,
                Some(b'#') => RegionKind::Comment,
                _ => {
                    i = start + 1;
                    continue;
                }
            };
            let mut open_end = start + 2;
            if matches!(bytes.get(open_end), Some(b'-' | b'~')) {
                open_end += 1;
            }
            let region = if kind == RegionKind::Comment {
                let (close, end) = match text[open_end..].find("#}") {
                    Some(e) => (Some(range(open_end + e, open_end + e + 2)), open_end + e + 2),
                    None => (None, text.len()),
                };
                Region {
                    kind,
                    range: range(start, end),
                    open: range(start, open_end),
                    close,
                    toks: Vec::new(),
                }
            } else {
                let closer = if kind == RegionKind::Output { b'}' } else { b'%' };
                let (toks, close) = lex_code(text, open_end, closer);
                let end = close.map_or(text.len(), |c| usize::from(c.end()));
                Region { kind, range: range(start, end), open: range(start, open_end), close, toks }
            };
            i = usize::from(region.range.end()).max(start + 1);
            // `{% verbatim %}` / `{% raw %}`: the content is text
            let tag = (region.kind == RegionKind::Tag).then(|| tag_name(text, &region)).flatten();
            regions.push(region);
            if let Some(t @ ("verbatim" | "raw")) = tag {
                let end_tag = format!("end{t}");
                let mut j = i;
                while let Some(rel) = text[j..].find("{%") {
                    let s = j + rel;
                    let rest = text[s + 2..].trim_start_matches(['-', '~']).trim_start();
                    if rest.starts_with(end_tag.as_str()) {
                        break;
                    }
                    j = s + 2;
                }
                i = text[j..].find("{%").map_or(text.len(), |r| j + r);
            }
        }
        Template { text, regions }
    }

    fn tok_text(&self, t: &Tok) -> &'a str {
        &self.text[t.range]
    }

    /// The region whose code contains `offset` (between the delimiters).
    fn region_at(&self, offset: TextSize) -> Option<usize> {
        self.regions.iter().position(|r| {
            r.open.end() <= offset && r.close.map_or(r.range.end() >= offset, |c| offset <= c.start())
        })
    }
}

/// Tokens of a tag body starting at `from` up to `}}` / `%}` (`closer` is
/// the first character of the closing delimiter).
fn lex_code(text: &str, from: usize, closer: u8) -> (Vec<Tok>, Option<TextRange>) {
    let bytes = text.as_bytes();
    let mut toks = Vec::new();
    let mut i = from;
    while i < bytes.len() {
        let c = bytes[i];
        // closing delimiter, optionally with whitespace control
        let ctl =
            matches!(c, b'-' | b'~') && bytes.get(i + 1) == Some(&closer) && bytes.get(i + 2) == Some(&b'}');
        if ctl || c == closer && bytes.get(i + 1) == Some(&b'}') {
            let end = if ctl { i + 3 } else { i + 2 };
            return (toks, Some(range(i, end)));
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let kind = if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80)
            {
                i += 1;
            }
            TokKind::Name
        } else if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            TokKind::Number
        } else if c == b'\'' || c == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != c {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(bytes.len());
            TokKind::Str
        } else {
            let op = MULTI_OPS.iter().find(|op| text[i..].starts_with(*op));
            i += op.map_or(1, |o| o.len());
            // keep UTF-8 boundaries
            while !text.is_char_boundary(i.min(text.len())) {
                i += 1;
            }
            TokKind::Punct
        };
        toks.push(Tok { kind, range: range(start, i.min(text.len())) });
    }
    (toks, None)
}

fn tag_name<'t>(text: &'t str, region: &Region) -> Option<&'t str> {
    let first = region.toks.first().filter(|t| t.kind == TokKind::Name)?;
    Some(&text[first.range])
}

// ------------------------------------------------------------------ structure

/// Tags with an end tag (`set` and `block` only without an inline value).
const PAIRED: &[&str] = &[
    "block",
    "for",
    "if",
    "macro",
    "embed",
    "apply",
    "filter",
    "spaceless",
    "autoescape",
    "sandbox",
    "with",
    "set",
    "verbatim",
    "raw",
    "cache",
    "trans",
    "guard",
];

const TAGS: &[&str] = &[
    "apply",
    "autoescape",
    "block",
    "cache",
    "deprecated",
    "do",
    "embed",
    "extends",
    "flush",
    "for",
    "from",
    "guard",
    "if",
    "elseif",
    "else",
    "import",
    "include",
    "macro",
    "sandbox",
    "set",
    "use",
    "verbatim",
    "with",
    "types",
    // Symfony
    "form_theme",
    "trans",
    "trans_default_domain",
    "stopwatch",
];

const FILTERS: &[(&str, &str)] = &[
    ("abs", "Absolute value"),
    ("batch", "Batches items"),
    ("capitalize", "Capitalizes a value"),
    ("column", "Values of a column"),
    ("convert_encoding", "Converts a string to another encoding"),
    ("country_name", "Country name"),
    ("currency_name", "Currency name"),
    ("data_uri", "Data URI"),
    ("date", "Formats a date"),
    ("date_modify", "Modifies a date"),
    ("default", "The value or a default"),
    ("escape", "Escapes a string"),
    ("e", "Escapes a string"),
    ("filter", "Filters elements with an arrow function"),
    ("find", "First element matching an arrow function"),
    ("first", "First element"),
    ("format", "sprintf-like formatting"),
    ("format_currency", "Formats a currency"),
    ("format_date", "Formats a date"),
    ("format_datetime", "Formats a date and time"),
    ("format_number", "Formats a number"),
    ("html_to_markdown", "Converts HTML to Markdown"),
    ("inline_css", "Inlines CSS"),
    ("join", "Joins elements"),
    ("json_encode", "JSON representation"),
    ("keys", "Keys of an array"),
    ("language_name", "Language name"),
    ("last", "Last element"),
    ("length", "Number of elements / characters"),
    ("lower", "Lower case"),
    ("map", "Applies an arrow function"),
    ("markdown_to_html", "Converts Markdown to HTML"),
    ("merge", "Merges arrays"),
    ("nl2br", "Line breaks to <br>"),
    ("number_format", "Formats a number"),
    ("raw", "Marks a value as safe"),
    ("reduce", "Reduces a sequence"),
    ("replace", "Replaces placeholders"),
    ("reverse", "Reverses a sequence"),
    ("round", "Rounds a number"),
    ("slice", "Extracts a slice"),
    ("slug", "Slug of a string"),
    ("sort", "Sorts an array"),
    ("spaceless", "Removes whitespace between tags"),
    ("split", "Splits a string"),
    ("striptags", "Removes tags"),
    ("title", "Title case"),
    ("trim", "Trims a string"),
    ("u", "Unicode string object"),
    ("upper", "Upper case"),
    ("url_encode", "URL encoding"),
    // Symfony
    ("trans", "Translates a message"),
    ("humanize", "Humanizes a technical name"),
    ("yaml_encode", "YAML representation"),
    ("abbr_class", "Abbreviated class name"),
    ("file_excerpt", "Excerpt of a file"),
    ("format_file", "Link to a file"),
    ("serialize", "Serializes a value"),
    ("sanitize_html", "Sanitizes HTML"),
];

const FUNCTIONS: &[(&str, &str)] = &[
    ("attribute", "Dynamic attribute access"),
    ("block", "Content of a block"),
    ("constant", "Value of a constant"),
    ("cycle", "Cycles through values"),
    ("date", "Converts to a date"),
    ("dump", "Dumps a variable"),
    ("enum", "An enum"),
    ("enum_cases", "Cases of an enum"),
    ("include", "Renders a template"),
    ("max", "Biggest value"),
    ("min", "Smallest value"),
    ("parent", "Content of the parent block"),
    ("random", "Random value"),
    ("range", "List of numbers"),
    ("source", "Source of a template"),
    ("template_from_string", "Template from a string"),
    // Symfony
    ("absolute_url", "Absolute URL"),
    ("asset", "Public path of an asset"),
    ("asset_version", "Version of an asset"),
    ("csrf_token", "CSRF token"),
    ("form", "Renders a form"),
    ("form_start", "Start tag of a form"),
    ("form_end", "End tag of a form"),
    ("form_widget", "Renders a form widget"),
    ("form_errors", "Renders form errors"),
    ("form_label", "Renders a form label"),
    ("form_help", "Renders form help"),
    ("form_row", "Renders a form row"),
    ("form_rest", "Renders remaining fields"),
    ("field_name", "Name of a form field"),
    ("field_value", "Value of a form field"),
    ("is_granted", "Checks a permission"),
    ("logout_path", "Logout path"),
    ("logout_url", "Logout URL"),
    ("path", "Relative URL of a route"),
    ("url", "Absolute URL of a route"),
    ("relative_path", "Relative path"),
    ("render", "Renders a controller"),
    ("render_esi", "Renders a controller with ESI"),
    ("controller", "Controller reference"),
    ("t", "Translatable message"),
    ("impersonation_exit_path", "Path to stop impersonation"),
];

const TESTS: &[&str] = &[
    "constant",
    "defined",
    "divisible by",
    "empty",
    "even",
    "iterable",
    "null",
    "odd",
    "same as",
    "sequence",
    "mapping",
];

const KEYWORDS: &[&str] = &[
    "in", "is", "not", "and", "or", "b-and", "b-or", "b-xor", "as", "with", "only", "ignore", "missing",
    "if", "true", "false", "null", "none", "matches", "starts", "ends", "has", "some", "every", "xor",
];

/// Opening / closing region pairs of paired tags.
fn pairs(t: &Template) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, &str)> = Vec::new();
    for (i, r) in t.regions.iter().enumerate() {
        if r.kind != RegionKind::Tag {
            continue;
        }
        let Some(name) = tag_name(t.text, r) else { continue };
        if let Some(open) = name.strip_prefix("end") {
            if let Some(pos) = stack.iter().rposition(|(_, n)| *n == open) {
                let (o, _) = stack[pos];
                stack.truncate(pos);
                out.push((o, i));
            }
            continue;
        }
        if !PAIRED.contains(&name) {
            continue;
        }
        // `{% set x = 1 %}`, `{% block title 'Hello' %}` are complete
        let inline = match name {
            "set" => r.toks.iter().any(|tk| t.tok_text(tk) == "="),
            "block" => r.toks.len() > 2,
            _ => false,
        };
        if !inline {
            stack.push((i, name));
        }
    }
    out.sort();
    out
}

pub(crate) fn folds(text: &str) -> Vec<Fold> {
    let t = Template::new(text);
    let mut out: Vec<Fold> = pairs(&t)
        .into_iter()
        .map(|(o, c)| TextRange::new(t.regions[o].range.start(), t.regions[c].range.start()))
        .filter(|r| text[*r].contains('\n'))
        .map(|range| Fold { range, kind: FoldKind::Block })
        .collect();
    for r in t.regions.iter().filter(|r| r.kind == RegionKind::Comment && text[r.range].contains('\n')) {
        out.push(Fold { range: r.range, kind: FoldKind::Comment });
    }
    out.sort_by_key(|f| f.range.start());
    out
}

/// Outline: blocks and macros (nested).
pub(crate) fn symbols(text: &str) -> Vec<Symbol> {
    let t = Template::new(text);
    let mut flat: Vec<Symbol> = Vec::new();
    let all = pairs(&t);
    for r in t.regions.iter() {
        let (Some(name), Some(arg)) = (tag_name(text, r), r.toks.get(1)) else { continue };
        let kind = match name {
            "block" => SymbolKind::Property,
            "macro" => SymbolKind::Function,
            _ => continue,
        };
        let idx = t.regions.iter().position(|x| x.range == r.range).unwrap_or(0);
        let end =
            all.iter().find(|(o, _)| *o == idx).map_or(r.range.end(), |(_, c)| t.regions[*c].range.end());
        flat.push(Symbol {
            name: t.tok_text(arg).to_string(),
            detail: Some(name.to_string()),
            kind,
            deprecated: false,
            range: TextRange::new(r.range.start(), end),
            selection_range: arg.range,
            children: Vec::new(),
        });
    }
    nest(flat)
}

fn nest(flat: Vec<Symbol>) -> Vec<Symbol> {
    let mut roots: Vec<Symbol> = Vec::new();
    for s in flat {
        insert(&mut roots, s);
    }
    roots
}

fn insert(list: &mut Vec<Symbol>, s: Symbol) {
    if let Some(parent) =
        list.iter_mut().rev().find(|p| p.range.contains_range(s.range) && p.range != s.range)
    {
        insert(&mut parent.children, s);
    } else {
        list.push(s);
    }
}

pub(crate) fn tokens(text: &str, within: Option<TextRange>) -> Vec<SemanticToken> {
    let t = Template::new(text);
    let mut out = Vec::new();
    let tok = |range, token_type| SemanticToken { range, token_type, modifiers: 0 };
    for r in &t.regions {
        if within.is_some_and(|w| w.intersect(r.range).is_none()) || r.kind == RegionKind::Comment {
            continue;
        }
        out.push(tok(r.open, SemanticTokenType::Keyword));
        for (i, tk) in r.toks.iter().enumerate() {
            if tk.kind != TokKind::Name {
                continue;
            }
            let s = t.tok_text(tk);
            let prev = i.checked_sub(1).map(|p| t.tok_text(&r.toks[p]));
            let next = r.toks.get(i + 1).map(|n| t.tok_text(n));
            let ty = if r.kind == RegionKind::Tag && i == 0 {
                SemanticTokenType::Keyword
            } else if prev == Some(".") || prev == Some("?.") {
                if next == Some("(") {
                    SemanticTokenType::Method
                } else {
                    SemanticTokenType::Property
                }
            } else if prev == Some("|") || next == Some("(") {
                SemanticTokenType::Function
            } else if KEYWORDS.contains(&s) {
                SemanticTokenType::Keyword
            } else {
                SemanticTokenType::Variable
            };
            out.push(tok(tk.range, ty));
        }
        if let Some(c) = r.close {
            out.push(tok(c, SemanticTokenType::Keyword));
        }
    }
    out
}

// ------------------------------------------------------------------ templates on disk

/// The project root of a template: the nearest directory with a
/// `composer.json` (else the parent of a `templates` directory).
fn project_root(key: &str) -> Option<PathBuf> {
    let path = Path::new(key.strip_prefix("file://")?);
    let mut dir = path.parent();
    let mut templates_parent = None;
    while let Some(d) = dir {
        if d.join("composer.json").exists() {
            return Some(d.to_path_buf());
        }
        if templates_parent.is_none() && d.file_name().is_some_and(|n| n == "templates") {
            templates_parent = d.parent().map(Path::to_path_buf);
        }
        dir = d.parent();
    }
    templates_parent
}

/// `(namespace, directory)` of bundle templates: `@Foo/...` lives in
/// `FooBundle/Resources/views` or `FooBundle/templates`.
type Namespaces = Vec<(String, PathBuf)>;

type NamespaceCache = HashMap<PathBuf, (Instant, Arc<Namespaces>)>;

fn namespaces(root: &Path) -> Arc<Namespaces> {
    static CACHE: Mutex<Option<NamespaceCache>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some((when, v)) = map.get(root) {
        if when.elapsed() < Duration::from_secs(30) {
            return v.clone();
        }
    }
    let mut out = Vec::new();
    let walker = walkdir::WalkDir::new(root).max_depth(8).into_iter().filter_entry(|e| {
        let n = e.file_name().to_string_lossy();
        e.file_type().is_dir()
            && !matches!(n.as_ref(), ".git" | "node_modules" | "var" | "cache" | "Tests" | "tests")
    });
    for e in walker.flatten() {
        let name = e.file_name().to_string_lossy();
        let Some(ns) = name.strip_suffix("Bundle").filter(|n| !n.is_empty()) else { continue };
        for sub in ["Resources/views", "templates"] {
            let dir = e.path().join(sub);
            if dir.is_dir() {
                out.push((ns.to_string(), dir));
            }
        }
    }
    out.sort();
    let v = Arc::new(out);
    map.insert(root.to_path_buf(), (Instant::now(), v.clone()));
    v
}

/// The file of template `name` (`base.html.twig`, `@Foo/bar.html.twig`).
fn resolve_template(root: &Path, name: &str) -> Option<PathBuf> {
    if let Some(rest) = name.strip_prefix('@') {
        let (ns, file) = rest.split_once('/')?;
        let overridden = root.join("templates/bundles").join(format!("{ns}Bundle")).join(file);
        if overridden.is_file() {
            return Some(overridden);
        }
        return namespaces(root)
            .iter()
            .filter(|(n, _)| n == ns)
            .map(|(_, d)| d.join(file))
            .find(|p| p.is_file());
    }
    let p = root.join("templates").join(name);
    p.is_file().then_some(p)
}

/// All template names of the project (for completion).
fn template_names(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = crate::plugins::named_files(&root.join("templates"), ".twig", "/")
        .into_iter()
        .filter(|(n, _)| !n.starts_with("bundles/"))
        .map(|(n, _)| format!("{n}.twig"))
        .collect();
    for (ns, dir) in namespaces(root).iter() {
        out.extend(
            crate::plugins::named_files(dir, ".twig", "/")
                .into_iter()
                .map(|(n, _)| format!("@{ns}/{n}.twig")),
        );
    }
    out.sort();
    out.dedup();
    out
}

/// The names a template is rendered by.
fn names_of(key: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(i) = key.rfind("/templates/") {
        out.push(key[i + "/templates/".len()..].to_string());
    }
    for marker in ["/Resources/views/", "/templates/"] {
        if let Some(i) = key.find(marker) {
            let before = &key[..i];
            let bundle = before.rsplit('/').next().unwrap_or("");
            if let Some(ns) = bundle.strip_suffix("Bundle") {
                out.push(format!("@{ns}/{}", &key[i + marker.len()..]));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ context

/// Analysis of one template.
struct Cx<'w, 'a> {
    ws: &'w Workspace,
    file: FileId,
    key: &'a str,
    t: Template<'a>,
}

const TEMPLATE_TAGS: &[&str] = &["extends", "include", "embed", "import", "from", "use", "form_theme"];
const TEMPLATE_FUNCTIONS: &[&str] = &["include", "source"];

/// `$this->render('name', [...])` and friends.
const RENDER_METHODS: &[&str] =
    &["render", "renderview", "renderblock", "renderblockview", "stream", "display"];

/// Variables passed to a template: name, type and where the key is.
type ContextVars = Vec<(String, Type, Option<(FileId, TextRange)>)>;

/// Context variables per (workspace revision, template).
type ContextCache = HashMap<(u64, String), Arc<ContextVars>>;

fn context_cache() -> &'static Mutex<ContextCache> {
    static CACHE: std::sync::OnceLock<Mutex<ContextCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

impl<'w, 'a> Cx<'w, 'a> {
    fn new(ws: &'w Workspace, file: FileId) -> Option<Cx<'w, 'w>> {
        let f = ws.file(file)?;
        Some(Cx { ws, file, key: &f.path, t: Template::new(&f.text) })
    }

    fn text(&self, tk: &Tok) -> &str {
        self.t.tok_text(tk)
    }

    /// Variables controllers pass to this template.
    fn context_vars(&self) -> Arc<ContextVars> {
        let cache_key = (self.ws.revision(), self.key.to_string());
        if let Some(v) = context_cache().lock().unwrap_or_else(|e| e.into_inner()).get(&cache_key) {
            return v.clone();
        }
        let v = Arc::new(self.collect_context_vars());
        let mut guard = context_cache().lock().unwrap_or_else(|e| e.into_inner());
        if guard.len() > 64 {
            guard.clear();
        }
        guard.insert(cache_key, v.clone());
        v
    }

    fn collect_context_vars(&self) -> ContextVars {
        use crate::plugins::laravel::{arg_exprs, call_name, string_value};
        let names = names_of(self.key);
        if names.is_empty() {
            return Vec::new();
        }
        let quoted: Vec<String> = names.iter().flat_map(|n| [format!("'{n}'"), format!("\"{n}\"")]).collect();
        let files: Vec<FileId> = self
            .ws
            .files()
            .filter(|(_, f)| {
                f.kind == FileKind::Workspace
                    && !is_twig(&f.path)
                    && !crate::blade::is_blade(&f.path)
                    && quoted.iter().any(|q| f.text.contains(q.as_str()))
            })
            .map(|(id, _)| id)
            .collect();
        let mut out: ContextVars = Vec::new();
        let mut add = |name: String, ty: Type, loc: Option<(FileId, TextRange)>| match out
            .iter_mut()
            .find(|(n, _, _)| *n == name)
        {
            Some(e) => e.1 = Type::union(vec![e.1.clone(), ty]),
            None => out.push((name, ty, loc)),
        };
        for id in files {
            let Some(sema) = crate::sema::Sema::new(self.ws, id) else { continue };
            let mut data_arrays: Vec<php_syntax::SyntaxNode> = Vec::new();
            for call in sema
                .root
                .descendants()
                .filter(|n| matches!(n.kind(), METHOD_CALL_EXPR | STATIC_CALL_EXPR | CALL_EXPR))
            {
                let Some(name) = call_name(&call) else { continue };
                if !RENDER_METHODS.contains(&name.to_ascii_lowercase().as_str()) {
                    continue;
                }
                let args = arg_exprs(&call);
                if args.first().and_then(string_value).is_some_and(|v| names.contains(&v)) {
                    data_arrays.extend(args.get(1).filter(|a| a.kind() == ARRAY_EXPR).cloned());
                }
            }
            // #[Template('name')]: the arrays the method returns
            for attr in sema.root.descendants().filter(|n| n.kind() == ATTRIBUTE) {
                let is_template = attr.children().find(|c| c.kind() == QUALIFIED_NAME).is_some_and(|q| {
                    php_syntax::ast::text_without_trivia(&q).rsplit('\\').next() == Some("Template")
                });
                let named = attr
                    .descendants()
                    .filter(|n| n.kind() == LITERAL)
                    .any(|l| string_value(&l).is_some_and(|v| names.contains(&v)));
                if !is_template || !named {
                    continue;
                }
                let Some(method) = attr.ancestors().find(|a| a.kind() == METHOD_DECL) else { continue };
                for ret in method.descendants().filter(|n| n.kind() == RETURN_STMT) {
                    data_arrays.extend(ret.children().filter(|c| c.kind() == ARRAY_EXPR));
                }
            }
            for array in data_arrays {
                for el in array.children().filter(|c| c.kind() == ARRAY_ELEMENT) {
                    let parts: Vec<_> = el.children().filter(|c| crate::infer::is_expr(c.kind())).collect();
                    if parts.len() != 2 {
                        continue;
                    }
                    let Some(key) = string_value(&parts[0]) else { continue };
                    let ty = sema.infer().expr_type(&parts[1]);
                    add(key, ty, Some((id, parts[0].text_range())));
                }
            }
        }
        out
    }

    /// `{% set name ... %}` before `offset` (the closest).
    fn set_before(&self, name: &str, offset: TextSize) -> Option<(usize, &Region)> {
        self.t
            .regions
            .iter()
            .enumerate()
            .filter(|(_, r)| r.range.end() <= offset && r.kind == RegionKind::Tag)
            .filter(|(_, r)| tag_name(self.t.text, r) == Some("set"))
            .rfind(|(_, r)| r.toks.get(1).is_some_and(|t| self.text(t) == name))
    }

    /// Type of the variable `name` at `offset`.
    fn variable_type(&self, name: &str, offset: TextSize, depth: u8) -> Option<Type> {
        if depth > 4 {
            return None;
        }
        // `{% for k, v in items %}` around the offset
        let pairs = pairs(&self.t);
        for (o, c) in pairs.iter().rev() {
            let (open, close) = (&self.t.regions[*o], &self.t.regions[*c]);
            if !(open.range.end() <= offset && offset <= close.range.start()) {
                continue;
            }
            match tag_name(self.t.text, open) {
                Some("for") => {
                    let toks = &open.toks;
                    let Some(in_pos) = toks.iter().position(|t| self.text(t) == "in") else { continue };
                    let vars: Vec<&str> = toks[1..in_pos]
                        .iter()
                        .filter(|t| t.kind == TokKind::Name)
                        .map(|t| self.text(t))
                        .collect();
                    if name == "loop" {
                        return Some(loop_type());
                    }
                    let value_name = vars.last().copied();
                    if vars.len() == 2 && vars[0] == name {
                        return Some(Type::union(vec![Type::Int, Type::String]));
                    }
                    if value_name == Some(name) {
                        let end = toks[in_pos + 1..]
                            .iter()
                            .position(|t| matches!(self.text(t), "if"))
                            .map_or(toks.len(), |p| in_pos + 1 + p);
                        let items = self.eval(&toks[in_pos + 1..end], open.range.start(), depth + 1)?;
                        return items.iterable_value().or_else(|| self.element_of_class(&items));
                    }
                }
                Some("macro") if open.toks.iter().skip(2).any(|t| self.text(t) == name) => {
                    return Some(Type::Mixed);
                }
                _ => {}
            }
        }
        if let Some((_, r)) = self.set_before(name, offset) {
            return match r.toks.iter().position(|t| self.text(t) == "=") {
                Some(eq) => self.eval(&r.toks[eq + 1..], r.range.start(), depth + 1),
                None => Some(Type::String),
            };
        }
        if name == "app" {
            let app = Fqn::new("Symfony\\Bridge\\Twig\\AppVariable");
            return self.ws.index().class(&app).map(|_| Type::Named(app, Vec::new()));
        }
        self.context_vars().iter().find(|(n, _, _)| n == name).map(|(_, t, _)| t.clone())
    }

    /// `Traversable` classes: the value type of their iterator.
    fn element_of_class(&self, t: &Type) -> Option<Type> {
        let index = self.ws.index();
        for class in t.class_names() {
            if let Some(args) = index.ancestor_args(&class, t_args(t), &Fqn::new("Traversable")) {
                if let Some(v) = args.last() {
                    return Some(v.clone());
                }
            }
            if let Some(m) = index.find_method(&class, "getIterator") {
                if let Some(rt) = m.def.effective_return_type() {
                    if let Some(v) = rt.iterable_value() {
                        return Some(v);
                    }
                }
            }
        }
        None
    }

    /// Type of an expression (a chain like `user.address.city` or a literal).
    fn eval(&self, toks: &[Tok], offset: TextSize, depth: u8) -> Option<Type> {
        let first = toks.first()?;
        let mut ty = match first.kind {
            TokKind::Str => return Some(Type::String),
            TokKind::Number => {
                return Some(if self.text(first).contains('.') { Type::Float } else { Type::Int });
            }
            TokKind::Punct if matches!(self.text(first), "[" | "{") => return Some(Type::Array(None, None)),
            TokKind::Punct => return None,
            TokKind::Name => {
                let name = self.text(first);
                match name {
                    "true" | "false" => return Some(Type::Bool),
                    "null" | "none" => return Some(Type::Null),
                    _ if toks.get(1).is_some_and(|t| self.text(t) == "(") => return None,
                    _ => self.variable_type(name, offset, depth)?,
                }
            }
        };
        let mut i = 1;
        while i < toks.len() {
            match self.text(&toks[i]) {
                "." | "?." => {
                    let name = toks.get(i + 1).filter(|t| t.kind == TokKind::Name)?;
                    ty = self.attribute_type(&ty, self.text(name))?;
                    i += 2;
                    if toks.get(i).is_some_and(|t| self.text(t) == "(") {
                        i = skip_group(self, toks, i);
                    }
                }
                "[" => {
                    let key =
                        toks.get(i + 1).filter(|t| t.kind == TokKind::Str).map(|t| unquote(self.text(t)));
                    ty = match (&ty, key) {
                        (Type::Shape(entries), Some(k)) => entries.iter().find(|e| e.key == k)?.ty.clone(),
                        _ => ty.iterable_value()?,
                    };
                    i = skip_group(self, toks, i);
                }
                "|" => {
                    let name = toks.get(i + 1).map(|t| self.text(t))?;
                    ty = match name {
                        "first" | "last" => ty.iterable_value()?,
                        "default" | "raw" | "reverse" | "sort" | "slice" | "merge" | "filter" => ty,
                        "length" => Type::Int,
                        "keys" => Type::List(Some(Box::new(Type::union(vec![Type::Int, Type::String])))),
                        "join" | "upper" | "lower" | "title" | "capitalize" | "trim" | "escape" | "e"
                        | "striptags" | "nl2br" | "format" | "replace" | "json_encode" | "trans" | "date"
                        | "number_format" | "url_encode" => Type::String,
                        _ => return None,
                    };
                    i += 2;
                    if toks.get(i).is_some_and(|t| self.text(t) == "(") {
                        i = skip_group(self, toks, i);
                    }
                }
                _ => return Some(ty),
            }
        }
        Some(ty)
    }

    /// Twig's attribute lookup: array key, property, `name()`, `getName()`,
    /// `isName()`, `hasName()`.
    fn attribute_type(&self, ty: &Type, name: &str) -> Option<Type> {
        if let Type::Shape(entries) = ty {
            if let Some(e) = entries.iter().find(|e| e.key == name) {
                return Some(e.ty.clone());
            }
        }
        let mut out = Vec::new();
        for class in ty.class_names() {
            if let Some((_, target)) = self.member(&class, name) {
                out.push(match target {
                    Member::Property(p) => p.effective_type().unwrap_or(Type::Mixed),
                    Member::Method(m, owner) => {
                        let t = m.effective_return_type().unwrap_or(Type::Mixed);
                        match t {
                            Type::SelfType | Type::Static => Type::Named(owner, Vec::new()),
                            t => t,
                        }
                    }
                });
            }
        }
        (!out.is_empty()).then(|| Type::union(out))
    }

    /// The PHP member `obj.name` resolves to.
    fn member(&self, class: &Fqn, name: &str) -> Option<(FileId, Member<'w>)> {
        let index = self.ws.index();
        if let Some(p) = index.find_property(class, name) {
            if p.def.visibility == php_hir::symbols::Visibility::Public && !p.def.is_static {
                return Some((p.file, Member::Property(p.def)));
            }
        }
        let cap = capitalize(name);
        for candidate in [name.to_string(), format!("get{cap}"), format!("is{cap}"), format!("has{cap}")] {
            if let Some(m) = index.find_method(class, &candidate) {
                if m.def.visibility == php_hir::symbols::Visibility::Public {
                    return Some((m.file, Member::Method(m.def, m.owner.fqn.clone())));
                }
            }
        }
        None
    }
}

enum Member<'a> {
    Property(&'a php_hir::symbols::PropertyDef),
    Method(&'a php_hir::symbols::FunctionDef, Fqn),
}

fn t_args(t: &Type) -> &[Type] {
    match t {
        Type::Named(_, args) => args,
        _ => &[],
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn unquote(s: &str) -> String {
    crate::plugins::unquote(s)
}

/// Index after the bracket group starting at `i` (`(`, `[` or `{`).
fn skip_group(cx: &Cx, toks: &[Tok], i: usize) -> usize {
    let mut depth = 0;
    for (j, t) in toks.iter().enumerate().skip(i) {
        match cx.text(t) {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
    }
    toks.len()
}

/// Twig's `loop` variable.
fn loop_type() -> Type {
    let e = |key: &str, ty: Type| php_hir::types::ShapeEntry { key: key.into(), ty, optional: false };
    Type::Shape(vec![
        e("index", Type::Int),
        e("index0", Type::Int),
        e("revindex", Type::Int),
        e("revindex0", Type::Int),
        e("first", Type::Bool),
        e("last", Type::Bool),
        e("length", Type::Int),
        e("parent", Type::Array(None, None)),
    ])
}

/// The start of the expression chain ending before token `end`.
fn chain_start(cx: &Cx, toks: &[Tok], end: usize) -> usize {
    let mut i = end;
    while i > 0 {
        let t = &toks[i - 1];
        match cx.text(t) {
            ")" | "]" => {
                // back to the matching opener
                let mut depth = 0;
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    match cx.text(&toks[j]) {
                        ")" | "]" => depth += 1,
                        "(" | "[" => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                i = j;
            }
            "." | "?." | "|" => i -= 1,
            _ if t.kind == TokKind::Name => {
                let before = i.checked_sub(2).map(|p| cx.text(&toks[p]));
                i -= 1;
                if !matches!(before, Some("." | "?." | "|")) {
                    return i;
                }
            }
            _ => return i,
        }
    }
    0
}

// ------------------------------------------------------------------ features

/// What the cursor is on.
enum Target {
    /// A string literal in a template position (`{% extends 'x' %}`).
    TemplateName(String, TextRange),
    /// A route name (`path('x')`).
    RouteName(String),
    /// `{% block name %}`
    BlockName(String),
    Variable(String),
    /// `chain.name`: the attribute after the chain ending at token `end`.
    Attribute {
        region: usize,
        end: usize,
        name: String,
    },
    Filter(String),
    Function(String),
    Tag(String),
}

impl Cx<'_, '_> {
    fn target(&self, offset: TextSize) -> Option<(Target, TextRange)> {
        let ri = self.t.region_at(offset)?;
        let r = &self.t.regions[ri];
        if r.kind == RegionKind::Comment {
            return None;
        }
        let i = r.toks.iter().position(|t| t.range.start() <= offset && offset <= t.range.end())?;
        let tk = &r.toks[i];
        let s = self.text(tk);
        let prev = i.checked_sub(1).map(|p| self.text(&r.toks[p]));
        let next = r.toks.get(i + 1).map(|n| self.text(n));
        let tag = (r.kind == RegionKind::Tag).then(|| tag_name(self.t.text, r)).flatten();
        let target = match tk.kind {
            TokKind::Str => {
                let value = unquote(s);
                let call = self.enclosing_call(&r.toks, i);
                if tag.is_some_and(|t| TEMPLATE_TAGS.contains(&t))
                    || call.as_deref().is_some_and(|c| TEMPLATE_FUNCTIONS.contains(&c))
                {
                    Target::TemplateName(value, tk.range)
                } else if matches!(call.as_deref(), Some("path" | "url")) && prev == Some("(") {
                    Target::RouteName(value)
                } else {
                    return None;
                }
            }
            TokKind::Name if r.kind == RegionKind::Tag && i == 0 => Target::Tag(s.to_string()),
            TokKind::Name if i == 1 && matches!(tag, Some("block" | "endblock")) => {
                Target::BlockName(s.to_string())
            }
            TokKind::Name if matches!(prev, Some("." | "?.")) => {
                Target::Attribute { region: ri, end: i - 1, name: s.to_string() }
            }
            TokKind::Name if prev == Some("|") => Target::Filter(s.to_string()),
            TokKind::Name if next == Some("(") => Target::Function(s.to_string()),
            TokKind::Name if !KEYWORDS.contains(&s) => Target::Variable(s.to_string()),
            _ => return None,
        };
        Some((target, tk.range))
    }

    /// The function whose argument list contains token `i`.
    fn enclosing_call(&self, toks: &[Tok], i: usize) -> Option<String> {
        let mut depth = 0;
        for j in (0..i).rev() {
            match self.text(&toks[j]) {
                ")" => depth += 1,
                "(" => {
                    if depth == 0 {
                        return j
                            .checked_sub(1)
                            .filter(|p| toks[*p].kind == TokKind::Name)
                            .map(|p| self.text(&toks[p]).to_string());
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        None
    }

    fn root(&self) -> Option<PathBuf> {
        project_root(self.key)
    }

    /// `{% extends 'parent' %}` of this template.
    fn parent_key(&self) -> Option<String> {
        let r = self.t.regions.iter().find(|r| tag_name(self.t.text, r) == Some("extends"))?;
        let name = r.toks.iter().find(|t| t.kind == TokKind::Str).map(|t| unquote(self.text(t)))?;
        resolve_template(&self.root()?, &name).map(|p| crate::plugins::key_for(&p))
    }
}

/// `(key, line)` of `{% block name %}` in the template `key` or its parents.
fn block_definition(ws: &Workspace, key: &str, name: &str, depth: u8) -> Option<(String, u32)> {
    if depth > 8 {
        return None;
    }
    let text: String = match ws.file_id(key).and_then(|id| ws.file(id)) {
        Some(f) => f.text.to_string(),
        None => std::fs::read_to_string(key.strip_prefix("file://")?).ok()?,
    };
    let t = Template::new(&text);
    for r in &t.regions {
        if tag_name(&text, r) == Some("block") && r.toks.get(1).is_some_and(|tk| t.tok_text(tk) == name) {
            let line = text[..usize::from(r.range.start())].matches('\n').count() as u32;
            return Some((key.to_string(), line));
        }
    }
    let r = t.regions.iter().find(|r| tag_name(&text, r) == Some("extends"))?;
    let parent = r.toks.iter().find(|tk| tk.kind == TokKind::Str).map(|tk| unquote(t.tok_text(tk)))?;
    let p = resolve_template(&project_root(key)?, &parent)?;
    block_definition(ws, &crate::plugins::key_for(&p), name, depth + 1)
}

fn route_candidates(cx: &Cx) -> Vec<crate::plugins::StringCandidate> {
    // the Symfony plugin knows the routes; ask it like for `generateUrl('...')`
    let arg = crate::plugins::StringArg {
        callee: crate::plugins::Callee::Method {
            class: Fqn::new("Symfony\\Component\\Routing\\Generator\\UrlGeneratorInterface"),
            name: "generate".into(),
            is_static: false,
        },
        index: 0,
        value: String::new(),
    };
    let pcx = crate::plugins::PluginCx { ws: cx.ws, file: cx.file };
    cx.ws.plugins().iter().flat_map(|p| p.string_candidates(&pcx, &arg)).collect()
}

/// Definition targets as `(key, line)`: templates, parent blocks, routes.
pub(crate) fn string_targets(ws: &Workspace, file: FileId, offset: TextSize) -> Vec<(String, u32)> {
    let Some(cx) = Cx::new(ws, file) else { return Vec::new() };
    let Some((target, _)) = cx.target(offset) else { return Vec::new() };
    match target {
        Target::TemplateName(name, _) => cx
            .root()
            .and_then(|root| resolve_template(&root, &name))
            .map(|p| vec![(crate::plugins::key_for(&p), 0)])
            .unwrap_or_default(),
        Target::BlockName(name) => {
            cx.parent_key().and_then(|k| block_definition(ws, &k, &name, 0)).into_iter().collect()
        }
        Target::RouteName(name) => {
            route_candidates(&cx).into_iter().filter(|c| c.value == name).filter_map(|c| c.location).collect()
        }
        _ => Vec::new(),
    }
}

pub(crate) fn goto_definition(ws: &Workspace, file: FileId, offset: TextSize) -> Vec<Location> {
    let Some(cx) = Cx::new(ws, file) else { return Vec::new() };
    let Some((target, _)) = cx.target(offset) else { return Vec::new() };
    match target {
        Target::Attribute { region, end, name } => {
            let toks = &cx.t.regions[region].toks;
            let start = chain_start(&cx, toks, end);
            let Some(ty) = cx.eval(&toks[start..end], offset, 0) else { return Vec::new() };
            ty.class_names()
                .iter()
                .filter_map(|c| cx.member(c, &name))
                .map(|(f, m)| match m {
                    Member::Property(p) => Location { file: f, range: p.range, focus: p.name_range },
                    Member::Method(m, _) => Location { file: f, range: m.range, focus: m.name_range },
                })
                .collect()
        }
        Target::Variable(name) => {
            if let Some((_, r)) = cx.set_before(&name, offset) {
                return vec![Location { file, range: r.range, focus: r.toks[1].range }];
            }
            cx.context_vars()
                .iter()
                .filter(|(n, _, _)| *n == name)
                .filter_map(|(_, _, loc)| *loc)
                .map(|(f, r)| Location { file: f, range: r, focus: r })
                .collect()
        }
        _ => Vec::new(),
    }
}

pub(crate) fn hover(ws: &Workspace, file: FileId, offset: TextSize) -> Option<Hover> {
    let cx = Cx::new(ws, file)?;
    let (target, range) = cx.target(offset)?;
    let markdown = match target {
        Target::Variable(name) => {
            let ty = cx.variable_type(&name, offset, 0)?;
            format!("```php\n{ty} ${name}\n```")
        }
        Target::Attribute { region, end, name } => {
            let toks = &cx.t.regions[region].toks;
            let start = chain_start(&cx, toks, end);
            let ty = cx.eval(&toks[start..end], offset, 0)?;
            let class = ty.class_names().into_iter().find(|c| cx.member(c, &name).is_some())?;
            let (_, member) = cx.member(&class, &name)?;
            let (code, doc) = match member {
                Member::Property(p) => (crate::render::property_signature(p), p.doc.clone()),
                Member::Method(m, _) => (crate::render::function_signature(m, true), m.doc.clone()),
            };
            let mut md = crate::render::code_block(None, &format!("{class}\n{code}"));
            if let Some(d) = doc {
                let docs = crate::render::doc_markdown(&d, false);
                if !docs.is_empty() {
                    md.push_str("\n\n");
                    md.push_str(&docs);
                }
            }
            md
        }
        Target::Filter(name) => {
            let (_, doc) = FILTERS.iter().find(|(n, _)| *n == name)?;
            format!("Twig filter `{name}`\n\n{doc}")
        }
        Target::Function(name) => {
            let (_, doc) = FUNCTIONS.iter().find(|(n, _)| *n == name)?;
            format!("Twig function `{name}()`\n\n{doc}")
        }
        Target::Tag(name) if TAGS.contains(&name.as_str()) || name.starts_with("end") => {
            format!("Twig tag `{name}`")
        }
        Target::TemplateName(name, _) => {
            let root = cx.root()?;
            let path = resolve_template(&root, &name)?;
            format!("Template `{}`", path.strip_prefix(&root).unwrap_or(&path).display())
        }
        _ => return None,
    };
    Some(Hover { markdown, range })
}

pub(crate) fn document_links(ws: &Workspace, file: FileId) -> Vec<DocumentLink> {
    let Some(cx) = Cx::new(ws, file) else { return Vec::new() };
    let Some(root) = cx.root() else { return Vec::new() };
    let mut out = Vec::new();
    for r in &cx.t.regions {
        for tk in r.toks.iter().filter(|t| t.kind == TokKind::Str) {
            let Some((Target::TemplateName(name, range), _)) =
                cx.target(tk.range.start() + TextSize::from(1))
            else {
                continue;
            };
            if let Some(p) = resolve_template(&root, &name) {
                // the link covers the name without quotes
                let inner =
                    TextRange::new(range.start() + TextSize::from(1), range.end() - TextSize::from(1));
                out.push(DocumentLink { range: inner, target: crate::plugins::key_for(&p) });
            }
        }
    }
    out
}

pub(crate) fn completions(ws: &Workspace, file: FileId, offset: TextSize) -> Option<CompletionList> {
    let cx = Cx::new(ws, file)?;
    let ri = cx.t.region_at(offset)?;
    let r = &cx.t.regions[ri];
    if r.kind == RegionKind::Comment {
        return Some(CompletionList::default());
    }
    // tokens before the cursor; the identifier being typed
    let before: Vec<Tok> = r.toks.iter().copied().filter(|t| t.range.start() < offset).collect();
    let current =
        before.last().filter(|t| t.range.end() >= offset && matches!(t.kind, TokKind::Name | TokKind::Str));
    let (replace, prev_count) = match current {
        Some(t) if t.kind == TokKind::Name => (TextRange::new(t.range.start(), offset), before.len() - 1),
        Some(t) => {
            // in a string: replace its content
            let start = t.range.start() + TextSize::from(1);
            let end =
                if cx.text(t).len() > 1 && cx.t.text[..usize::from(t.range.end())].ends_with(['\'', '"']) {
                    t.range.end() - TextSize::from(1)
                } else {
                    t.range.end()
                };
            let mut items = Vec::new();
            let value_range = TextRange::new(start, end.max(start));
            match cx.target(offset) {
                Some((Target::TemplateName(..), _)) => {
                    if let Some(root) = cx.root() {
                        for n in template_names(&root) {
                            items.push(CompletionItem::new(n, CompletionKind::File, value_range));
                        }
                    }
                }
                Some((Target::RouteName(_), _)) => {
                    for c in route_candidates(&cx) {
                        let mut item = CompletionItem::new(c.value, CompletionKind::Constant, value_range);
                        item.detail = c.detail;
                        items.push(item);
                    }
                }
                _ => {}
            }
            return Some(CompletionList { items, incomplete: false });
        }
        None => (TextRange::empty(offset), before.len()),
    };
    let prev = prev_count.checked_sub(1).map(|p| cx.text(&before[p]));
    let mut items = Vec::new();
    let item = |label: &str, kind: CompletionKind, detail: Option<&str>| {
        let mut i = CompletionItem::new(label, kind, replace);
        i.detail = detail.map(str::to_string);
        i
    };
    if r.kind == RegionKind::Tag && prev_count == 0 {
        for t in TAGS {
            items.push(item(t, CompletionKind::Keyword, None));
        }
        // the end tag of the innermost open pair
        if let Some(open) = innermost_open(&cx.t, offset) {
            items.insert(0, item(&format!("end{open}"), CompletionKind::Keyword, None));
        }
    } else if prev == Some("|") {
        for (f, d) in FILTERS {
            items.push(item(f, CompletionKind::Function, Some(d)));
        }
    } else if matches!(prev, Some("." | "?.")) {
        let start = chain_start(&cx, &before, prev_count - 1);
        if let Some(ty) = cx.eval(&before[start..prev_count - 1], offset, 0) {
            items.extend(attribute_items(&cx, &ty, replace));
        }
    } else if matches!(prev, Some("is"))
        || prev == Some("not") && prev_count >= 2 && cx.text(&before[prev_count - 2]) == "is"
    {
        for t in TESTS {
            items.push(item(t, CompletionKind::Function, None));
        }
    } else {
        let mut names: Vec<(String, Option<String>)> = Vec::new();
        for (n, t, _) in cx.context_vars().iter() {
            names.push((n.clone(), Some(t.to_string())));
        }
        for reg in cx.t.regions.iter().filter(|x| x.range.end() <= offset) {
            if tag_name(cx.t.text, reg) == Some("set") {
                if let Some(n) = reg.toks.get(1) {
                    names.push((cx.text(n).to_string(), None));
                }
            }
        }
        for (o, c) in pairs(&cx.t) {
            let (open, close) = (&cx.t.regions[o], &cx.t.regions[c]);
            if open.range.end() <= offset
                && offset <= close.range.start()
                && tag_name(cx.t.text, open) == Some("for")
            {
                names.push(("loop".into(), None));
                for t in open.toks[1..]
                    .iter()
                    .take_while(|t| cx.text(t) != "in")
                    .filter(|t| t.kind == TokKind::Name)
                {
                    names.push((cx.text(t).to_string(), None));
                }
            }
        }
        names.push(("app".into(), None));
        let mut seen = std::collections::HashSet::new();
        for (n, detail) in names {
            if seen.insert(n.clone()) {
                items.push(item(&n, CompletionKind::Variable, detail.as_deref()));
            }
        }
        for (f, d) in FUNCTIONS {
            let mut i = item(f, CompletionKind::Function, Some(d));
            i.insert_text = format!("{f}($1)");
            i.is_snippet = true;
            items.push(i);
        }
    }
    Some(CompletionList { items, incomplete: false })
}

fn innermost_open(t: &Template, offset: TextSize) -> Option<String> {
    let mut stack: Vec<String> = Vec::new();
    let closed: std::collections::HashSet<usize> = pairs(t).into_iter().flat_map(|(o, c)| [o, c]).collect();
    for (i, r) in t.regions.iter().enumerate() {
        if r.range.start() >= offset {
            break;
        }
        let Some(name) = tag_name(t.text, r) else { continue };
        if PAIRED.contains(&name) && !closed.contains(&i) {
            let inline = (name == "set" && r.toks.iter().any(|tk| t.tok_text(tk) == "="))
                || (name == "block" && r.toks.len() > 2);
            if !inline {
                stack.push(name.to_string());
            }
        }
    }
    stack.pop()
}

/// `obj.` completion: public properties, methods (getters also by their
/// short name) and shape keys.
fn attribute_items(cx: &Cx, ty: &Type, replace: TextRange) -> Vec<CompletionItem> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Type::Shape(entries) = ty {
        for e in entries {
            if seen.insert(e.key.clone()) {
                let mut i = CompletionItem::new(e.key.clone(), CompletionKind::Property, replace);
                i.detail = Some(e.ty.to_string());
                out.push(i);
            }
        }
    }
    let index = cx.ws.index();
    for class in ty.class_names() {
        let members = index.members(&class);
        for p in members.properties.iter().map(|f| f.def) {
            if p.visibility == php_hir::symbols::Visibility::Public
                && !p.is_static
                && seen.insert(p.name.clone())
            {
                let mut i = CompletionItem::new(p.name.clone(), CompletionKind::Property, replace);
                i.detail = p.effective_type().map(|t| t.to_string());
                out.push(i);
            }
        }
        for m in members.methods.iter().map(|f| f.def) {
            if m.visibility != php_hir::symbols::Visibility::Public || m.name.starts_with("__") {
                continue;
            }
            let short = ["get", "is", "has"].iter().find_map(|p| {
                m.name
                    .strip_prefix(p)
                    .filter(|rest| {
                        rest.starts_with(|c: char| c.is_ascii_uppercase())
                            && m.params.iter().all(|p| p.default.is_some())
                    })
                    .map(|rest| {
                        let mut c = rest.chars();
                        c.next()
                            .map(|f| f.to_lowercase().collect::<String>() + c.as_str())
                            .unwrap_or_default()
                    })
            });
            let label = short.unwrap_or_else(|| m.name.clone());
            if seen.insert(label.clone()) {
                let mut i = CompletionItem::new(label, CompletionKind::Method, replace);
                i.detail = Some(crate::render::function_signature(m, true));
                out.push(i);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_regions_and_tokens() {
        let text = "<h1>{{ user.name|upper }}</h1>{% if x %}{# c #}{% endif %}{% verbatim %}{{ no }}{% endverbatim %}";
        let t = Template::new(text);
        let kinds: Vec<RegionKind> = t.regions.iter().map(|r| r.kind).collect();
        use RegionKind::*;
        assert_eq!(kinds, vec![Output, Tag, Comment, Tag, Tag, Tag]);
        let toks: Vec<&str> = t.regions[0].toks.iter().map(|tk| t.tok_text(tk)).collect();
        assert_eq!(toks, vec!["user", ".", "name", "|", "upper"]);
        assert_eq!(pairs(&t), vec![(1, 3), (4, 5)]);
    }

    #[test]
    fn whitespace_control_and_strings() {
        let t = Template::new("{%- set a = '%}' ~%}{{- a -}}");
        assert_eq!(t.regions.len(), 2);
        let toks: Vec<&str> = t.regions[0].toks.iter().map(|tk| t.tok_text(tk)).collect();
        assert_eq!(toks, vec!["set", "a", "=", "'%}'"]);
    }

    #[test]
    fn unterminated_regions_do_not_panic() {
        for text in ["{{", "{% if", "{# x", "{{ 'abc", "{{ a.", "{%- ", "é{{é}}", "{% verbatim %}"] {
            let t = Template::new(text);
            let _ = pairs(&t);
            let _ = folds(text);
            let _ = symbols(text);
            let _ = tokens(text, None);
        }
    }
}
