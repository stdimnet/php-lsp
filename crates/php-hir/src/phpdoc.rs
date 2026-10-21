//! PHPDoc parsing: doc blocks, tags and the PHPDoc type language
//! (including common PHPStan / Psalm extensions).

use crate::types::{builtin_type, ShapeEntry, Type};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocBlock {
    /// First paragraph.
    pub summary: String,
    /// Remaining free text before the tags.
    pub description: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Tag name without `@`, e.g. `param`, `phpstan-return`.
    pub name: String,
    /// Everything after the tag name (may span lines).
    pub body: String,
}

/// A parsed `@param`, `@var`, `@property`, ... tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedTag {
    pub type_text: String,
    pub var: Option<String>,
    pub description: String,
}

/// A parsed `@method` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodTag {
    pub is_static: bool,
    pub return_type: Option<String>,
    pub name: String,
    /// `(type $name = default, ...)` parameter texts.
    pub params: Vec<MethodTagParam>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodTagParam {
    pub type_text: Option<String>,
    pub name: String,
    pub default: Option<String>,
    pub variadic: bool,
}

/// `@template T of Bound`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateTag {
    pub name: String,
    pub bound: Option<String>,
}

impl DocBlock {
    /// Parses a `/** ... */` comment.
    pub fn parse(text: &str) -> DocBlock {
        let inner = text.trim();
        let inner = inner.strip_prefix("/**").unwrap_or(inner);
        let inner = inner.strip_suffix("*/").unwrap_or(inner);
        let mut lines: Vec<String> = Vec::new();
        for line in inner.lines() {
            let l = line.trim_start();
            let l = l.strip_prefix('*').unwrap_or(l);
            let l = l.strip_prefix(' ').unwrap_or(l);
            lines.push(l.trim_end().to_string());
        }
        let mut doc = DocBlock::default();
        let mut text_lines: Vec<&str> = Vec::new();
        let mut in_code = false;
        let mut i = 0;
        while i < lines.len() {
            let l = &lines[i];
            if l.trim_start().starts_with("```") {
                in_code = !in_code;
            }
            if !in_code && l.trim_start().starts_with('@') {
                break;
            }
            text_lines.push(l);
            i += 1;
        }
        // tags
        let mut current: Option<Tag> = None;
        while i < lines.len() {
            let l = lines[i].trim_start();
            if let Some(rest) = l.strip_prefix('@') {
                if let Some(t) = current.take() {
                    doc.tags.push(t);
                }
                let name_len =
                    rest.find(|c: char| c.is_whitespace() || c == '(' || c == '{').unwrap_or(rest.len());
                current = Some(Tag {
                    name: rest[..name_len].to_string(),
                    body: rest[name_len..].trim().to_string(),
                });
            } else if let Some(t) = current.as_mut() {
                if !t.body.is_empty() {
                    t.body.push('\n');
                }
                t.body.push_str(l);
            }
            i += 1;
        }
        if let Some(t) = current.take() {
            doc.tags.push(t);
        }
        for t in &mut doc.tags {
            t.body = t.body.trim_end().to_string();
        }
        // summary = first paragraph
        let text = text_lines.join("\n");
        let text = text.trim();
        match text.find("\n\n") {
            Some(i) => {
                doc.summary = text[..i].trim().to_string();
                doc.description = text[i..].trim().to_string();
            }
            None => doc.summary = text.to_string(),
        }
        doc
    }

