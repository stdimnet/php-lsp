//! Class names referenced from PHPDoc comments (`@param User $u`,
//! `@return Collection<int, Post>`, ...).

use php_syntax::{SyntaxNode, SyntaxToken, TextRange, TextSize};

/// A class-like name in a doc comment, with its range in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocTypeRef {
    pub range: TextRange,
    pub name: String,
}

const TYPE_TAGS: &[&str] = &[
    "param",
    "return",
    "var",
    "throws",
    "property",
    "property-read",
    "property-write",
    "method",
    "extends",
    "implements",
    "use",
    "mixin",
    "template",
    "template-covariant",
    "template-contravariant",
    "template-extends",
    "template-implements",
];

/// Pseudo types and words that are never class names.
const NOT_CLASSES: &[&str] = &[
    "of",
    "as",
    "is",
    "not",
    "null",
    "true",
    "false",
    "mixed",
    "void",
    "never",
    "int",
    "integer",
    "float",
    "double",
    "string",
    "bool",
    "boolean",
    "array",
    "object",
    "callable",
    "iterable",
    "resource",
    "self",
    "static",
    "parent",
    "list",
    "key-of",
    "value-of",
    "class-string",
    "interface-string",
    "trait-string",
    "enum-string",
    "array-key",
    "scalar",
    "numeric",
    "number",
    "positive-int",
    "negative-int",
    "non-negative-int",
    "non-positive-int",
    "non-zero-int",
    "non-empty-string",
    "numeric-string",
    "literal-string",
    "lowercase-string",
    "non-empty-array",
    "non-empty-list",
    "callable-string",
    "callable-array",
    "closed-resource",
    "open-resource",
    "non-falsy-string",
    "truthy-string",
    "int-mask",
    "int-mask-of",
    "pure-callable",
    "empty",
    "noreturn",
    "never-return",
    "max",
    "min",
    "non-empty-mixed",
    "pure-closure",
    "closure",
];

fn tag_name(rest: &str) -> &str {
    let end = rest.find(|c: char| c.is_whitespace() || c == '(' || c == '{').unwrap_or(rest.len());
    &rest[..end]
}

/// Class names used in type positions of a doc comment token.
pub(crate) fn doc_type_refs(token: &SyntaxToken) -> Vec<DocTypeRef> {
    let text = token.text();
    let base = usize::from(token.text_range().start());
    let mut out = Vec::new();
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let line_off = line_start;
        line_start += line.len();
        let Some(at) = line.find('@') else { continue };
        let rest = &line[at + 1..];
        let name = tag_name(rest);
        let plain = name.trim_start_matches("phpstan-").trim_start_matches("psalm-");
        if !TYPE_TAGS.contains(&plain.to_ascii_lowercase().as_str()) {
            continue;
        }
        let body_start = at + 1 + name.len();
        let body = &line[body_start..];
        let lead = body.len() - body.trim_start().len();
        let body_trim = body.trim_start();
        // the type expression; for @template it is `T of Bound`
        let (ty, offset) = if plain.starts_with("template") {
            match body_trim.split_once(" of ").or_else(|| body_trim.split_once(" as ")) {
                Some((_, bound)) => {
                    let pos = body_trim.len() - bound.len();
                    (php_hir::phpdoc::split_type(bound).0, pos)
                }
                None => continue,
            }
        } else if plain == "method" {
            // return type before the method name: `@method static Foo bar()`
            let head = body_trim.split('(').next().unwrap_or("");
            let mut words: Vec<&str> = head.split_whitespace().collect();
            words.pop(); // method name
            if words.first().is_some_and(|w| w.eq_ignore_ascii_case("static")) && words.len() > 1 {
                words.remove(0);
            }
            match words.first() {
                Some(w) => (*w, body_trim.find(w).unwrap_or(0)),
                None => continue,
            }
        } else {
            let (t, _) = php_hir::phpdoc::split_type(body_trim);
            if t.starts_with('$') {
                continue;
            }
            (t, 0)
        };
        let ty_base = base + line_off + body_start + lead + offset;
        scan_names(ty, ty_base, &mut out);
    }
    out
}

fn scan_names(ty: &str, base: usize, out: &mut Vec<DocTypeRef>) {
    let bytes = ty.as_bytes();
    let mut i = 0;
    let mut in_quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_quote {
            if b == q {
                in_quote = None;
            }
            i += 1;
            continue;
        }
        if b == b'\'' || b == b'"' {
            in_quote = Some(b);
            i += 1;
            continue;
        }
        if b == b'$' {
            // `$this` / variable names
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' || b == b'\\' || b >= 0x80 {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || bytes[i] == b'_'
                    || bytes[i] == b'\\'
                    || bytes[i] >= 0x80
                    || bytes[i] == b'-' && bytes.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()))
            {
                i += 1;
            }
            let word = &ty[start..i];
            // shape keys (`name: type`, `name?: type`) and constants (`Foo::BAR` keeps Foo)
            let after = ty[i..].trim_start();
            let is_key = (after.starts_with(':') && !after.starts_with("::")) || after.starts_with("?:");
            if !is_key
                && !word.is_empty()
                && !NOT_CLASSES.contains(&word.to_ascii_lowercase().as_str())
                && !word.chars().all(|c| c.is_ascii_digit())
            {
                out.push(DocTypeRef {
                    range: TextRange::new(
                        TextSize::from((base + start) as u32),
                        TextSize::from((base + i) as u32),
                    ),
                    name: word.to_string(),
                });
            }
            continue;
        }
        i += 1;
    }
}

/// Names declared with `@template` anywhere in the file.
pub(crate) fn template_names(root: &SyntaxNode) -> Vec<String> {
    let mut out = Vec::new();
    for t in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        if t.kind() != php_syntax::SyntaxKind::DOC_COMMENT
            || !t.text().contains("@template") && !t.text().contains("-template")
        {
            continue;
        }
        let doc = php_hir::DocBlock::parse(t.text());
        out.extend(doc.templates().into_iter().map(|t| t.name));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_names() {
        let text = "<?php\n/**\n * @param array<int, User>|null $users\n * @return Collection<Post>\n * @throws \\App\\NotFound\n * @var array{name: Name, 'x': int}\n * @method static Builder query()\n * @template T of Model\n */\nfunction f() {}";
        let parse = php_syntax::parse(text, php_syntax::PhpVersion::LATEST);
        let tok = parse
            .syntax_node()
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| t.kind() == php_syntax::SyntaxKind::DOC_COMMENT)
            .unwrap();
        let names: Vec<(String, &str)> =
            doc_type_refs(&tok).into_iter().map(|r| (r.name, &text[r.range])).collect();
        let names: Vec<&str> = names.iter().map(|(_, t)| *t).collect();
        assert_eq!(names, vec!["User", "Collection", "Post", "\\App\\NotFound", "Name", "Builder", "Model"]);
    }
}
