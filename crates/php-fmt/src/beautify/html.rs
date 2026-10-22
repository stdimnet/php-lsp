//! js-beautify's HTML beautifier (`js/src/html`), with the change
//! Intelephense makes: a text token `<?php N ?>` changes the indentation by
//! `N` levels (it stands for PHP code opening or closing blocks).
//!
//! js-beautify is MIT licensed, Copyright (c) 2007-2018 Einar Lielmanis,
//! Liam Newman, and contributors.

use super::core::{
    BaseOptions, Directives, InputScanner, Output, Pattern, Re, TemplatablePattern, WhitespacePattern,
};

/// A formatter for embedded code.
type Formatter<'a> = Box<dyn Fn(&str) -> String + 'a>;

/// Formats embedded code: `(text, options) -> formatted`.
pub(crate) type SubBeautifier = fn(&str, &ChildOptions) -> String;

/// What the HTML options pass on to the script / style beautifiers.
#[derive(Clone, Debug)]
pub(crate) struct ChildOptions {
    pub indent_size: usize,
    pub use_tabs: bool,
    pub wrap_line_length: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct HtmlOptions {
    pub base: BaseOptions,
    pub indent_inner_html: bool,
    pub indent_body_inner_html: bool,
    pub indent_head_inner_html: bool,
    pub indent_handlebars: bool,
    pub wrap_attributes: String,
    pub wrap_attributes_min_attrs: usize,
    pub wrap_attributes_indent_size: usize,
    pub extra_liners: Vec<String>,
    pub inline: Vec<String>,
    pub inline_custom_elements: bool,
    pub void_elements: Vec<String>,
    pub unformatted: Vec<String>,
    pub content_unformatted: Vec<String>,
    pub unformatted_content_delimiter: String,
    pub indent_scripts: String,
}

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

impl HtmlOptions {
    /// js-beautify's defaults with Intelephense's settings: indentation
    /// from the editor, `wrap_attributes: preserve`, templating `auto`.
    pub fn intelephense(indent_size: usize, use_tabs: bool) -> HtmlOptions {
        let mut base = BaseOptions::new(indent_size, use_tabs);
        base.templating = list(&["django", "erb", "handlebars", "php"]);
        let indent_size = base.indent_size;
        HtmlOptions {
            base,
            indent_inner_html: false,
            indent_body_inner_html: true,
            indent_head_inner_html: true,
            indent_handlebars: true,
            wrap_attributes: "preserve".into(),
            wrap_attributes_min_attrs: 2,
            wrap_attributes_indent_size: indent_size,
            extra_liners: list(&["head", "body", "/html"]),
            inline: list(&[
                "a", "abbr", "area", "audio", "b", "bdi", "bdo", "br", "button", "canvas", "cite", "code",
                "data", "datalist", "del", "dfn", "em", "embed", "i", "iframe", "img", "input", "ins", "kbd",
                "keygen", "label", "map", "mark", "math", "meter", "noscript", "object", "output",
                "progress", "q", "ruby", "s", "samp", "select", "small", "span", "strong", "sub", "sup",
                "svg", "template", "textarea", "time", "u", "var", "video", "wbr", "text", "acronym", "big",
                "strike", "tt",
            ]),
            inline_custom_elements: true,
            void_elements: list(&[
                "area", "base", "br", "col", "embed", "hr", "img", "input", "keygen", "link", "menuitem",
                "meta", "param", "source", "track", "wbr", "!doctype", "?xml", "basefont", "isindex",
            ]),
            unformatted: Vec::new(),
            content_unformatted: list(&["pre", "textarea"]),
            unformatted_content_delimiter: String::new(),
            indent_scripts: "normal".into(),
        }
    }

    fn child(&self) -> ChildOptions {
        ChildOptions {
            indent_size: self.base.indent_size,
            use_tabs: self.base.indent_with_tabs,
            wrap_line_length: self.base.wrap_line_length,
        }
    }
}

// ------------------------------------------------------------------ tokenizer

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum T {
    TagOpen,
    TagClose,
    ControlFlowOpen,
    ControlFlowClose,
    Attribute,
    Equals,
    Value,
    Comment,
    Text,
    Unknown,
    Start,
    Eof,
    /// Parser token type of text content (`TK_CONTENT`).
    Content,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub kind: T,
    pub text: String,
    pub newlines: usize,
    pub whitespace_before: String,
    pub parent: Option<usize>,
    pub next: Option<usize>,
    pub previous: Option<usize>,
    pub opened: Option<usize>,
    pub closed: Option<usize>,
    pub directives: Option<Vec<(String, String)>>,
}

struct Patterns {
    word: TemplatablePattern,
    word_control_flow_close_excluded: TemplatablePattern,
    single_quote: TemplatablePattern,
    double_quote: TemplatablePattern,
    attribute: TemplatablePattern,
    element_name: TemplatablePattern,
    angular_control_flow_start: Pattern,
    handlebars_comment: Pattern,
    handlebars: Pattern,
    handlebars_open: Pattern,
    handlebars_raw_close: Pattern,
    comment: Pattern,
    cdata: Pattern,
    conditional_comment: Pattern,
    processing: Pattern,
    unformatted_content_delimiter: Option<Pattern>,
}

struct Tokenizer<'o> {
    input: InputScanner,
    options: &'o HtmlOptions,
    whitespace: WhitespacePattern,
    patterns: Patterns,
    directives: Directives,
    tokens: Vec<Token>,
}

