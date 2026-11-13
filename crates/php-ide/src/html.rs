//! Basic HTML support for inline HTML in PHP templates: tag / attribute /
//! closing tag completion and linked editing of matching tag names.

use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange, TextSize};

use crate::completion::{CompletionItem, CompletionKind};

const ELEMENTS: &[&str] = &[
    "a",
    "abbr",
    "address",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "base",
    "bdi",
    "bdo",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "cite",
    "code",
    "col",
    "colgroup",
    "data",
    "datalist",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "link",
    "main",
    "map",
    "mark",
    "menu",
    "meta",
    "meter",
    "nav",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "picture",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "script",
    "search",
    "section",
    "select",
    "slot",
    "small",
    "source",
    "span",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "svg",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
];

const VOID: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];

const GLOBAL_ATTRS: &[&str] = &[
    "accesskey",
    "autofocus",
    "class",
    "contenteditable",
    "dir",
    "draggable",
    "hidden",
    "id",
    "inert",
    "lang",
    "role",
    "slot",
    "spellcheck",
    "style",
    "tabindex",
    "title",
    "translate",
    "onblur",
    "onchange",
    "onclick",
    "ondblclick",
    "onfocus",
    "oninput",
    "onkeydown",
    "onkeyup",
    "onload",
    "onmousedown",
    "onmouseenter",
    "onmouseleave",
    "onmouseover",
    "onmouseup",
    "onsubmit",
];