    pub fn tags_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Tag> + 'a {
        self.tags.iter().filter(move |t| t.name.eq_ignore_ascii_case(name))
    }

    pub fn has_tag(&self, name: &str) -> bool {
        self.tags_named(name).next().is_some()
    }

    /// Finds a tag preferring the `phpstan-`/`psalm-` variants.
    fn preferred<'a>(&'a self, name: &str) -> Vec<&'a Tag> {
        for prefix in ["phpstan-", "psalm-", ""] {
            let full = format!("{prefix}{name}");
            let tags: Vec<_> = self.tags.iter().filter(|t| t.name.eq_ignore_ascii_case(&full)).collect();
            if !tags.is_empty() {
                return tags;
            }
        }
        Vec::new()
    }

    /// `@param Type $name description`
    pub fn param(&self, var: &str) -> Option<TypedTag> {
        let var = var.trim_start_matches('$');
        let mut best = None;
        for prefix in ["", "psalm-", "phpstan-"] {
            for t in self.tags_named(&format!("{prefix}param")) {
                let tag = parse_typed_tag(&t.body);
                if tag.var.as_deref().map(|v| v.trim_start_matches("...").trim_start_matches('&'))
                    == Some(var)
                {
                    best = Some(tag);
                }
            }
        }
        best
    }

    pub fn params(&self) -> Vec<TypedTag> {
        self.tags_named("param").map(|t| parse_typed_tag(&t.body)).collect()
    }

    /// Type text of `@return`.
    pub fn return_tag(&self) -> Option<TypedTag> {
        self.preferred("return").first().map(|t| parse_typed_tag_no_var(&t.body))
    }

    /// `@var Type $name` – `name` filters by variable when given.
    pub fn var(&self, name: Option<&str>) -> Option<TypedTag> {
        let tags = self.preferred("var");
        let parsed: Vec<TypedTag> = tags.iter().map(|t| parse_typed_tag(&t.body)).collect();
        match name {
            Some(n) => {
                let n = n.trim_start_matches('$');
                parsed
                    .iter()
                    .find(|t| t.var.as_deref() == Some(n))
                    .or_else(|| parsed.iter().find(|t| t.var.is_none()))
                    .cloned()
            }
            None => parsed.into_iter().next(),
        }
    }

    pub fn throws(&self) -> Vec<TypedTag> {
        self.tags_named("throws").map(|t| parse_typed_tag_no_var(&t.body)).collect()
    }

    /// `@deprecated` (with optional reason).
    pub fn deprecated(&self) -> Option<String> {
        self.tags_named("deprecated").next().map(|t| t.body.clone())
    }

    /// `@property`, `@property-read`, `@property-write`
    pub fn properties(&self) -> Vec<(TypedTag, PropertyAccess)> {
        let mut out = Vec::new();
        for t in &self.tags {
            let name = t.name.to_ascii_lowercase();
            let name = name.trim_start_matches("phpstan-").trim_start_matches("psalm-");
            let access = match name {
                "property" => PropertyAccess::ReadWrite,
                "property-read" => PropertyAccess::Read,
                "property-write" => PropertyAccess::Write,
                _ => continue,
            };
            let tag = parse_typed_tag(&t.body);
            if tag.var.is_some() {
                out.push((tag, access));
            }
        }
        out
    }

    /// `@method [static] [ReturnType] name(params) description`
    pub fn methods(&self) -> Vec<MethodTag> {
        self.tags
            .iter()
            .filter(|t| {
                matches!(t.name.to_ascii_lowercase().as_str(), "method" | "phpstan-method" | "psalm-method")
            })
            .filter_map(|t| parse_method_tag(&t.body))
            .collect()
    }

    pub fn templates(&self) -> Vec<TemplateTag> {
        let mut out = Vec::new();
        for t in &self.tags {
            let name = t.name.to_ascii_lowercase();
            let name = name.trim_start_matches("phpstan-").trim_start_matches("psalm-");
            if !matches!(name, "template" | "template-covariant" | "template-contravariant") {
                continue;
            }
            let mut parts = t.body.split_whitespace();
            let Some(n) = parts.next() else { continue };
            let bound = match parts.next() {
                Some("of" | "as") => parts.next().map(str::to_string),
                _ => None,
            };
            if !out.iter().any(|o: &TemplateTag| o.name == n) {
                out.push(TemplateTag { name: n.to_string(), bound });
            }
        }
        out
    }

    /// `@extends Base<T>` / `@implements I<T>` / `@use Trait<T>`
    pub fn generic_supertypes(&self, kind: &str) -> Vec<String> {
        let mut out = Vec::new();
        for prefix in ["", "phpstan-", "psalm-", "template-"] {
            for t in self.tags_named(&format!("{prefix}{kind}")) {
                let (ty, _) = split_type(&t.body);
                if !ty.is_empty() {
                    out.push(ty.to_string());
                }
            }
        }
        out
    }

    /// `@mixin Class`
    pub fn mixins(&self) -> Vec<String> {
        self.tags_named("mixin")
            .map(|t| split_type(&t.body).0.to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyAccess {
    ReadWrite,
    Read,
    Write,
}

/// Splits a tag body into the leading type expression and the rest.
pub fn split_type(body: &str) -> (&str, &str) {
    let body = body.trim_start();
    let bytes = body.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' => quote = Some(b),
            b'<' | b'(' | b'[' | b'{' => depth += 1,
            b'>' | b')' | b']' | b'}' => depth -= 1,
            b'-' if bytes.get(i + 1) == Some(&b'>') => i += 1,
            _ if b.is_ascii_whitespace() && depth <= 0 => {
                // allow `int | string` and `A, B` inside generics only
                let rest = body[i..].trim_start();
                let prev = body[..i].trim_end();
                if rest.starts_with('|')
                    || rest.starts_with('&')
                    || prev.ends_with('|')
                    || prev.ends_with('&')
                    || prev.ends_with(':')
                {
                    i += 1;
                    continue;
                }
                return (&body[..i], body[i..].trim_start());
            }
            _ => {}
        }
        i += 1;
    }
    (body, "")
}

