//! Laravel Blade templates.
//!
//! A template is turned into "virtual" PHP code with the same byte offsets
//! and line breaks: HTML becomes whitespace, `{{ e }}` becomes `;(e);`,
//! directives become the corresponding PHP statements and calls. Every
//! feature then works on Blade files without any offset mapping.

use php_syntax::{TextRange, TextSize};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Blade {
    /// The virtual PHP code (parsed in PHP mode, see
    /// [`php_syntax::parse_php_code`]).
    pub text: String,
    /// Ranges holding PHP code (echoes, directives with their arguments,
    /// `@php` blocks).
    pub regions: Vec<TextRange>,
    /// Directive names (without `@`) and their ranges.
    pub directives: Vec<(TextRange, String)>,
    /// `{{` / `}}` / `{!!` / `!!}` of echoes.
    pub delimiters: Vec<TextRange>,
}

/// Types of variables Blade defines itself (loaded with the stubs).
pub(crate) const STUB: &str = r#"<?php
namespace PhpLsp\Blade;

/** The `$loop` variable inside `@foreach` / `@forelse`. */
final class Loop
{
    /** Index of the current iteration (starts at 0). */
    public int $index;
    /** The current iteration (starts at 1). */
    public int $iteration;
    /** Iterations remaining. */
    public int $remaining;
    /** Number of items in the array being iterated. */
    public int $count;
    /** Whether this is the first iteration. */
    public bool $first;
    /** Whether this is the last iteration. */
    public bool $last;
    /** Whether this is an even iteration. */
    public bool $even;
    /** Whether this is an odd iteration. */
    public bool $odd;
    /** Nesting level of the current loop. */
    public int $depth;
    /** The parent loop's variable when in a nested loop. */
    public ?Loop $parent;
}
"#;

/// Class of `$loop` (see [`STUB`]).
pub(crate) const LOOP_CLASS: &str = "PhpLsp\\Blade\\Loop";

pub(crate) fn is_blade(path: &str) -> bool {
    path.ends_with(".blade.php")
}

/// Directives turned into PHP control structures: name -> PHP keyword.
const CONTROL: &[(&str, &str)] = &[
    ("if", "if"),
    ("elseif", "if"),
    ("unless", "if"),
    ("foreach", "foreach"),
    ("forelse", "foreach"),
    ("for", "for"),
    ("while", "while"),
    ("isset", "isset"),
    ("empty", "empty"),
    ("break", "if"),
    ("continue", "if"),
];

/// Directives taking arguments that are compiled to PHP code.
pub(crate) const WITH_ARGS: &[&str] = &[
    "include",
    "includeIf",
    "includeWhen",
    "includeUnless",
    "includeFirst",
    "each",
    "extends",
    "extendsFirst",
    "section",
    "yield",
    "push",
    "pushOnce",
    "pushIf",
    "prepend",
    "prependOnce",
    "stack",
    "hasSection",
    "sectionMissing",
    "hasstack",
    "component",
    "slot",
    "props",
    "aware",
    "json",
    "js",
    "class",
    "style",
    "checked",
    "selected",
    "disabled",
    "readonly",
    "required",
    "method",
    "error",
    "can",
    "cannot",
    "canany",
    "auth",
    "guest",
    "env",
    "production",
    "lang",
    "choice",
    "inject",
    "dd",
    "dump",
    "vite",
    "fragment",
    "session",
    "context",
    "use",
    "switch",
    "case",
    "livewire",
    "entangle",
    "teleport",
    "persist",
    "once",
    "unset",
];

/// Directives without arguments.
pub(crate) const WITHOUT_ARGS: &[&str] = &[
    "else",
    "endif",
    "endunless",
    "endisset",
    "endempty",
    "endforeach",
    "endforelse",
    "endfor",
    "endwhile",
    "endswitch",
    "default",
    "break",
    "continue",
    "csrf",
    "endsection",
    "show",
    "stop",
    "append",
    "overwrite",
    "parent",
    "endpush",
    "endpushOnce",
    "endPushIf",
    "endprepend",
    "endprependOnce",
    "endonce",
    "once",
    "endcomponent",
    "endslot",
    "auth",
    "endauth",
    "guest",
    "endguest",
    "endenv",
    "production",
    "endproduction",
    "endcan",
    "endcannot",
    "endcanany",
    "enderror",
    "endfragment",
    "endsession",
    "endcontext",
    "empty",
    "livewireStyles",
    "livewireScripts",
    "livewireScriptConfig",
    "viteReactRefresh",
    "endteleport",
    "endpersist",
    "endverbatim",
    "endphp",
];