impl<'o> Tokenizer<'o> {
    fn new(text: &str, options: &'o HtmlOptions) -> Tokenizer<'o> {
        let templatable = TemplatablePattern::new().read_options(&options.base.templating);
        let p = Pattern::default();
        let mut patterns = Patterns {
            word: templatable.until(r"[\n\r\t <]"),
            word_control_flow_close_excluded: templatable.until(r"[\n\r\t <}]"),
            single_quote: templatable.until_after("'"),
            double_quote: templatable.until_after("\""),
            attribute: templatable.until(r"[\n\r\t =>]|/>"),
            element_name: templatable.until(r"[\n\r\t >/]"),
            angular_control_flow_start: p.matching(r"@[a-zA-Z]+[^({]*[({]"),
            handlebars_comment: p.starting_with(r"\{\{!--").until_after(r"--\}\}"),
            handlebars: p.starting_with(r"\{\{").until_after(r"\}\}"),
            handlebars_open: p.until(r"[\n\r\t }]"),
            handlebars_raw_close: p.until(r"\}\}"),
            comment: p.starting_with("<!--").until_after("-->"),
            cdata: p.starting_with(r"<!\[CDATA\[").until_after(r"\]\]>"),
            conditional_comment: p.starting_with(r"<!\[").until_after(r"\]>"),
            processing: p.starting_with(r"<\?").until_after(r"\?>"),
            unformatted_content_delimiter: None,
        };
        if options.indent_handlebars {
            patterns.word = patterns.word.exclude("handlebars");
            patterns.word_control_flow_close_excluded =
                patterns.word_control_flow_close_excluded.exclude("handlebars");
        }
        if !options.unformatted_content_delimiter.is_empty() {
            let lit = super::core::escape(&options.unformatted_content_delimiter);
            patterns.unformatted_content_delimiter = Some(p.matching(&lit).until_after(&lit));
        }
        Tokenizer {
            input: InputScanner::new(text),
            options,
            whitespace: WhitespacePattern::new(),
            patterns,
            directives: Directives::new("<!--", "-->"),
            tokens: Vec::new(),
        }
    }

    fn create(&self, kind: T, text: impl Into<String>) -> Token {
        Token {
            kind,
            text: text.into(),
            newlines: self.whitespace.newline_count,
            whitespace_before: self.whitespace.whitespace_before_token.clone(),
            parent: None,
            next: None,
            previous: None,
            opened: None,
            closed: None,
            directives: None,
        }
    }

    fn tokenize(mut self) -> Vec<Token> {
        self.input.restart();
        self.tokens.clear();
        let start = Token {
            kind: T::Start,
            text: String::new(),
            newlines: 0,
            whitespace_before: String::new(),
            parent: None,
            next: None,
            previous: None,
            opened: None,
            closed: None,
            directives: None,
        };
        // the start token is kept at index 0 so `previous` always exists
        self.tokens.push(start);
        let mut previous = 0usize;
        let mut open_token: Option<usize> = None;
        let mut open_stack: Vec<Option<usize>> = Vec::new();
        while self.tokens[previous].kind != T::Eof {
            let mut current = self.next_token(previous, open_token);
            let index = self.tokens.len();
            current.parent = open_token;
            if matches!(current.kind, T::TagOpen | T::ControlFlowOpen) {
                open_stack.push(open_token);
                open_token = Some(index);
            } else if let Some(o) = open_token {
                if self.is_closing(&current, o) {
                    current.opened = Some(o);
                    self.tokens[o].closed = Some(index);
                    open_token = open_stack.pop().flatten();
                    current.parent = open_token;
                }
            }
            current.previous = Some(previous);
            self.tokens[previous].next = Some(index);
            self.tokens.push(current);
            previous = index;
        }
        self.tokens
    }

    fn is_closing(&self, current: &Token, open: usize) -> bool {
        let o = &self.tokens[open].text;
        (current.kind == T::TagClose
            && (((current.text == ">" || current.text == "/>") && o.starts_with('<'))
                || (current.text == "}}" && o.starts_with("{{"))))
            || (current.kind == T::ControlFlowClose && current.text == "}" && o.ends_with('{'))
    }

    fn next_token(&mut self, previous: usize, open: Option<usize>) -> Token {
        self.whitespace.read(&mut self.input);
        let Some(c) = self.input.peek0() else { return self.create(T::Eof, "") };
        if let Some(t) = self.read_open_handlebars(c, open) {
            return t;
        }
        if let Some(t) = self.read_attribute(c, previous, open) {
            return t;
        }
        if let Some(t) = self.read_close(c, open) {
            return t;
        }
        if let Some(t) = self.read_control_flows(c, open) {
            return t;
        }
        if let Some(t) = self.read_raw_content(c, previous, open) {
            return t;
        }
        if let Some(t) = self.read_content_word(c, open) {
            return t;
        }
        if let Some(t) = self.read_comment_or_cdata(c) {
            return t;
        }
        if let Some(t) = self.read_processing(c) {
            return t;
        }
        if let Some(t) = self.read_open(c, open) {
            return t;
        }
        let c = self.input.next().map(String::from).unwrap_or_default();
        self.create(T::Unknown, c)
    }

    fn open_kind(&self, open: Option<usize>) -> Option<T> {
        open.map(|o| self.tokens[o].kind)
    }

    fn read_comment_or_cdata(&mut self, c: char) -> Option<Token> {
        if c != '<' || self.input.peek(1) != Some('!') {
            return None;
        }
        let mut directives = None;
        let mut s = self.patterns.comment.read(&mut self.input);
        if !s.is_empty() {
            directives = self.directives.get(&s);
            if directives.as_ref().is_some_and(|d| d.iter().any(|(k, v)| k == "ignore" && v == "start")) {
                s.push_str(&self.directives.read_ignored(&mut self.input));
            }
        } else {
            s = self.patterns.cdata.read(&mut self.input);
        }
        if s.is_empty() {
            return None;
        }
        let mut t = self.create(T::Comment, s);
        t.directives = directives;
        Some(t)
    }

    fn read_processing(&mut self, c: char) -> Option<Token> {
        if c != '<' {
            return None;
        }
        let peek1 = self.input.peek(1);
        if peek1 != Some('!') && peek1 != Some('?') {
            return None;
        }
        let mut s = self.patterns.conditional_comment.read(&mut self.input);
        if s.is_empty() {
            s = self.patterns.processing.read(&mut self.input);
        }
        (!s.is_empty()).then(|| self.create(T::Comment, s))
    }

    fn read_open(&mut self, c: char, open: Option<usize>) -> Option<Token> {
        if open.is_some() && self.open_kind(open) != Some(T::ControlFlowOpen) {
            return None;
        }
        if c != '<' {
            return None;
        }
        let mut s = String::from(self.input.next()?);
        if self.input.peek0() == Some('/') {
            s.push(self.input.next()?);
        }
        s.push_str(&self.patterns.element_name.read(&mut self.input));
        Some(self.create(T::TagOpen, s))
    }

    fn read_open_handlebars(&mut self, c: char, open: Option<usize>) -> Option<Token> {
        if open.is_some() && self.open_kind(open) != Some(T::ControlFlowOpen) {
            return None;
        }
        if !(self.options.indent_handlebars && c == '{' && self.input.peek(1) == Some('{')) {
            return None;
        }
        if self.input.peek(2) == Some('!') {
            let mut s = self.patterns.handlebars_comment.read(&mut self.input);
            if s.is_empty() {
                s = self.patterns.handlebars.read(&mut self.input);
            }
            Some(self.create(T::Comment, s))
        } else {
            let s = self.patterns.handlebars_open.read(&mut self.input);
            Some(self.create(T::TagOpen, s))
        }
    }

    fn read_control_flows(&mut self, c: char, open: Option<usize>) -> Option<Token> {
        if !self.options.base.templating.iter().any(|t| t == "angular") || !self.options.indent_handlebars {
            return None;
        }
        if c == '@' {
            let mut s = self.patterns.angular_control_flow_start.read(&mut self.input);
            if s.is_empty() {
                return None;
            }
            let mut opening = usize::from(s.ends_with('('));
            let mut closing = 0;
            while !(s.ends_with('{') && opening == closing) {
                let Some(n) = self.input.next() else { break };
                if n == '(' {
                    opening += 1;
                } else if n == ')' {
                    closing += 1;
                }
                s.push(n);
            }
            Some(self.create(T::ControlFlowOpen, s))
        } else if c == '}' && self.open_kind(open) == Some(T::ControlFlowOpen) {
            let s = self.input.next()?;
            Some(self.create(T::ControlFlowClose, s))
        } else {
            None
        }
    }

    fn read_close(&mut self, c: char, open: Option<usize>) -> Option<Token> {
        let o = open?;
        if self.tokens[o].kind != T::TagOpen {
            return None;
        }
        let first = self.tokens[o].text.chars().next();
        if first == Some('<') && (c == '>' || (c == '/' && self.input.peek(1) == Some('>'))) {
            let mut s = String::from(self.input.next()?);
            if c == '/' {
                s.push(self.input.next()?);
            }
            Some(self.create(T::TagClose, s))
        } else if first == Some('{') && c == '}' && self.input.peek(1) == Some('}') {
            self.input.next();
            self.input.next();
            Some(self.create(T::TagClose, "}}"))
        } else {
            None
        }
    }

    fn read_attribute(&mut self, c: char, previous: usize, open: Option<usize>) -> Option<Token> {
        let o = open?;
        if !self.tokens[o].text.starts_with('<') {
            return None;
        }
        if c == '=' {
            let s = self.input.next()?;
            return Some(self.create(T::Equals, s));
        }
        if c == '"' || c == '\'' {
            let mut content = String::from(self.input.next()?);
            if c == '"' {
                content.push_str(&self.patterns.double_quote.read(&mut self.input));
            } else {
                content.push_str(&self.patterns.single_quote.read(&mut self.input));
            }
            return Some(self.create(T::Value, content));
        }
        let s = self.patterns.attribute.read(&mut self.input);
        if s.is_empty() {
            return None;
        }
        let kind = if self.tokens[previous].kind == T::Equals { T::Value } else { T::Attribute };
        Some(self.create(kind, s))
    }

    fn is_content_unformatted(&self, tag: &str) -> bool {
        !self.options.void_elements.iter().any(|v| v == tag)
            && (self.options.content_unformatted.iter().any(|v| v == tag)
                || self.options.unformatted.iter().any(|v| v == tag))
    }

    fn read_raw_content(&mut self, c: char, previous: usize, open: Option<usize>) -> Option<Token> {
        let mut s = String::new();
        if open.is_some_and(|o| self.tokens[o].text.starts_with('{')) {
            s = self.patterns.handlebars_raw_close.read(&mut self.input);
        } else {
            let prev = &self.tokens[previous];
            if prev.kind == T::TagClose && !prev.text.starts_with('/') {
                if let Some(opened) = prev.opened.filter(|o| self.tokens[*o].text.starts_with('<')) {
                    let tag = self.tokens[opened].text[1..].to_lowercase();
                    if tag == "script" || tag == "style" {
                        if let Some(mut t) = self.read_comment_or_cdata(c) {
                            t.kind = T::Text;
                            return Some(t);
                        }
                        let end =
                            Re::with_flags(&format!("</{}[\\n\\r\\t ]*?>", super::core::escape(&tag)), "i");
                        s = self.input.read_until(&end, false);
                    } else if self.is_content_unformatted(&tag) {
                        let end =
                            Re::with_flags(&format!("</{}[\\n\\r\\t ]*?>", super::core::escape(&tag)), "i");
                        s = self.input.read_until(&end, false);
                    }
                }
            }
        }
        (!s.is_empty()).then(|| self.create(T::Text, s))
    }

    fn read_content_word(&mut self, c: char, open: Option<usize>) -> Option<Token> {
        let mut s = String::new();
        if let Some(p) = &self.patterns.unformatted_content_delimiter {
            if self.options.unformatted_content_delimiter.starts_with(c) {
                s = p.read(&mut self.input);
            }
        }
        if s.is_empty() {
            s = if self.open_kind(open) == Some(T::ControlFlowOpen) {
                self.patterns.word_control_flow_close_excluded.read(&mut self.input)
            } else {
                self.patterns.word.read(&mut self.input)
            };
        }
        (!s.is_empty()).then(|| self.create(T::Text, s))
    }
}

// ------------------------------------------------------------------ printer

struct Printer {
    indent_level: isize,
    alignment_size: usize,
    max_preserve_newlines: usize,
    preserve_newlines: bool,
    output: Output,
}

impl Printer {
    fn new(options: &HtmlOptions, base_indent: &str) -> Printer {
        Printer {
            indent_level: 0,
            alignment_size: 0,
            max_preserve_newlines: options.base.max_preserve_newlines,
            preserve_newlines: options.base.preserve_newlines,
            output: Output::new(&options.base, base_indent),
        }
    }