fn parse_typed_tag(body: &str) -> TypedTag {
    let body = body.trim();
    // `@param $name` without type
    if body.starts_with('$') || body.starts_with("...$") || body.starts_with("&$") {
        let (var, rest) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
        return TypedTag {
            type_text: String::new(),
            var: Some(clean_var(var)),
            description: rest.trim().to_string(),
        };
    }
    let (ty, rest) = split_type(body);
    let rest = rest.trim_start();
    let (var, desc) = if rest.starts_with('$')
        || rest.starts_with("...$")
        || rest.starts_with("&$")
        || rest.starts_with("&...$")
    {
        let (v, d) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        (Some(clean_var(v)), d.trim())
    } else {
        (None, rest)
    };
    TypedTag { type_text: ty.to_string(), var, description: desc.to_string() }
}

fn clean_var(v: &str) -> String {
    v.trim_start_matches('&').trim_start_matches("...").trim_start_matches('$').to_string()
}

fn parse_typed_tag_no_var(body: &str) -> TypedTag {
    let (ty, rest) = split_type(body.trim());
    TypedTag { type_text: ty.to_string(), var: None, description: rest.trim().to_string() }
}

fn parse_method_tag(body: &str) -> Option<MethodTag> {
    let body = body.trim();
    let paren = body.find('(')?;
    let head = body[..paren].trim();
    let close = matching_paren(body, paren)?;
    let params_text = &body[paren + 1..close];
    let description = body[close + 1..].trim();
    let description = description.strip_prefix(':').map(|d| {
        // `@method name(): ReturnType` style
        d.trim()
    });
    let mut words: Vec<&str> = Vec::new();
    let mut rest = head;
    while !rest.is_empty() {
        let (t, r) = split_type(rest);
        if t.is_empty() {
            break;
        }
        words.push(t);
        rest = r;
    }
    let name = words.pop()?.to_string();
    let mut is_static = false;
    if words.first().is_some_and(|w| w.eq_ignore_ascii_case("static")) && !words.is_empty() {
        // `static` alone before the name is the return type `static` only
        // if no other word follows.
        if words.len() >= 2 {
            is_static = true;
            words.remove(0);
        }
    }
    let mut return_type = words.first().map(|w| w.to_string());
    let description_text;
    if let Some(d) = description {
        let (t, r) = split_type(d);
        if return_type.is_none() && !t.is_empty() {
            return_type = Some(t.to_string());
        }
        description_text = r.to_string();
    } else {
        description_text = body[close + 1..].trim().to_string();
    }
    let params = split_top_level(params_text, ',')
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .filter_map(|p| {
            let p = p.trim();
            let (decl, default) = match p.split_once('=') {
                Some((d, v)) => (d.trim(), Some(v.trim().to_string())),
                None => (p, None),
            };
            let dollar = decl.rfind('$')?;
            let name = decl[dollar + 1..].trim().to_string();
            let before = decl[..dollar].trim();
            let variadic = before.ends_with("...");
            let before = before.trim_end_matches("...").trim_end_matches('&').trim();
            Some(MethodTagParam {
                type_text: if before.is_empty() { None } else { Some(before.to_string()) },
                name,
                default,
                variadic,
            })
        })
        .collect();
    Some(MethodTag { is_static, return_type, name, params, description: description_text })
}

fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

// ---------------------------------------------------------------- type parser

/// Parses a PHPDoc type expression. Class names are passed to `resolve`
/// (which should apply namespace / use resolution). Never fails: unknown
/// syntax yields `mixed`.
pub fn parse_type(text: &str, resolve: &dyn Fn(&str) -> Type) -> Type {
    let tokens = tokenize(text);
    if tokens.is_empty() {
        return Type::Mixed;
    }
    let mut p = TypeParser { tokens, pos: 0, resolve };
    let t = p.union();
    t.unwrap_or(Type::Mixed)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Name(String),
    Var(String),
    Str(String),
    Num(String),
    Punct(char),
    Ellipsis,
}

fn tokenize(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_alphabetic() || c == '_' || c == '\\' || (c as u32) >= 0x80 {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric()
                    || chars[i] == '_'
                    || chars[i] == '\\'
                    || (chars[i] as u32) >= 0x80
                    || (chars[i] == '-' && chars.get(i + 1).is_some_and(|c| c.is_alphabetic())))
            {
                i += 1;
            }
            out.push(Tok::Name(chars[start..i].iter().collect()));
        } else if c == '$' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Var(chars[start..i].iter().collect()));
        } else if c == '\'' || c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != c {
                i += 1;
            }
            i += 1;
            out.push(Tok::Str(chars[start..i.min(chars.len())].iter().collect()));
        } else if c.is_ascii_digit() || (c == '-' && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit())) {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_')
            {
                i += 1;
            }
            out.push(Tok::Num(chars[start..i].iter().collect()));
        } else if c == '.' && chars.get(i + 1) == Some(&'.') && chars.get(i + 2) == Some(&'.') {
            out.push(Tok::Ellipsis);
            i += 3;
        } else {
            out.push(Tok::Punct(c));
            i += 1;
        }
    }
    out
}

struct TypeParser<'a> {
    tokens: Vec<Tok>,
    pos: usize,
    resolve: &'a dyn Fn(&str) -> Type,
}