/// Directives taking a view name as first argument.
pub(crate) const VIEW_DIRECTIVES: &[&str] = &[
    "include",
    "includeIf",
    "includeWhen",
    "includeUnless",
    "includeFirst",
    "each",
    "extends",
    "extendsFirst",
    "component",
];

/// PHP keywords that cannot be used as function names.
const KEYWORDS: &[&str] = &[
    "include",
    "use",
    "class",
    "switch",
    "case",
    "unset",
    "empty",
    "isset",
    "list",
    "array",
    "print",
    "echo",
    "exit",
    "die",
    "eval",
    "require",
    "default",
    "break",
    "continue",
    "extends",
    "yield",
    "readonly",
    "static",
    "global",
    "const",
    "return",
    "throw",
    "new",
    "function",
    "fn",
    "match",
    "goto",
    "namespace",
    "trait",
    "interface",
];

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn find(src: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from > src.len() {
        return None;
    }
    src[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// Like [`find`], skipping quoted strings.
fn find_code(src: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    let mut i = from;
    let mut quote: Option<u8> = None;
    while i < src.len() {
        let b = src[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            None => {
                if src[i..].starts_with(needle) {
                    return Some(i);
                }
                if b == b'\'' || b == b'"' {
                    quote = Some(b);
                }
            }
        }
        i += 1;
    }
    None
}

/// End (exclusive) of the balanced parenthesis starting at `open`.
fn matching_paren(src: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    let mut quote: Option<u8> = None;
    while i < src.len() {
        let b = src[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'\'' | b'"' => quote = Some(b),
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                b'\n' if depth == 0 => return None,
                _ => {}
            },
        }
        i += 1;
    }
    None
}

fn line_end(src: &[u8], from: usize) -> usize {
    src[from..].iter().position(|&b| b == b'\n').map_or(src.len(), |p| p + from)
}

struct Builder<'a> {
    src: &'a [u8],
    out: Vec<u8>,
    regions: Vec<TextRange>,
    directives: Vec<(TextRange, String)>,
    delimiters: Vec<TextRange>,
}

fn range(from: usize, to: usize) -> TextRange {
    TextRange::new(TextSize::from(from as u32), TextSize::from(to as u32))
}

impl Builder<'_> {
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.out[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn copy(&mut self, from: usize, to: usize) {
        self.out[from..to].copy_from_slice(&self.src[from..to]);
    }
    fn region(&mut self, from: usize, to: usize) {
        self.regions.push(TextRange::new(TextSize::from(from as u32), TextSize::from(to as u32)));
    }
}