    fn set_space_before_token(&mut self, value: bool, non_breaking: bool) {
        self.output.space_before_token = value;
        self.output.non_breaking_space = non_breaking;
    }

    fn set_wrap_point(&mut self) {
        self.output.set_indent(self.indent_level, self.alignment_size);
        self.output.set_wrap_point();
    }

    fn add_raw_token(&mut self, t: &Token) {
        self.output.add_raw_token(t.newlines, &t.whitespace_before, &t.text);
    }

    fn print_preserved_newlines(&mut self, t: &Token, previous_kind: T) -> bool {
        let mut newlines = 0;
        if t.kind != T::Text && previous_kind != T::Text {
            newlines = usize::from(t.newlines > 0);
        }
        if self.preserve_newlines {
            newlines = if t.newlines < self.max_preserve_newlines + 1 {
                t.newlines
            } else {
                self.max_preserve_newlines + 1
            };
        }
        for n in 0..newlines {
            self.print_newline(n > 0);
        }
        newlines != 0
    }

    fn traverse_whitespace(&mut self, t: &Token, previous_kind: T) -> bool {
        if !t.whitespace_before.is_empty() || t.newlines > 0 {
            if !self.print_preserved_newlines(t, previous_kind) {
                self.output.space_before_token = true;
            }
            return true;
        }
        false
    }