impl TypeParser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn at_punct(&self, c: char) -> bool {
        self.peek() == Some(&Tok::Punct(c))
    }

    fn eat_punct(&mut self, c: char) -> bool {
        if self.at_punct(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn union(&mut self) -> Option<Type> {
        let first = self.intersection()?;
        if !self.at_punct('|') {
            return Some(first);
        }
        let mut members = vec![first];
        while self.eat_punct('|') {
            if let Some(t) = self.intersection() {
                members.push(t);
            }
        }
        Some(Type::union(members))
    }

    fn intersection(&mut self) -> Option<Type> {
        let first = self.prefix()?;
        if !self.at_punct('&') {
            return Some(first);
        }
        let mut members = vec![first];
        while self.eat_punct('&') {
            if let Some(t) = self.prefix() {
                members.push(t);
            }
        }
        Some(Type::Intersection(members))
    }

    fn prefix(&mut self) -> Option<Type> {
        if self.eat_punct('?') {
            return self.postfix().map(Type::nullable);
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Option<Type> {
        let mut t = self.atom()?;
        while self.at_punct('[') && self.tokens.get(self.pos + 1) == Some(&Tok::Punct(']')) {
            self.pos += 2;
            t = Type::array_of(t);
        }
        Some(t)
    }

    fn generic_args(&mut self) -> Vec<Type> {
        let mut args = Vec::new();
        if !self.eat_punct('<') {
            return args;
        }
        loop {
            if self.eat_punct('>') {
                break;
            }
            match self.union() {
                Some(t) => args.push(t),
                None => {
                    // skip unknown token
                    if self.peek().is_none() {
                        break;
                    }
                    self.pos += 1;
                }
            }
            if !self.eat_punct(',') {
                self.eat_punct('>');
                break;
            }
        }
        args
    }

    fn atom(&mut self) -> Option<Type> {
        let tok = self.peek()?.clone();
        match tok {
            Tok::Punct('(') => {
                self.pos += 1;
                let t = self.union();
                // conditional types `(T is X ? A : B)` -> union of branches
                if let Some(Tok::Name(n)) = self.peek() {
                    if n == "is" {
                        self.pos += 1;
                        self.union();
                        let mut branches = Vec::new();
                        if self.eat_punct('?') {
                            if let Some(a) = self.union() {
                                branches.push(a);
                            }
                            if self.eat_punct(':') {
                                if let Some(b) = self.union() {
                                    branches.push(b);
                                }
                            }
                        }
                        self.eat_punct(')');
                        return Some(Type::union(branches));
                    }
                }
                self.eat_punct(')');
                t
            }
            Tok::Var(v) => {
                self.pos += 1;
                if v == "$this" {
                    Some(Type::Static)
                } else {
                    Some(Type::Mixed)
                }
            }
            Tok::Str(s) | Tok::Num(s) => {
                self.pos += 1;
                Some(Type::Literal(s))
            }
            Tok::Name(name) => {
                self.pos += 1;
                let lower = name.to_ascii_lowercase();
                // shapes
                if self.at_punct('{')
                    && matches!(lower.as_str(), "array" | "list" | "object" | "non-empty-array")
                {
                    let entries = self.shape();
                    return Some(if lower == "object" { Type::Object } else { Type::Shape(entries) });
                }
                if matches!(
                    lower.as_str(),
                    "callable" | "closure" | "\\closure" | "pure-callable" | "pure-closure"
                ) && self.at_punct('(')
                {
                    let (params, ret) = self.callable_sig();
                    return Some(Type::Closure(params, ret.map(Box::new)));
                }
                // `Foo::BAR` constant references
                if self.at_punct(':') && self.tokens.get(self.pos + 1) == Some(&Tok::Punct(':')) {
                    self.pos += 2;
                    if let Some(Tok::Name(_) | Tok::Punct('*')) = self.peek() {
                        self.pos += 1;
                    }
                    while let Some(Tok::Name(_) | Tok::Punct('*')) = self.peek() {
                        self.pos += 1;
                    }
                    return Some(Type::Mixed);
                }
                let args = self.generic_args();
                Some(self.named(&lower, &name, args))
            }
            Tok::Punct('*') => {
                self.pos += 1;
                Some(Type::Mixed)
            }
            _ => None,
        }
    }

    fn named(&self, lower: &str, name: &str, mut args: Vec<Type>) -> Type {
        let two = |args: &mut Vec<Type>| -> (Option<Box<Type>>, Option<Box<Type>>) {
            match args.len() {
                0 => (None, None),
                1 => (None, Some(Box::new(args.remove(0)))),
                _ => {
                    let k = args.remove(0);
                    let v = args.remove(0);
                    (Some(Box::new(k)), Some(Box::new(v)))
                }
            }
        };
        match lower {
            "array" | "non-empty-array" | "associative-array" => {
                let (k, v) = two(&mut args);
                Type::Array(k, v)
            }
            "list" | "non-empty-list" => Type::List(args.into_iter().next().map(Box::new)),
            "iterable" => {
                let (k, v) = two(&mut args);
                Type::Iterable(k, v)
            }
            "class-string" | "interface-string" | "enum-string" | "trait-string" => {
                Type::ClassString(args.into_iter().next().map(Box::new))
            }
            "key-of" | "value-of" | "int-mask" | "int-mask-of" => Type::Mixed,
            "int" | "integer" => Type::Int,
            _ => match builtin_type(name) {
                Some(t) => t,
                None => match (self.resolve)(name) {
                    Type::Named(n, _) => Type::Named(n, args),
                    other => other,
                },
            },
        }
    }

    fn shape(&mut self) -> Vec<ShapeEntry> {
        let mut entries = Vec::new();
        self.eat_punct('{');
        let mut index = 0;
        loop {
            if self.eat_punct('}') || self.peek().is_none() {
                break;
            }
            if self.peek() == Some(&Tok::Ellipsis) {
                self.pos += 1;
                self.generic_args();
                self.eat_punct(',');
                continue;
            }
            // `key?: type` or just `type`
            let save = self.pos;
            let key = match self.peek().cloned() {
                Some(Tok::Name(n)) | Some(Tok::Num(n)) => Some(n),
                Some(Tok::Str(s)) => Some(s.trim_matches(['\'', '"']).to_string()),
                _ => None,
            };
            let mut entry_key = None;
            let mut optional = false;
            if let Some(k) = key {
                self.pos += 1;
                if self.eat_punct('?') {
                    optional = true;
                }
                if self.eat_punct(':') {
                    entry_key = Some(k);
                } else {
                    self.pos = save;
                    optional = false;
                }
            }
            let ty = self.union().unwrap_or(Type::Mixed);
            let key = entry_key.unwrap_or_else(|| {
                let k = index.to_string();
                index += 1;
                k
            });
            entries.push(ShapeEntry { key, ty, optional });
            if !self.eat_punct(',') {
                self.eat_punct('}');
                break;
            }
        }
        entries
    }

    fn callable_sig(&mut self) -> (Vec<Type>, Option<Type>) {
        let mut params = Vec::new();
        self.eat_punct('(');
        loop {
            if self.eat_punct(')') || self.peek().is_none() {
                break;
            }
            let t = self.union().unwrap_or(Type::Mixed);
            // optional `...`, `&`, `$name`, `=`
            while matches!(self.peek(), Some(Tok::Ellipsis | Tok::Var(_) | Tok::Punct('=') | Tok::Punct('&')))
            {
                self.pos += 1;
            }
            params.push(t);
            if !self.eat_punct(',') {
                self.eat_punct(')');
                break;
            }
        }
        let ret = if self.eat_punct(':') { self.prefix() } else { None };
        (params, ret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(s: &str) -> String {
        parse_type(s, &|n| Type::named(n)).to_string()
    }

    #[test]
    fn types() {
        assert_eq!(ty("int|string|null"), "int|string|null");
        assert_eq!(ty("?Foo"), "?Foo");
        assert_eq!(ty("Foo[]"), "Foo[]");
        assert_eq!(ty("array<int, Foo>"), "array<int, Foo>");
        assert_eq!(ty("list<string>"), "list<string>");
        assert_eq!(ty("array{name: string, age?: int}"), "array{name: string, age?: int}");
        assert_eq!(ty("Collection<int, User>"), "Collection<int, User>");
        assert_eq!(ty("callable(int, string): bool"), "Closure(int, string): bool");
        assert_eq!(ty("class-string<Model>"), "class-string<Model>");
        assert_eq!(ty("$this"), "static");
        assert_eq!(ty("(A&B)|null"), "(A&B)|null");
        assert_eq!(ty("'a'|'b'"), "'a'|'b'");
        assert_eq!(ty("($x is int ? A : B)"), "A|B");
        assert_eq!(ty("non-empty-string"), "string");
    }

    #[test]
    fn doc_block() {
        let doc = DocBlock::parse(
            "/**\n * Finds a user.\n *\n * Longer text.\n *\n * @param int|string $id The id\n * @param array<string, mixed> $opts\n * @return User|null\n * @phpstan-return ?User\n * @throws NotFound\n * @deprecated use get()\n * @property-read string $name\n * @method static self make(int $a, string ...$rest) Create\n * @template T of Model\n */",
        );
        assert_eq!(doc.summary, "Finds a user.");
        assert_eq!(doc.description, "Longer text.");
        let p = doc.param("$id").unwrap();
        assert_eq!(p.type_text, "int|string");
        assert_eq!(p.description, "The id");
        assert_eq!(doc.param("opts").unwrap().type_text, "array<string, mixed>");
        assert_eq!(doc.return_tag().unwrap().type_text, "?User");
        assert_eq!(doc.throws()[0].type_text, "NotFound");
        assert_eq!(doc.deprecated().as_deref(), Some("use get()"));
        let props = doc.properties();
        assert_eq!(props[0].0.var.as_deref(), Some("name"));
        assert_eq!(props[0].1, PropertyAccess::Read);
        let m = &doc.methods()[0];
        assert!(m.is_static);
        assert_eq!(m.return_type.as_deref(), Some("self"));
        assert_eq!(m.name, "make");
        assert_eq!(m.params.len(), 2);
        assert!(m.params[1].variadic);
        assert_eq!(doc.templates()[0].bound.as_deref(), Some("Model"));
    }

    #[test]
    fn inline_var() {
        let doc = DocBlock::parse("/** @var \\App\\User $user */");
        let v = doc.var(Some("user")).unwrap();
        assert_eq!(v.type_text, "\\App\\User");
    }
}