pub(crate) fn virtual_php(text: &str) -> Blade {
    let src = text.as_bytes();
    let out = src.iter().map(|&b| if b == b'\n' || b == b'\r' { b } else { b' ' }).collect();
    let mut b = Builder { src, out, regions: Vec::new(), directives: Vec::new(), delimiters: Vec::new() };
    let mut i = 0;
    let mut in_component = false;
    while i < src.len() {
        let rest = &src[i..];
        if rest.starts_with(b"{{--") {
            // comment -> doc comment (so that `@var` works)
            let end = find(src, i + 4, b"--}}").unwrap_or(src.len());
            b.put(i, b"/** ");
            b.copy(i + 4, end);
            let mut j = i + 4;
            while let Some(k) = find(src, j, b"*/").filter(|&k| k < end) {
                b.put(k, b"* ");
                j = k + 2;
            }
            if end < src.len() {
                b.put(end, b"  */");
            }
            b.region(i, (end + 4).min(src.len()));
            i = end + 4;
        } else if rest.starts_with(b"@{{") {
            // escaped echo: printed literally
            i = find(src, i + 3, b"}}").map_or(src.len(), |e| e + 2);
        } else if rest.starts_with(b"{{") || rest.starts_with(b"{!!") {
            let raw = rest.starts_with(b"{!!");
            let (open, close): (usize, &[u8]) = if raw { (3, b"!!}") } else { (2, b"}}") };
            match find_code(src, i + open, close) {
                Some(end) => {
                    b.put(i, b";(");
                    b.copy(i + open, end);
                    b.put(end, b");");
                    b.delimiters.push(range(i, i + open));
                    b.delimiters.push(range(end, end + close.len()));
                    b.region(i, end + close.len());
                    i = end + close.len();
                }
                None => {
                    // still being typed
                    let end = line_end(src, i);
                    b.put(i, b";");
                    b.copy(i + open, end);
                    b.region(i, end);
                    i = end;
                }
            }
        } else if rest.starts_with(b"<?php") || rest.starts_with(b"<?=") {
            let echo = rest.starts_with(b"<?=");
            let open = if echo { 3 } else { 5 };
            let end = find_code(src, i + open, b"?>").unwrap_or(src.len());
            b.put(i, if echo { b";(" } else { b";" });
            b.copy(i + open, end);
            if end < src.len() {
                b.put(end, if echo { b");" } else { b"; " });
            }
            b.region(i, (end + 2).min(src.len()));
            i = end + 2;
        } else if rest[0] == b'@' && (i == 0 || !is_word(src[i - 1]) && src[i - 1] != b'@') {
            i = directive(&mut b, i);
        } else if rest.starts_with(b"<x-") || rest.starts_with(b"<x:") {
            in_component = true;
            i += 3;
        } else if in_component && rest[0] == b'>' {
            in_component = false;
            i += 1;
        } else if in_component
            && rest[0] == b':'
            && i > 0
            && src[i - 1].is_ascii_whitespace()
            && src.get(i + 1) != Some(&b':')
        {
            // `<x-alert :type="$expr">`
            let name_end =
                i + 1 + src[i + 1..].iter().take_while(|&&c| is_word(c) || c == b'-' || c == b'.').count();
            let quote = src.get(name_end + 1).copied();
            if src.get(name_end) == Some(&b'=') && matches!(quote, Some(b'"') | Some(b'\'')) {
                if let Some(close) = find(src, name_end + 2, &[quote.unwrap()]) {
                    b.put(i, b";");
                    b.put(name_end + 1, b"(");
                    b.copy(name_end + 2, close);
                    b.put(close, b")");
                    b.region(i, close + 1);
                    i = close + 1;
                    continue;
                }
            }
            i = name_end;
        } else {
            i += 1;
        }
    }
    let mut text =
        String::from_utf8(b.out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    text.push(';');
    Blade { text, regions: b.regions, directives: b.directives, delimiters: b.delimiters }
}

/// Handles the directive at `i` (`src[i] == '@'`); returns the next offset.
fn directive(b: &mut Builder, i: usize) -> usize {
    let src = b.src;
    if src.get(i + 1) == Some(&b'@') {
        // `@@if`: escaped
        return i + 2 + src[i + 2..].iter().take_while(|&&c| is_word(c)).count();
    }
    let name_len = src[i + 1..].iter().take_while(|&&c| is_word(c)).count();
    if name_len == 0 || src[i + 1].is_ascii_digit() {
        return i + 1;
    }
    let name_end = i + 1 + name_len;
    let name = std::str::from_utf8(&src[i + 1..name_end]).unwrap_or("").to_string();
    let ws = src[name_end..].iter().take_while(|&&c| c == b' ' || c == b'\t').count();
    let args = (src.get(name_end + ws) == Some(&b'(')).then(|| matching_paren(src, name_end + ws)).flatten();
    let name_range = TextRange::new(TextSize::from(i as u32 + 1), TextSize::from(name_end as u32));

    if name == "verbatim" {
        return find(src, name_end, b"@endverbatim").map_or(src.len(), |e| e + "@endverbatim".len());
    }
    if name == "php" {
        b.directives.push((name_range, name));
        if let Some(end) = args {
            // inline `@php($x = 1)`
            b.put(i, b";");
            b.copy(name_end, end);
            b.region(name_end, end);
            return end;
        }
        let end = find(src, name_end, b"@endphp").unwrap_or(src.len());
        b.put(i, b";");
        b.copy(name_end, end);
        if end < src.len() {
            b.put(end, b";");
            b.directives.push((range(end + 1, end + "@endphp".len()), "endphp".to_string()));
        }
        b.region(name_end, end);
        return end + "@endphp".len();
    }
    if let Some(end) = args.filter(|_| name == "use") {
        if use_import(b, i, name_end, end) {
            b.directives.push((name_range, name));
            b.region(name_end, end);
            return end;
        }
    }
    if let (Some(end), Some((_, kw))) = (args, CONTROL.iter().find(|(n, _)| *n == name)) {
        b.directives.push((name_range, name.clone()));
        b.put(i, b";");
        b.put(i + 1, kw.as_bytes());
        b.copy(name_end, end);
        b.region(name_end, end);
        return end;
    }
    if let Some(end) = args.filter(|_| WITH_ARGS.contains(&name.as_str())) {
        b.directives.push((name_range, name.clone()));
        b.put(i, b";");
        if KEYWORDS.contains(&name.as_str()) || VIEW_DIRECTIVES.contains(&name.as_str()) {
            b.put(i + 1, "_".repeat(name_len).as_bytes());
        } else {
            b.copy(i + 1, name_end);
        }
        b.copy(name_end, end);
        b.region(name_end, end);
        return end;
    }
    if WITHOUT_ARGS.contains(&name.as_str()) {
        b.directives.push((name_range, name));
        b.put(i, b";");
        return name_end;
    }
    // unknown (custom directives, CSS at-rules, Alpine `@click`): text
    name_end
}

/// `@use('A\\B')` / `@use('A\\B', 'C')` -> `;use A\\B;` / `;use A\\B as C;`.
fn use_import(b: &mut Builder, at: usize, name_end: usize, end: usize) -> bool {
    let src = b.src;
    let inner = &src[name_end..end];
    let quotes: Vec<usize> = inner
        .iter()
        .enumerate()
        .filter(|(_, &c)| c == b'\'' || c == b'"')
        .map(|(k, _)| name_end + k)
        .collect();
    let (q0, q1) = match quotes.as_slice() {
        [a, b, ..] => (*a, *b),
        _ => return false,
    };
    b.put(at, b";");
    b.copy(at + 1, name_end);
    b.copy(q0 + 1, q1);
    if let [_, _, q2, q3] = quotes.as_slice() {
        if q2 - q1 >= 3 {
            b.put(q1 + 1, b"as");
            b.copy(q2 + 1, *q3);
        }
    }
    b.put(end - 1, b";");
    true
}

/// Directive snippets: (name, snippet after `@`).
const SNIPPETS: &[(&str, &str)] = &[
    ("if", "if(${1})\n\t$0\n@endif"),
    ("elseif", "elseif(${1})"),
    ("unless", "unless(${1})\n\t$0\n@endunless"),
    ("isset", "isset(${1})\n\t$0\n@endisset"),
    ("empty", "empty(${1})\n\t$0\n@endempty"),
    ("foreach", "foreach(${1:\\$items} as ${2:\\$item})\n\t$0\n@endforeach"),
    ("forelse", "forelse(${1:\\$items} as ${2:\\$item})\n\t$0\n@empty\n\t\n@endforelse"),
    ("for", "for(${1:\\$i = 0}; ${2:\\$i < 10}; ${3:\\$i++})\n\t$0\n@endfor"),
    ("while", "while(${1})\n\t$0\n@endwhile"),
    ("switch", "switch(${1})\n\t@case(${2})\n\t\t$0\n\t\t@break\n\t@default\n@endswitch"),
    ("case", "case(${1})"),
    ("auth", "auth\n\t$0\n@endauth"),
    ("guest", "guest\n\t$0\n@endguest"),
    ("can", "can('${1}')\n\t$0\n@endcan"),
    ("env", "env('${1:local}')\n\t$0\n@endenv"),
    ("production", "production\n\t$0\n@endproduction"),
    ("section", "section('${1}')\n\t$0\n@endsection"),
    ("push", "push('${1}')\n\t$0\n@endpush"),
    ("once", "once\n\t$0\n@endonce"),
    ("error", "error('${1}')\n\t$0\n@enderror"),
    ("php", "php\n\t$0\n@endphp"),
    ("verbatim", "verbatim\n\t$0\n@endverbatim"),
    ("component", "component('${1}')\n\t$0\n@endcomponent"),
    ("slot", "slot('${1}')\n\t$0\n@endslot"),
    ("fragment", "fragment('${1}')\n\t$0\n@endfragment"),
    ("include", "include('${1}')"),
    ("includeIf", "includeIf('${1}')"),
    ("includeWhen", "includeWhen(${1}, '${2}')"),
    ("each", "each('${1}', ${2:\\$items}, '${3:item}')"),
    ("extends", "extends('${1}')"),
    ("yield", "yield('${1}')"),
    ("stack", "stack('${1}')"),
    ("props", "props([${1}])"),
    ("use", "use('${1}')"),
    ("json", "json(${1})"),
    ("class", "class([${1}])"),
    ("style", "style([${1}])"),
    ("checked", "checked(${1})"),
    ("selected", "selected(${1})"),
    ("disabled", "disabled(${1})"),
    ("method", "method('${1:PUT}')"),
    ("lang", "lang('${1}')"),
    ("vite", "vite(['${1:resources/js/app.js}'])"),
    ("dd", "dd(${1})"),
    ("dump", "dump(${1})"),
];

/// Completion of directive names after `@` (outside of PHP code).
pub(crate) fn directive_completions(
    text: &str,
    offset: TextSize,
) -> Option<Vec<crate::completion::CompletionItem>> {
    use crate::completion::{CompletionItem, CompletionKind};
    let before = &text[..usize::from(offset)];
    let word = before.len() - before.bytes().rev().take_while(|&c| is_word(c)).count();
    if word == 0 || before.as_bytes()[word - 1] != b'@' || word >= 2 && is_word(before.as_bytes()[word - 2]) {
        return None;
    }
    let range = TextRange::new(TextSize::from(word as u32), offset);
    let mut items = Vec::new();
    for (name, snippet) in SNIPPETS {
        let mut item = CompletionItem::new(format!("@{name}"), CompletionKind::Snippet, range);
        item.filter_text = Some(name.to_string());
        item.insert_text = snippet.to_string();
        item.is_snippet = true;
        item.detail = Some("Blade directive".into());
        items.push(item);
    }
    for name in WITHOUT_ARGS.iter().filter(|n| !SNIPPETS.iter().any(|(s, _)| s == *n)) {
        let mut item = CompletionItem::new(format!("@{name}"), CompletionKind::Keyword, range);
        item.filter_text = Some(name.to_string());
        item.insert_text = name.to_string();
        item.detail = Some("Blade directive".into());
        items.push(item);
    }
    Some(items)
}

/// Directives opening a block closed by `@end<name>`.
const BLOCKS: &[&str] = &[
    "if",
    "unless",
    "isset",
    "empty",
    "foreach",
    "forelse",
    "for",
    "while",
    "switch",
    "section",
    "push",
    "pushOnce",
    "pushIf",
    "prepend",
    "prependOnce",
    "once",
    "auth",
    "guest",
    "can",
    "cannot",
    "canany",
    "env",
    "production",
    "error",
    "component",
    "slot",
    "fragment",
    "session",
    "context",
    "php",
    "teleport",
    "persist",
];

/// A `@name ... @endname` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub name: String,
    /// The opening directive (`@` included) with its arguments.
    pub open: TextRange,
    /// The closing directive (`@` included).
    pub close: TextRange,
    /// First string argument (`@section('content')` -> `content`).
    pub label: Option<String>,
}