    fn print_newline(&mut self, force: bool) {
        self.output.add_new_line(force);
    }

    fn print_token(&mut self, text: &str) {
        if !text.is_empty() {
            self.output.set_indent(self.indent_level, self.alignment_size);
            self.output.add_token(text);
        }
    }

    fn indent(&mut self) {
        self.indent_level += 1;
    }

    fn get_full_indent(&mut self, level: isize) -> String {
        let level = self.indent_level + level;
        if level < 1 {
            return String::new();
        }
        self.output.get_indent_string(level, 0)
    }
}

// ------------------------------------------------------------------ beautifier

#[derive(Clone, Debug, Default)]
struct TagOpen {
    parent: Option<usize>,
    text: String,
    tag_name: String,
    is_inline_element: bool,
    is_unformatted: bool,
    is_content_unformatted: bool,
    is_empty_element: bool,
    is_start_tag: bool,
    is_end_tag: bool,
    indent_content: bool,
    multiline_content: bool,
    custom_beautifier_name: Option<&'static str>,
    start_tag_token: Option<usize>,
    attr_count: usize,
    has_wrapped_attrs: bool,
    alignment_size: usize,
    tag_complete: bool,
    tag_start_char: Option<char>,
    tag_check: String,
}

struct Frame {
    parent: Option<usize>,
    tag: String,
    indent_level: isize,
    parser_token: Option<usize>,
}

/// The type and text of the last parser token.
#[derive(Clone)]
struct Last {
    kind: T,
}

pub(crate) struct Beautifier<'o> {
    source: String,
    options: &'o HtmlOptions,
    js: Option<SubBeautifier>,
    css: Option<SubBeautifier>,
    tags: Vec<TagOpen>,
    frames: Vec<Frame>,
    current_frame: Option<usize>,
}