fn element_attrs(tag: &str) -> &'static [&'static str] {
    match tag {
        "a" => &["href", "target", "rel", "download", "hreflang", "type", "referrerpolicy"],
        "img" => &["src", "alt", "width", "height", "loading", "srcset", "sizes", "decoding", "crossorigin"],
        "input" => &[
            "type",
            "name",
            "value",
            "placeholder",
            "required",
            "checked",
            "disabled",
            "readonly",
            "min",
            "max",
            "step",
            "pattern",
            "autocomplete",
            "maxlength",
            "minlength",
            "multiple",
            "accept",
            "form",
            "list",
            "size",
        ],
        "form" => {
            &["action", "method", "enctype", "novalidate", "target", "autocomplete", "name", "accept-charset"]
        }
        "button" => &["type", "name", "value", "disabled", "form", "formaction", "formmethod"],
        "select" => &["name", "multiple", "required", "disabled", "size", "form", "autocomplete"],
        "option" => &["value", "selected", "disabled", "label"],
        "textarea" => {
            &["name", "rows", "cols", "placeholder", "required", "disabled", "readonly", "maxlength", "wrap"]
        }
        "label" => &["for", "form"],
        "link" => &["rel", "href", "type", "media", "sizes", "crossorigin", "integrity", "as"],
        "meta" => &["name", "content", "charset", "http-equiv", "property"],
        "script" => &["src", "type", "async", "defer", "crossorigin", "integrity", "nomodule", "nonce"],
        "style" => &["media", "nonce"],
        "iframe" => {
            &["src", "srcdoc", "name", "width", "height", "allow", "allowfullscreen", "loading", "sandbox"]
        }
        "video" | "audio" => {
            &["src", "controls", "autoplay", "loop", "muted", "preload", "poster", "width", "height"]
        }
        "source" => &["src", "srcset", "type", "media", "sizes"],
        "td" | "th" => &["colspan", "rowspan", "headers", "scope"],
        "html" => &["lang", "xmlns"],
        "ol" => &["start", "reversed", "type"],
        "li" => &["value"],
        "time" => &["datetime"],
        "details" | "dialog" => &["open"],
        _ => &[],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tag {
    name: String,
    name_range: TextRange,
    closing: bool,
    self_closing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Text,
    /// Inside `<name ...` or `</name ...`.
    InTag {
        tag: Tag,
        quote: Option<char>,
        after_name: bool,
        attr_start: Option<TextSize>,
    },
    Comment,
    /// Inside `<script>` / `<style>` contents.
    Raw(String),
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '_' | '.')
}

/// Scans all inline HTML of a file (state carries over PHP blocks), up to
/// `stop`. Returns the completed tags and the state at `stop`.
fn scan(root: &SyntaxNode, stop: Option<TextSize>) -> (Vec<Tag>, State) {
    let mut tags = Vec::new();
    let mut state = State::Text;
    for tok in
        root.descendants_with_tokens().filter_map(|e| e.into_token()).filter(|t| t.kind() == INLINE_HTML)
    {
        let base = tok.text_range().start();
        if stop.is_some_and(|s| base > s) {
            break;
        }
        let text = tok.text();
        let mut i = 0;
        while i < text.len() {
            let at = base + TextSize::from(i as u32);
            if stop.is_some_and(|s| at >= s) {
                return (tags, state);
            }
            let rest = &text[i..];
            let c = rest.chars().next().unwrap();
            match &mut state {
                State::Text => {
                    if rest.starts_with("<!--") {
                        state = State::Comment;
                        i += 4;
                        continue;
                    }
                    if c == '<' {
                        let closing = rest[1..].starts_with('/');
                        let name_start = i + 1 + closing as usize;
                        if text[name_start..].starts_with(|c: char| c.is_ascii_alphabetic())
                            || stop.is_some_and(|s| base + TextSize::from(name_start as u32) == s)
                        {
                            let len = text[name_start..]
                                .find(|c| !is_name_char(c))
                                .unwrap_or(text.len() - name_start);
                            let name_range = TextRange::at(
                                base + TextSize::from(name_start as u32),
                                TextSize::from(len as u32),
                            );
                            state = State::InTag {
                                tag: Tag {
                                    name: text[name_start..name_start + len].to_string(),
                                    name_range,
                                    closing,
                                    self_closing: false,
                                },
                                quote: None,
                                after_name: false,
                                attr_start: None,
                            };
                            i = name_start;
                            continue;
                        }
                    }
                }
                State::InTag { tag, quote, after_name, attr_start } => {
                    if !*after_name {
                        if at >= tag.name_range.end() {
                            *after_name = true;
                            continue;
                        }
                    } else if let Some(q) = *quote {
                        if c == q {
                            *quote = None;
                        }
                    } else if c == '"' || c == '\'' {
                        *quote = Some(c);
                        *attr_start = None;
                    } else if c == '>' {
                        let mut tag = tag.clone();
                        tag.self_closing = text[..i].ends_with('/');
                        let raw = !tag.closing
                            && matches!(tag.name.to_ascii_lowercase().as_str(), "script" | "style");
                        let name = tag.name.clone();
                        tags.push(tag);
                        state = if raw { State::Raw(name) } else { State::Text };
                    } else if is_name_char(c) {
                        if attr_start.is_none() {
                            *attr_start = Some(at);
                        }
                    } else {
                        *attr_start = None;
                    }
                }
                State::Comment => {
                    if rest.starts_with("-->") {
                        state = State::Text;
                        i += 3;
                        continue;
                    }
                }
                State::Raw(name) => {
                    if rest.starts_with("</")
                        && rest.get(2..2 + name.len()).is_some_and(|n| n.eq_ignore_ascii_case(name))
                    {
                        // re-scan `</name` as a closing tag
                        state = State::Text;
                        continue;
                    }
                }
            }
            i += c.len_utf8();
        }
    }
    (tags, state)
}

/// Elements still open after `tags`, innermost last.
fn open_elements(tags: &[Tag]) -> Vec<&Tag> {
    let mut stack: Vec<&Tag> = Vec::new();
    for t in tags {
        if t.closing {
            if let Some(pos) = stack.iter().rposition(|o| o.name.eq_ignore_ascii_case(&t.name)) {
                stack.truncate(pos);
            }
        } else if !t.self_closing && !VOID.contains(&t.name.to_ascii_lowercase().as_str()) {
            stack.push(t);
        }
    }
    stack
}

fn text_before(root: &SyntaxNode, offset: TextSize) -> String {
    root.token_at_offset(offset)
        .left_biased()
        .map(|t| t.text()[..usize::from(offset - t.text_range().start())].to_string())
        .unwrap_or_default()
}

fn in_html(root: &SyntaxNode, offset: TextSize) -> bool {
    root.token_at_offset(offset).left_biased().is_some_and(|t| t.kind() == INLINE_HTML)
}

pub(crate) fn completions(root: &SyntaxNode, offset: TextSize) -> Option<Vec<CompletionItem>> {
    if !in_html(root, offset) {
        return None;
    }
    let (tags, state) = scan(root, Some(offset));
    let State::InTag { tag, quote: None, after_name, attr_start } = state else { return Some(Vec::new()) };
    let mut items = Vec::new();
    if !after_name {
        // cursor between `<` and `/` (the name starts after it)
        if tag.name_range.start() > offset {
            return Some(Vec::new());
        }
        let range = TextRange::new(tag.name_range.start(), offset);
        if tag.closing {
            if let Some(open) = open_elements(&tags).last() {
                let mut item = CompletionItem::new(open.name.clone(), CompletionKind::Snippet, range);
                item.insert_text = format!("{}>", open.name);
                items.push(item);
            }
        } else {
            for &e in ELEMENTS {
                let mut item = CompletionItem::new(e, CompletionKind::Snippet, range);
                item.detail = Some("HTML element".into());
                items.push(item);
            }
        }
    } else if !tag.closing && !text_before(root, offset).ends_with('=') {
        let start = attr_start.unwrap_or(offset).min(offset);
        let range = TextRange::new(start, offset);
        let lower = tag.name.to_ascii_lowercase();
        for &a in element_attrs(&lower).iter().chain(GLOBAL_ATTRS) {
            let mut item = CompletionItem::new(a, CompletionKind::Property, range);
            item.insert_text = format!("{a}=\"$1\"");
            item.is_snippet = true;
            item.detail = Some("HTML attribute".into());
            items.push(item);
        }
    }
    Some(items)
}

/// Ranges of the opening and closing tag names for a tag name at `offset`.
pub(crate) fn linked_editing_ranges(root: &SyntaxNode, offset: TextSize) -> Option<Vec<TextRange>> {
    if !in_html(root, offset)
        && !root.token_at_offset(offset).right_biased().is_some_and(|t| t.kind() == INLINE_HTML)
    {
        return None;
    }
    let (tags, _) = scan(root, None);
    let idx = tags.iter().position(|t| t.name_range.contains_inclusive(offset))?;
    let mut stack: Vec<usize> = Vec::new();
    for (i, t) in tags.iter().enumerate() {
        if t.closing {
            let Some(pos) = stack.iter().rposition(|&o| tags[o].name.eq_ignore_ascii_case(&t.name)) else {
                continue;
            };
            let open = stack[pos];
            stack.truncate(pos);
            if open == idx || i == idx {
                return Some(vec![tags[open].name_range, t.name_range]);
            }
        } else if !t.self_closing && !VOID.contains(&t.name.to_ascii_lowercase().as_str()) {
            stack.push(i);
        }
    }
    None
}