/// Arguments `(...)` following a directive name ending at `name_end`.
fn directive_args(src: &[u8], name_end: usize) -> Option<TextRange> {
    let ws = src[name_end..].iter().take_while(|&&c| c == b' ' || c == b'\t').count();
    (src.get(name_end + ws) == Some(&b'('))
        .then(|| matching_paren(src, name_end + ws))
        .flatten()
        .map(|e| range(name_end + ws, e))
}

/// Number of top-level commas in an argument list `(...)`.
fn top_level_commas(args: &str) -> usize {
    let (mut depth, mut quote, mut n) = (0i32, None, 0);
    for c in args.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 1 => n += 1,
                _ => {}
            },
        }
    }
    n
}

fn first_string(args: &str) -> Option<String> {
    let start = args.find(['\'', '"'])?;
    let q = args[start..].chars().next()?;
    let end = args[start + 1..].find(q)? + start + 1;
    Some(args[start + 1..end].to_string())
}

/// Pairs block directives.
pub(crate) fn blocks(text: &str, blade: &Blade) -> Vec<Block> {
    let src = text.as_bytes();
    let mut stack: Vec<(String, TextRange, Option<String>)> = Vec::new();
    let mut out = Vec::new();
    for (name_range, name) in &blade.directives {
        let at = name_range.start() - TextSize::from(1);
        let args = directive_args(src, usize::from(name_range.end()));
        let args_text = args.map(|r| &text[r]);
        let open_end = args.map_or(name_range.end(), |r| r.end());
        let closes = match name.as_str() {
            "show" | "stop" | "append" | "overwrite" => Some("section".to_string()),
            n => n.strip_prefix("end").filter(|r| !r.is_empty()).map(|r| r.to_string()),
        };
        if let Some(target) = closes {
            if let Some(pos) = stack.iter().rposition(|(n, _, _)| n.eq_ignore_ascii_case(&target)) {
                let (name, open, label) = stack.remove(pos);
                stack.truncate(pos);
                out.push(Block { name, open, close: TextRange::new(at, name_range.end()), label });
            }
            continue;
        }
        let opens = BLOCKS.contains(&name.as_str())
            // `@empty` without arguments separates `@forelse`; inline `@php(...)`
            && !(matches!(name.as_str(), "empty" | "php") && args.is_some() != (name == "empty"))
            // `@section('title', 'value')` / `@push('x', 'y')` have no body
            && !(matches!(name.as_str(), "section" | "push" | "prepend" | "slot") && args_text.is_some_and(|a| top_level_commas(a) > 0));
        if opens {
            stack.push((name.clone(), TextRange::new(at, open_end), args_text.and_then(first_string)));
        }
    }
    out.sort_by_key(|b| b.open.start());
    out
}