fn php_adjustment(text: &str) -> Option<isize> {
    let rest = text.strip_prefix("<?php ")?.strip_suffix(" ?>")?;
    let digits = rest.strip_prefix('-').unwrap_or(rest);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

impl<'o> Beautifier<'o> {
    pub fn new(
        source: &str,
        options: &'o HtmlOptions,
        js: Option<SubBeautifier>,
        css: Option<SubBeautifier>,
    ) -> Beautifier<'o> {
        Beautifier {
            source: source.to_string(),
            options,
            js,
            css,
            tags: Vec::new(),
            frames: Vec::new(),
            current_frame: None,
        }
    }

    fn wrap_force(&self) -> bool {
        self.options.wrap_attributes.starts_with("force")
    }
    fn wrap_force_expand_multiline(&self) -> bool {
        self.options.wrap_attributes == "force-expand-multiline"
    }
    fn wrap_force_aligned(&self) -> bool {
        self.options.wrap_attributes == "force-aligned"
    }
    fn wrap_aligned_multiple(&self) -> bool {
        self.options.wrap_attributes == "aligned-multiple"
    }
    fn wrap_preserve(&self) -> bool {
        self.options.wrap_attributes.starts_with("preserve")
    }
    fn wrap_preserve_aligned(&self) -> bool {
        self.options.wrap_attributes == "preserve-aligned"
    }

    // tag stack
    fn get_parser_token(&self) -> Option<usize> {
        self.current_frame.and_then(|f| self.frames[f].parser_token)
    }

    fn record_tag(&mut self, token: usize, printer: &Printer) {
        self.frames.push(Frame {
            parent: self.current_frame,
            tag: self.tags[token].tag_name.clone(),
            indent_level: printer.indent_level,
            parser_token: Some(token),
        });
        self.current_frame = Some(self.frames.len() - 1);
    }

    fn get_frame(&self, tags: &[&str], stop: Option<&[&str]>) -> Option<usize> {
        let mut frame = self.current_frame;
        while let Some(f) = frame {
            let tag = self.frames[f].tag.as_str();
            if tags.contains(&tag) {
                break;
            } else if stop.is_some_and(|s| s.contains(&tag)) {
                return None;
            }
            frame = self.frames[f].parent;
        }
        frame
    }

    fn try_pop(&mut self, tag: &str, stop: Option<&[&str]>, printer: &mut Printer) -> Option<usize> {
        let frame = self.get_frame(&[tag], stop)?;
        printer.indent_level = self.frames[frame].indent_level;
        self.current_frame = self.frames[frame].parent;
        self.frames[frame].parser_token
    }

    fn indent_to_tag(&mut self, tags: &[&str], printer: &mut Printer) {
        if let Some(f) = self.get_frame(tags, None) {
            printer.indent_level = self.frames[f].indent_level;
        }
    }

    pub fn beautify(mut self) -> String {
        let mut eol = "\n".to_string();
        if let Some(i) = self.source.find(['\r', '\n']) {
            eol = if self.source[i..].starts_with("\r\n") {
                "\r\n".into()
            } else {
                self.source[i..i + 1].into()
            };
        }
        let source = self.source.replace("\r\n", "\n").replace('\r', "\n");
        let base_indent: String = source.chars().take_while(|c| *c == '\t' || *c == ' ').collect();
        let mut printer = Printer::new(self.options, &base_indent);
        let mut tokens = Tokenizer::new(&source, self.options).tokenize();
        let mut last = Last { kind: T::Start };
        self.tags.push(TagOpen { tag_complete: true, ..Default::default() });
        let mut last_tag = 0usize;
        let mut i = 1;
        while i < tokens.len() && tokens[i].kind != T::Eof {
            let kind = tokens[i].kind;
            let parser = if matches!(kind, T::TagOpen | T::Comment) {
                let t = self.handle_tag_open(&mut printer, &tokens, i, last_tag, &last);
                last_tag = t;
                Last { kind: T::TagOpen }
            } else if matches!(kind, T::Attribute | T::Equals | T::Value)
                || (kind == T::Text && !self.tags[last_tag].tag_complete)
            {
                self.handle_inside_tag(&mut printer, &tokens, i, last_tag, &last)
            } else if kind == T::TagClose {
                self.handle_tag_close(&mut printer, &tokens, i, last_tag)
            } else if kind == T::Text {
                self.handle_text(&mut printer, &mut tokens, i, last_tag)
            } else if kind == T::ControlFlowOpen {
                self.handle_control_flow_open(&mut printer, &tokens, i)
            } else if kind == T::ControlFlowClose {
                self.handle_control_flow_close(&mut printer, &tokens, i)
            } else {
                printer.add_raw_token(&tokens[i]);
                Last { kind }
            };
            last = parser;
            i += 1;
        }
        printer.output.get_code(&eol)
    }

    fn prev_kind(tokens: &[Token], i: usize) -> T {
        tokens[i].previous.map_or(T::Start, |p| tokens[p].kind)
    }

    fn handle_control_flow_open(&mut self, printer: &mut Printer, tokens: &[Token], i: usize) -> Last {
        let t = &tokens[i];
        printer.set_space_before_token(t.newlines > 0 || !t.whitespace_before.is_empty(), true);
        if t.newlines > 0 {
            printer.print_preserved_newlines(t, Self::prev_kind(tokens, i));
        } else {
            printer.set_space_before_token(t.newlines > 0 || !t.whitespace_before.is_empty(), true);
        }
        printer.print_token(&t.text);
        printer.indent();
        Last { kind: t.kind }
    }

    fn handle_control_flow_close(&mut self, printer: &mut Printer, tokens: &[Token], i: usize) -> Last {
        let t = &tokens[i];
        if printer.indent_level > 0 {
            printer.indent_level -= 1;
            printer.output.set_indent(printer.indent_level, printer.alignment_size);
        }
        if t.newlines > 0 {
            printer.print_preserved_newlines(t, Self::prev_kind(tokens, i));
        } else {
            printer.set_space_before_token(t.newlines > 0 || !t.whitespace_before.is_empty(), true);
        }
        printer.print_token(&t.text);
        Last { kind: t.kind }
    }

    fn handle_tag_close(
        &mut self,
        printer: &mut Printer,
        tokens: &[Token],
        i: usize,
        last_tag: usize,
    ) -> Last {
        let t = &tokens[i];
        printer.alignment_size = 0;
        self.tags[last_tag].tag_complete = true;
        printer.set_space_before_token(t.newlines > 0 || !t.whitespace_before.is_empty(), true);
        let lt = self.tags[last_tag].clone();
        if lt.is_unformatted {
            printer.add_raw_token(t);
        } else {
            if lt.tag_start_char == Some('<') {
                printer.set_space_before_token(t.text.starts_with('/'), true);
                if self.wrap_force_expand_multiline() && lt.has_wrapped_attrs {
                    printer.print_newline(false);
                }
            }
            printer.print_token(&t.text);
        }
        if lt.indent_content && !(lt.is_unformatted || lt.is_content_unformatted) {
            printer.indent();
            self.tags[last_tag].indent_content = false;
        }
        if !lt.is_inline_element && !(lt.is_unformatted || lt.is_content_unformatted) {
            printer.set_wrap_point();
        }
        Last { kind: t.kind }
    }

    fn handle_inside_tag(
        &mut self,
        printer: &mut Printer,
        tokens: &[Token],
        i: usize,
        last_tag: usize,
        last: &Last,
    ) -> Last {
        let t = &tokens[i];
        let mut wrapped = self.tags[last_tag].has_wrapped_attrs;
        printer.set_space_before_token(t.newlines > 0 || !t.whitespace_before.is_empty(), true);
        let lt = self.tags[last_tag].clone();
        if lt.is_unformatted {
            printer.add_raw_token(t);
        } else if lt.tag_start_char == Some('{') && t.kind == T::Text {
            if printer.print_preserved_newlines(t, Self::prev_kind(tokens, i)) {
                let mut t2 = t.clone();
                t2.newlines = 0;
                printer.add_raw_token(&t2);
            } else {
                printer.print_token(&t.text);
            }
        } else {
            if t.kind == T::Attribute {
                printer.set_space_before_token(true, false);
            } else if t.kind == T::Equals || (t.kind == T::Value && Self::prev_kind(tokens, i) == T::Equals) {
                printer.set_space_before_token(false, false);
            }
            if t.kind == T::Attribute && lt.tag_start_char == Some('<') {
                if self.wrap_preserve() || self.wrap_preserve_aligned() {
                    printer.traverse_whitespace(t, Self::prev_kind(tokens, i));
                    wrapped = wrapped || t.newlines != 0;
                }
                if self.wrap_force()
                    && lt.attr_count >= self.options.wrap_attributes_min_attrs
                    && (last.kind != T::TagOpen || self.wrap_force_expand_multiline())
                {
                    printer.print_newline(false);
                    wrapped = true;
                }
            }
            printer.print_token(&t.text);
            wrapped = wrapped || printer.output.previous_token_wrapped;
            self.tags[last_tag].has_wrapped_attrs = wrapped;
        }
        Last { kind: t.kind }
    }

    fn handle_text(
        &mut self,
        printer: &mut Printer,
        tokens: &mut [Token],
        i: usize,
        last_tag: usize,
    ) -> Last {
        let lt = self.tags[last_tag].clone();
        if lt.custom_beautifier_name.is_some() {
            self.print_custom_beautifier_text(printer, tokens, i, &lt);
        } else if lt.is_unformatted || lt.is_content_unformatted {
            printer.add_raw_token(&tokens[i]);
        } else {
            let prev = Self::prev_kind(tokens, i);
            let t = &tokens[i];
            printer.traverse_whitespace(t, prev);
            // Intelephense: `<?php N ?>` opens / closes PHP blocks
            let adjustment = php_adjustment(&t.text);
            if let Some(n) = adjustment.filter(|n| *n < 1) {
                let n = if n == 0 { -1 } else { n };
                printer.indent_level = (printer.indent_level + n).max(0);
            }
            printer.print_token(&t.text);
            if let Some(n) = adjustment.filter(|n| *n > -1) {
                let n = if n == 0 { 1 } else { n };
                printer.indent_level += n;
            }
        }
        Last { kind: T::Content }
    }

    fn print_custom_beautifier_text(
        &mut self,
        printer: &mut Printer,
        tokens: &mut [Token],
        i: usize,
        lt: &TagOpen,
    ) {
        if tokens[i].text.is_empty() {
            return;
        }
        let name = lt.custom_beautifier_name.unwrap_or("");
        let beautifier: Option<Formatter<'_>> = match name {
            "javascript" => self.js.map(|f| {
                let child = self.options.child();
                Box::new(move |s: &str| f(s, &child)) as Box<dyn Fn(&str) -> String>
            }),
            "css" => self.css.map(|f| {
                let child = self.options.child();
                Box::new(move |s: &str| f(s, &child)) as Box<dyn Fn(&str) -> String>
            }),
            "html" => {
                let (opts, js, css) = (self.options, self.js, self.css);
                Some(Box::new(move |s: &str| Beautifier::new(s, opts, js, css).beautify()))
            }
            _ => None,
        };
        let script_indent_level: isize = match self.options.indent_scripts.as_str() {
            "keep" => 0,
            "separate" => -printer.indent_level,
            _ => 1,
        };
        let indentation = printer.get_full_indent(script_indent_level);
        let mut text = strip_trailing_blank_line(&tokens[i].text);
        let mut pre = String::new();
        let mut post = String::new();
        if name != "html" && (text.starts_with("<!--") || text.starts_with("<![CDATA[")) {
            let re = Re::new(r"(?s)^(<!--[^\n]*|<!\[CDATA\[)(\n?)([ \t\n]*)(.*)(-->|\]\]>)$");
            let Some(m) = re.captures(&text) else {
                printer.add_raw_token(&tokens[i]);
                return;
            };
            let (g1, g2, g3, g4, g5) = (
                m[1].to_string(),
                m[2].to_string(),
                m[3].to_string(),
                m[4].to_string(),
                m.get(5).map_or(String::new(), |g| g.as_str().to_string()),
            );
            pre = format!("{indentation}{g1}\n");
            text = g4;
            if !g5.is_empty() {
                post = format!("{indentation}{g5}");
            }
            text = strip_trailing_blank_line(&text);
            if !g2.is_empty() || g3.contains('\n') {
                let ws: String = g3.chars().rev().take_while(|c| *c == ' ' || *c == '\t').collect();
                if !ws.is_empty() {
                    tokens[i].whitespace_before = ws.chars().rev().collect();
                }
            }
        }
        if !text.is_empty() {
            if let Some(b) = &beautifier {
                text = b(&format!("{indentation}{text}"));
            } else {
                let white = tokens[i].whitespace_before.clone();
                if !white.is_empty() {
                    text = text.replace(&format!("\n{white}"), "\n");
                }
                text = format!("{indentation}{}", text.replace('\n', &format!("\n{indentation}")));
            }
        }
        if !pre.is_empty() {
            text = if text.is_empty() { format!("{pre}{post}") } else { format!("{pre}{text}\n{post}") };
        }
        printer.print_newline(false);
        if !text.is_empty() {
            let t = &mut tokens[i];
            t.text = text;
            t.whitespace_before.clear();
            t.newlines = 0;
            printer.add_raw_token(t);
            printer.print_newline(true);
        }
    }

    fn handle_tag_open(
        &mut self,
        printer: &mut Printer,
        tokens: &[Token],
        i: usize,
        last_tag: usize,
        last: &Last,
    ) -> usize {
        let pt = self.get_tag_open_token(tokens, i);
        let lt = self.tags[last_tag].clone();
        let raw = &tokens[i];
        if (lt.is_unformatted || lt.is_content_unformatted)
            && !lt.is_empty_element
            && raw.kind == T::TagOpen
            && !self.tags[pt].is_start_tag
        {
            printer.add_raw_token(raw);
            let name = self.tags[pt].tag_name.clone();
            self.tags[pt].start_tag_token = self.try_pop(&name, None, printer);
        } else {
            printer.traverse_whitespace(raw, Self::prev_kind(tokens, i));
            self.set_tag_position(printer, tokens, i, pt, last_tag, last);
            if !self.tags[pt].is_inline_element {
                printer.set_wrap_point();
            }
            printer.print_token(&raw.text);
        }
        if self.tags[pt].is_start_tag && self.wrap_force() {
            let mut j = i + 1;
            while j < tokens.len() {
                let k = tokens[j].kind;
                if k == T::Attribute {
                    self.tags[pt].attr_count += 1;
                }
                if k == T::Eof || k == T::TagClose {
                    break;
                }
                j += 1;
            }
        }
        if self.wrap_force_aligned() || self.wrap_aligned_multiple() || self.wrap_preserve_aligned() {
            self.tags[pt].alignment_size = super::core::js_len(&raw.text) + 1;
        }
        if !self.tags[pt].tag_complete && !self.tags[pt].is_unformatted {
            printer.alignment_size = self.tags[pt].alignment_size;
        }
        pt
    }

    fn get_tag_open_token(&mut self, tokens: &[Token], i: usize) -> usize {
        let raw = &tokens[i];
        let mut t = TagOpen { parent: self.get_parser_token(), ..Default::default() };
        t.tag_start_char = raw.text.chars().next();
        t.text = raw.text.clone();
        if t.tag_start_char == Some('<') {
            t.tag_check = raw.text[1..].chars().take_while(|c| !c.is_whitespace() && *c != '>').collect();
        } else {
            // `{{~?(?:[\^]|#\*?)?([^\s}]+)`
            let mut s = raw.text.strip_prefix("{{").unwrap_or("");
            s = s.strip_prefix('~').unwrap_or(s);
            if let Some(r) = s.strip_prefix('^') {
                s = r;
            } else if let Some(r) = s.strip_prefix('#') {
                s = r.strip_prefix('*').unwrap_or(r);
            }
            t.tag_check = s.chars().take_while(|c| !c.is_whitespace() && *c != '}').collect();
            if (raw.text.starts_with("{{#>") || raw.text.starts_with("{{~#>")) && t.tag_check.starts_with('>')
            {
                if t.tag_check == ">" && raw.next.is_some() {
                    let next = &tokens[raw.next.unwrap_or(i)];
                    t.tag_check = next.text.split(' ').next().unwrap_or("").to_string();
                } else {
                    t.tag_check = raw.text.split('>').nth(1).unwrap_or("").to_string();
                }
            }
        }
        t.tag_check = t.tag_check.to_lowercase();
        if raw.kind == T::Comment {
            t.tag_complete = true;
        }
        t.is_start_tag = !t.tag_check.starts_with('/');
        t.tag_name = if t.is_start_tag { t.tag_check.clone() } else { t.tag_check[1..].to_string() };
        t.is_end_tag = !t.is_start_tag || raw.closed.is_some_and(|c| tokens[c].text == "/>");
        let handlebar_starts =
            if t.tag_start_char == Some('{') && t.text.len() >= 3 && t.text.as_bytes()[2] == b'~' {
                3
            } else {
                2
            };
        t.is_end_tag = t.is_end_tag
            || (t.tag_start_char == Some('{')
                && (t.text.chars().count() < 3
                    || t.text.chars().nth(handlebar_starts).is_some_and(|c| c != '#' && c != '^')));
        // `_get_tag_open_token`
        t.alignment_size = self.options.wrap_attributes_indent_size;
        t.is_end_tag = t.is_end_tag || self.options.void_elements.contains(&t.tag_check);
        t.is_empty_element = t.tag_complete || (t.is_start_tag && t.is_end_tag);
        t.is_unformatted = !t.tag_complete && self.options.unformatted.contains(&t.tag_check);
        t.is_content_unformatted =
            !t.is_empty_element && self.options.content_unformatted.contains(&t.tag_check);
        t.is_inline_element = self.options.inline.contains(&t.tag_name)
            || (self.options.inline_custom_elements && t.tag_name.contains('-'))
            || t.tag_start_char == Some('{');
        self.tags.push(t);
        self.tags.len() - 1
    }

    fn set_tag_position(
        &mut self,
        printer: &mut Printer,
        tokens: &[Token],
        i: usize,
        pt: usize,
        last_tag: usize,
        last: &Last,
    ) {
        let raw = &tokens[i];
        if !self.tags[pt].is_empty_element {
            if self.tags[pt].is_end_tag {
                let name = self.tags[pt].tag_name.clone();
                self.tags[pt].start_tag_token = self.try_pop(&name, None, printer);
            } else {
                if self.do_optional_end_element(pt, printer) && !self.tags[pt].is_inline_element {
                    printer.print_newline(false);
                }
                self.record_tag(pt, printer);
                let t = &self.tags[pt];
                if (t.tag_name == "script" || t.tag_name == "style")
                    && !(t.is_unformatted || t.is_content_unformatted)
                {
                    let check = t.tag_check.clone();
                    self.tags[pt].custom_beautifier_name = custom_beautifier_name(&check, tokens, i);
                }
            }
        }
        if self.options.extra_liners.contains(&self.tags[pt].tag_check) {
            printer.print_newline(false);
            if !printer.output.just_added_blankline() {
                printer.print_newline(true);
            }
        }
        let t = self.tags[pt].clone();
        let lt = self.tags[last_tag].clone();
        if t.is_empty_element {
            if t.tag_start_char == Some('{') && t.tag_check == "else" {
                self.indent_to_tag(&["if", "unless", "each"], printer);
                self.tags[pt].indent_content = true;
                if !printer.output.current_line_has_match(&Re::new(r"\{\{#if")) {
                    printer.print_newline(false);
                }
            }
            if t.tag_name == "!--" && last.kind == T::TagClose && lt.is_end_tag && !t.text.contains('\n') {
                // comments stay on the line of the preceding end tag
            } else {
                if !(t.is_inline_element || t.is_unformatted) {
                    printer.print_newline(false);
                }
                self.calculate_parent_multiline(printer, pt);
            }
        } else if t.is_end_tag {
            let mut expand = t.start_tag_token.is_some_and(|s| self.tags[s].multiline_content);
            expand = expand
                || (!t.is_inline_element
                    && !(lt.is_inline_element || lt.is_unformatted)
                    && !(last.kind == T::TagClose && t.start_tag_token == Some(last_tag))
                    && last.kind != T::Content);
            if t.is_content_unformatted || t.is_unformatted {
                expand = false;
            }
            if expand {
                printer.print_newline(false);
            }
        } else {
            let mut indent_content = t.custom_beautifier_name.is_none();
            if t.tag_start_char == Some('<') {
                match t.tag_name.as_str() {
                    "html" => indent_content = self.options.indent_inner_html,
                    "head" => indent_content = self.options.indent_head_inner_html,
                    "body" => indent_content = self.options.indent_body_inner_html,
                    _ => {}
                }
            }
            self.tags[pt].indent_content = indent_content;
            if !(t.is_inline_element || t.is_unformatted)
                && (last.kind != T::Content || t.is_content_unformatted)
            {
                printer.print_newline(false);
            }
            self.calculate_parent_multiline(printer, pt);
        }
        let _ = raw;
    }

    fn calculate_parent_multiline(&mut self, printer: &Printer, pt: usize) {
        let t = &self.tags[pt];
        if let Some(parent) = t.parent {
            if printer.output.just_added_newline()
                && !((t.is_inline_element || t.is_unformatted) && self.tags[parent].is_inline_element)
            {
                self.tags[parent].multiline_content = true;
            }
        }
    }

    fn do_optional_end_element(&mut self, pt: usize, printer: &mut Printer) -> bool {
        let t = self.tags[pt].clone();
        let Some(parent) = t.parent else { return false };
        if t.is_empty_element || !t.is_start_tag {
            return false;
        }
        const P_CLOSERS: &[&str] = &[
            "address",
            "article",
            "aside",
            "blockquote",
            "details",
            "div",
            "dl",
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
            "header",
            "hr",
            "main",
            "menu",
            "nav",
            "ol",
            "p",
            "pre",
            "section",
            "table",
            "ul",
        ];
        const P_PARENT_EXCLUDES: &[&str] = &["a", "audio", "del", "ins", "map", "noscript", "video"];
        let mut result: Option<usize> = None;
        let name = t.tag_name.as_str();
        let mut pop = |s: &mut Self, tag: &str, stop: Option<&[&str]>, result: &mut Option<usize>| {
            if result.is_none() {
                *result = s.try_pop(tag, stop, printer);
            }
        };
        match name {
            "body" => pop(self, "head", None, &mut result),
            "li" => pop(self, "li", Some(&["ol", "ul", "menu"]), &mut result),
            "dd" | "dt" => {
                pop(self, "dt", Some(&["dl"]), &mut result);
                pop(self, "dd", Some(&["dl"]), &mut result);
            }
            _ if self.tags[parent].tag_name == "p" && P_CLOSERS.contains(&name) => {
                let p_parent = self.tags[parent].parent;
                if p_parent.is_none_or(|pp| !P_PARENT_EXCLUDES.contains(&self.tags[pp].tag_name.as_str())) {
                    pop(self, "p", None, &mut result);
                }
            }
            "rp" | "rt" => {
                pop(self, "rt", Some(&["ruby", "rtc"]), &mut result);
                pop(self, "rp", Some(&["ruby", "rtc"]), &mut result);
            }
            "optgroup" => pop(self, "optgroup", Some(&["select"]), &mut result),
            "option" => pop(self, "option", Some(&["select", "datalist", "optgroup"]), &mut result),
            "colgroup" => pop(self, "caption", Some(&["table"]), &mut result),
            "thead" => {
                pop(self, "caption", Some(&["table"]), &mut result);
                pop(self, "colgroup", Some(&["table"]), &mut result);
            }
            "tbody" | "tfoot" => {
                pop(self, "caption", Some(&["table"]), &mut result);
                pop(self, "colgroup", Some(&["table"]), &mut result);
                pop(self, "thead", Some(&["table"]), &mut result);
                pop(self, "tbody", Some(&["table"]), &mut result);
            }
            "tr" => {
                pop(self, "caption", Some(&["table"]), &mut result);
                pop(self, "colgroup", Some(&["table"]), &mut result);
                pop(self, "tr", Some(&["table", "thead", "tbody", "tfoot"]), &mut result);
            }
            "th" | "td" => {
                pop(self, "td", Some(&["table", "thead", "tbody", "tfoot", "tr"]), &mut result);
                pop(self, "th", Some(&["table", "thead", "tbody", "tfoot", "tr"]), &mut result);
            }
            _ => {}
        }
        self.tags[pt].parent = self.get_parser_token();
        result.is_some()
    }
}

/// `text.replace(/\n[ \t]*$/, '')`
fn strip_trailing_blank_line(text: &str) -> String {
    let trimmed = text.trim_end_matches([' ', '\t']);
    match trimmed.strip_suffix('\n') {
        Some(t) => t.to_string(),
        None => text.to_string(),
    }
}

fn type_attribute(tokens: &[Token], start: usize) -> Option<String> {
    let mut j = tokens[start].next?;
    while tokens[j].kind != T::Eof && tokens[start].closed != Some(j) {
        if tokens[j].kind == T::Attribute && tokens[j].text == "type" {
            let eq = tokens[j].next?;
            if tokens[eq].kind == T::Equals {
                let v = tokens[eq].next?;
                if tokens[v].kind == T::Value {
                    return Some(tokens[v].text.clone());
                }
            }
            return None;
        }
        j = tokens[j].next?;
    }
    None
}

fn custom_beautifier_name(tag_check: &str, tokens: &[Token], i: usize) -> Option<&'static str> {
    tokens[i].closed?;
    let default = match tag_check {
        "script" => "text/javascript",
        "style" => "text/css",
        _ => "",
    };
    let ty = type_attribute(tokens, i).unwrap_or_else(|| default.to_string());
    if ty.contains("text/css") {
        return Some("css");
    }
    let js = Re::new(
        r"module|((text|application|dojo)/(x-)?(javascript|ecmascript|jscript|livescript|(ld\+)?json|method|aspect))",
    );
    if js.is_match(&ty) {
        return Some("javascript");
    }
    if Re::new(r"(text|application|dojo)/(x-)?(html)").is_match(&ty) {
        return Some("html");
    }
    if Re::new(r"test/null").is_match(&ty) {
        return Some("null");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(s: &str) -> String {
        let o = HtmlOptions::intelephense(4, false);
        Beautifier::new(s, &o, None, None).beautify()
    }

    #[test]
    fn indents_blocks() {
        assert_eq!(html("<div>\n<p>Hello</p>\n</div>\n"), "<div>\n    <p>Hello</p>\n</div>");
        assert_eq!(
            html("<html>\n<head><title>x</title></head>\n<body>\n<div class=\"a\"\nid=\"b\">text  with   spaces</div>\n</body>\n</html>"),
            "<html>\n\n<head>\n    <title>x</title>\n</head>\n\n<body>\n    <div class=\"a\"\n        id=\"b\">text with spaces</div>\n</body>\n\n</html>"
        );
    }

    #[test]
    fn php_placeholders_indent() {
        assert_eq!(
            html("<div>\n<?php 1 ?>\n<span>x</span>\n<?php 0 ?>\n<b>y</b>\n<?php -1 ?>\n</div>"),
            "<div>\n    <?php 1 ?>\n        <span>x</span>\n    <?php 0 ?>\n        <b>y</b>\n    <?php -1 ?>\n</div>"
        );
    }
}