/// Folding ranges of a template: directive blocks and multi-line HTML elements.
pub(crate) fn folds(text: &str, blade: &Blade) -> Vec<crate::folding::Fold> {
    use crate::folding::{Fold, FoldKind};
    let mut out: Vec<Fold> = blocks(text, blade)
        .into_iter()
        .filter(|b| text[TextRange::new(b.open.start(), b.close.start())].contains('\n'))
        .map(|b| Fold { range: TextRange::new(b.open.start(), b.close.start()), kind: FoldKind::Block })
        .collect();
    for m in text.match_indices("{{--") {
        let start = m.0;
        if let Some(end) = find(text.as_bytes(), start, b"--}}") {
            if text[start..end].contains('\n') {
                out.push(Fold { range: range(start, end + 4), kind: FoldKind::Comment });
            }
        }
    }
    out.sort_by_key(|f| f.range.start());
    out
}

/// Outline of a template: sections, stacks, slots, fragments and components.
pub(crate) fn symbols(text: &str, blade: &Blade) -> Vec<crate::symbols::Symbol> {
    use crate::symbols::{Symbol, SymbolKind};
    const NAMED: &[&str] = &[
        "section",
        "push",
        "pushOnce",
        "prepend",
        "prependOnce",
        "slot",
        "fragment",
        "component",
        "yield",
        "stack",
    ];
    let blocks = blocks(text, blade);
    let mut flat: Vec<Symbol> = Vec::new();
    for (name_range, name) in &blade.directives {
        if !NAMED.contains(&name.as_str()) {
            continue;
        }
        let at = name_range.start() - TextSize::from(1);
        let args = directive_args(text.as_bytes(), usize::from(name_range.end()));
        let Some(label) = args.and_then(|r| first_string(&text[r])) else { continue };
        let full = blocks
            .iter()
            .find(|b| b.open.start() == at)
            .map_or(TextRange::new(at, args.map_or(name_range.end(), |r| r.end())), |b| {
                TextRange::new(at, b.close.end())
            });
        flat.push(Symbol {
            name: label,
            detail: Some(format!("@{name}")),
            kind: if matches!(name.as_str(), "section" | "yield") {
                SymbolKind::Namespace
            } else {
                SymbolKind::Property
            },
            deprecated: false,
            range: full,
            selection_range: TextRange::new(at, name_range.end()),
            children: Vec::new(),
        });
    }
    // nest by range
    fn insert(into: &mut Vec<Symbol>, sym: Symbol) {
        if let Some(parent) =
            into.iter_mut().rev().find(|p| p.range.contains_range(sym.range) && p.range != sym.range)
        {
            insert(&mut parent.children, sym);
        } else {
            into.push(sym);
        }
    }
    let mut out = Vec::new();
    for s in flat {
        insert(&mut out, s);
    }
    out
}

/// Semantic tokens for directive names (with `@`) and echo delimiters.
pub(crate) fn tokens(blade: &Blade) -> Vec<crate::semantic_tokens::SemanticToken> {
    use crate::semantic_tokens::{SemanticToken, SemanticTokenType};
    blade
        .directives
        .iter()
        .map(|(r, _)| TextRange::new(r.start() - TextSize::from(1), r.end()))
        .chain(blade.delimiters.iter().copied())
        .map(|range| SemanticToken { range, token_type: SemanticTokenType::Keyword, modifiers: 0 })
        .collect()
}

/// Whether `offset` is inside PHP code of the template.
pub(crate) fn in_code(blade: &Blade, offset: TextSize) -> bool {
    blade.regions.iter().any(|r| r.start() < offset && offset <= r.end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(src: &str, expect: expect_test::Expect) {
        let b = virtual_php(src);
        assert_eq!(b.text.len(), src.len() + 1);
        assert_eq!(
            b.text.match_indices('\n').map(|m| m.0).collect::<Vec<_>>(),
            src.match_indices('\n').map(|m| m.0).collect::<Vec<_>>()
        );
        // spaces made visible (expect strips leading whitespace)
        expect.assert_eq(&b.text.replace(' ', "·"));
    }

    #[test]
    fn transforms_templates() {
        check(
            "<p>{{ $user->name }}</p>\n{!! $html !!} @{{ raw }} a@b.c\n{{-- @var \\App\\User $u */ --}}",
            expect_test::expect![[r#"
                ···;(·$user->name·);····
                ;(··$html·);··················
                /**··@var·\App\User·$u·*····*/;"#]],
        );
        check(
            "@if($a)\n  @foreach ($items as $item)\n    @include('row', ['i' => $item])\n  @endforeach\n@elseif(count($b) > 1)\n@else\n@endif",
            expect_test::expect![[r#"
                ;if($a)
                ··;foreach·($items·as·$item)
                ····;_______('row',·['i'·=>·$item])
                ··;··········
                ;if····(count($b)·>·1)
                ;····
                ;·····;"#]],
        );
        check(
            "@use('Illuminate\\Support\\Str')\n@use('App\\Models\\User', 'U')\n{{ Str::of($x) }}",
            expect_test::expect![[r#"
                ;use··Illuminate\Support\Str·;
                ;use··App\Models\User·as·U·;
                ;(·Str::of($x)·);;"#]],
        );
        check(
            "@php\n  $x = 1;\n@endphp\n@php($y = 2)\n<style>@media (x) {}</style>\n<x-alert :type=\"$t\" class=\"a\" />\n@verbatim {{ no }} @endverbatim",
            expect_test::expect![[r#"
                ;···
                ··$x·=·1;
                ;······
                ;···($y·=·2)
                ····························
                ·········;·····($t)·············
                ·······························;"#]],
        );
    }

    #[test]
    fn unterminated_echo() {
        check(
            "<p>{{ $user-></p>\n<b>",
            expect_test::expect![[r#"
                ···;··$user-></p>
                ···;"#]],
        );
    }
}
