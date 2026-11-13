//! The core of js-beautify (`js/src/core`): input scanner, patterns,
//! tokens and the output buffer. Ported closely so the formatters built on
//! it produce exactly js-beautify's output.
//!
//! js-beautify is MIT licensed, Copyright (c) 2007-2018 Einar Lielmanis,
//! Liam Newman, and contributors.

use regex::Regex;

/// A regular expression usable both anchored at a position (JavaScript's
/// sticky flag) and as a search from a position (global flag).
#[derive(Clone, Debug)]
pub(crate) struct Re {
    pub source: String,
    anchored: Regex,
    search: Regex,
}

impl Re {
    pub fn new(source: &str) -> Re {
        Re::with_flags(source, "")
    }

    /// `flags`: `i` for case insensitive.
    pub fn with_flags(source: &str, flags: &str) -> Re {
        let prefix = if flags.contains('i') { "(?i)" } else { "" };
        Re {
            source: source.to_string(),
            anchored: Regex::new(&format!("{prefix}^(?:{source})")).expect("valid pattern"),
            search: Regex::new(&format!("{prefix}(?:{source})")).expect("valid pattern"),
        }
    }

    /// The match starting exactly at `pos`.
    pub fn match_at<'t>(&self, text: &'t str, pos: usize) -> Option<&'t str> {
        self.anchored.find(&text[pos..]).map(|m| m.as_str())
    }

    /// The first match at or after `pos`: (start, end).
    pub fn find_from(&self, text: &str, pos: usize) -> Option<(usize, usize)> {
        self.search.find_at(text, pos).map(|m| (m.start(), m.end()))
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.search.is_match(text)
    }

    pub fn captures<'t>(&self, text: &'t str) -> Option<regex::Captures<'t>> {
        self.search.captures(text)
    }
}

/// Escapes `literal` for use in a pattern.
pub(crate) fn escape(literal: &str) -> String {
    regex::escape(literal)
}

/// Length in UTF-16 code units, as JavaScript counts string lengths.
pub(crate) fn js_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

pub(crate) struct InputScanner {
    input: String,
    position: usize,
}

impl InputScanner {
    pub fn new(input: &str) -> InputScanner {
        InputScanner { input: input.to_string(), position: 0 }
    }

    pub fn text(&self) -> &str {
        &self.input
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn set_position(&mut self, pos: usize) {
        self.position = pos;
    }

    pub fn restart(&mut self) {
        self.position = 0;
    }

    pub fn back(&mut self) {
        if self.position > 0 {
            let c = self.input[..self.position].chars().next_back().map_or(1, char::len_utf8);
            self.position -= c;
        }
    }

    pub fn has_next(&self) -> bool {
        self.position < self.input.len()
    }

    pub fn next(&mut self) -> Option<char> {
        let c = self.input[self.position..].chars().next()?;
        self.position += c.len_utf8();
        Some(c)
    }

    pub fn peek(&self, index: isize) -> Option<char> {
        if index >= 0 {
            self.input[self.position..].chars().nth(index as usize)
        } else {
            self.input[..self.position].chars().rev().nth((-index - 1) as usize)
        }
    }

    pub fn peek0(&self) -> Option<char> {
        self.peek(0)
    }

    fn offset(&self, index: isize) -> Option<usize> {
        if index >= 0 {
            let mut it = self.input[self.position..].char_indices();
            match it.nth(index as usize) {
                Some((i, _)) => Some(self.position + i),
                None => Some(self.input.len()),
            }
        } else {
            let mut p = self.position;
            for _ in 0..(-index) {
                let c = self.input[..p].chars().next_back()?;
                p -= c.len_utf8();
            }
            Some(p)
        }
    }

    /// Matches `pattern` at the position and advances past it.
    pub fn match_re(&mut self, pattern: &Re) -> Option<String> {
        let m = pattern.match_at(&self.input, self.position)?.to_string();
        self.position += m.len();
        Some(m)
    }

    pub fn read(&mut self, starting: Option<&Re>, until: Option<&Re>, until_after: bool) -> String {
        let mut val = String::new();
        let mut matched = false;
        if let Some(s) = starting {
            if let Some(m) = self.match_re(s) {
                val.push_str(&m);
                matched = true;
            }
        }
        if let Some(u) = until {
            if matched || starting.is_none() {
                val.push_str(&self.read_until(u, until_after));
            }
        }
        val
    }

    pub fn read_until(&mut self, pattern: &Re, until_after: bool) -> String {
        let match_index = match pattern.find_from(&self.input, self.position) {
            Some((start, end)) => {
                if until_after {
                    end
                } else {
                    start
                }
            }
            None => self.input.len(),
        };
        let val = self.input[self.position..match_index].to_string();
        self.position = match_index;
        val
    }

    pub fn read_until_after(&mut self, pattern: &Re) -> String {
        self.read_until(pattern, true)
    }

    pub fn peek_until_after(&mut self, pattern: &Re) -> String {
        let start = self.position;
        let val = self.read_until_after(pattern);
        self.position = start;
        val
    }

    /// Whether the (lower case) text before the last read character ends
    /// with `test`.
    pub fn look_back(&self, test: &str) -> bool {
        let Some(start) = self.offset(-1) else { return false };
        start >= test.len()
            && self.input.is_char_boundary(start - test.len())
            && self.input[start - test.len()..start].to_lowercase() == test
    }
}

/// js-beautify's `Pattern`: read an optional start, then a match or up to
/// an end pattern.
#[derive(Clone, Debug, Default)]
pub(crate) struct Pattern {
    pub starting: Option<Re>,
    pub matching: Option<Re>,
    pub until: Option<Re>,
    pub until_after: bool,
}

impl Pattern {
    pub fn read(&self, input: &mut InputScanner) -> String {
        let mut result = input.read(self.starting.as_ref(), None, false);
        if self.starting.is_none() || !result.is_empty() {
            result.push_str(&input.read(self.matching.as_ref(), self.until.as_ref(), self.until_after));
        }
        result
    }

    pub fn until_after(&self, pattern: &str) -> Pattern {
        Pattern { until: Some(Re::new(pattern)), until_after: true, ..self.clone() }
    }

    pub fn until(&self, pattern: &str) -> Pattern {
        Pattern { until: Some(Re::new(pattern)), until_after: false, ..self.clone() }
    }

    pub fn starting_with(&self, pattern: &str) -> Pattern {
        Pattern { starting: Some(Re::new(pattern)), ..self.clone() }
    }

    pub fn matching(&self, pattern: &str) -> Pattern {
        Pattern { matching: Some(Re::new(pattern)), ..self.clone() }
    }
}

/// Templating languages recognized inside other text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Languages {
    pub django: bool,
    pub erb: bool,
    pub handlebars: bool,
    pub php: bool,
    pub smarty: bool,
    pub angular: bool,
}

impl Languages {
    pub fn all(value: bool) -> Languages {
        Languages { django: value, erb: value, handlebars: value, php: value, smarty: value, angular: value }
    }

    pub fn set(&mut self, name: &str, value: bool) {
        match name {
            "django" => self.django = value,
            "erb" => self.erb = value,
            "handlebars" => self.handlebars = value,
            "php" => self.php = value,
            "smarty" => self.smarty = value,
            "angular" => self.angular = value,
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
struct TemplatePatterns {
    handlebars_comment: Pattern,
    handlebars_unescaped: Pattern,
    handlebars: Pattern,
    php: Pattern,
    erb: Pattern,
    django: Pattern,
    django_value: Pattern,
    django_comment: Pattern,
    smarty_comment: Pattern,
    smarty_literal: Pattern,
}

const PHP_START: &str = r"<\?(?:[= ]|php)";
const HANDLEBARS_START: &str = r"\{\{";
const ERB_START: &str = r"<%[^%]";
const DJANGO_START: &str = r"\{%";
const DJANGO_VALUE_START: &str = r"\{\{";
const DJANGO_COMMENT_START: &str = r"\{#";

impl TemplatePatterns {
    fn new() -> TemplatePatterns {
        let p = Pattern::default();
        TemplatePatterns {
            handlebars_comment: p.starting_with(r"\{\{!--").until_after(r"--\}\}"),
            handlebars_unescaped: p.starting_with(r"\{\{\{").until_after(r"\}\}\}"),
            handlebars: p.starting_with(HANDLEBARS_START).until_after(r"\}\}"),
            php: p.starting_with(PHP_START).until_after(r"\?>"),
            erb: p.starting_with(ERB_START).until_after(r"[^%]%>"),
            django: p.starting_with(DJANGO_START).until_after(r"%\}"),
            django_value: p.starting_with(DJANGO_VALUE_START).until_after(r"\}\}"),
            django_comment: p.starting_with(DJANGO_COMMENT_START).until_after(r"#\}"),
            smarty_comment: p.starting_with(r"\{\*").until_after(r"\*\}"),
            smarty_literal: p.starting_with(r"\{literal\}").until_after(r"\{/literal\}"),
        }
    }
}

/// js-beautify's `TemplatablePattern`: a pattern that skips over embedded
/// template tags (`<?php ... ?>`, `{{ ... }}`, ...).
#[derive(Clone, Debug)]
pub(crate) struct TemplatablePattern {
    base: Pattern,
    disabled: Languages,
    excluded: Languages,
    template: Option<Re>,
    patterns: std::rc::Rc<TemplatePatterns>,
}

impl TemplatablePattern {
    pub fn new() -> TemplatablePattern {
        let mut t = TemplatablePattern {
            base: Pattern::default(),
            disabled: Languages::default(),
            excluded: Languages::default(),
            template: None,
            patterns: std::rc::Rc::new(TemplatePatterns::new()),
        };
        t.update();
        t
    }

    fn update(&mut self) {
        let mut items: Vec<&str> = Vec::new();
        if !self.disabled.php {
            items.push(PHP_START);
        }
        if !self.disabled.handlebars {
            items.push(HANDLEBARS_START);
        }
        if !self.disabled.erb {
            items.push(ERB_START);
        }
        if !self.disabled.django {
            items.push(DJANGO_START);
            items.push(DJANGO_VALUE_START);
            items.push(DJANGO_COMMENT_START);
        }
        if !self.disabled.smarty {
            items.push(r"\{[^}{\s\n]");
        }
        let until = self.base.until.as_ref().map(|u| u.source.clone());
        if let Some(u) = &until {
            items.push(u);
        }
        self.template = Some(Re::new(&items.join("|")));
    }

    fn with(&self, f: impl FnOnce(&mut TemplatablePattern)) -> TemplatablePattern {
        let mut t = self.clone();
        f(&mut t);
        t.update();
        t
    }

    pub fn read_options(&self, templating: &[String]) -> TemplatablePattern {
        self.with(|t| {
            t.disabled = Languages::all(true);
            for l in templating {
                t.disabled.set(l, false);
            }
        })
    }

    pub fn exclude(&self, language: &str) -> TemplatablePattern {
        self.with(|t| t.excluded.set(language, true))
    }

    pub fn until(&self, pattern: &str) -> TemplatablePattern {
        self.with(|t| t.base = t.base.until(pattern))
    }

    pub fn until_after(&self, pattern: &str) -> TemplatablePattern {
        self.with(|t| t.base = t.base.until_after(pattern))
    }

    pub fn starting_with(&self, pattern: &str) -> TemplatablePattern {
        self.with(|t| t.base = t.base.starting_with(pattern))
    }

    pub fn matching(&self, pattern: &str) -> TemplatablePattern {
        self.with(|t| t.base = t.base.matching(pattern))
    }

    pub fn read(&self, input: &mut InputScanner) -> String {
        let template = self.template.as_ref();
        let mut result = if self.base.matching.is_some() {
            input.read(self.base.starting.as_ref(), None, false)
        } else {
            input.read(self.base.starting.as_ref(), template, false)
        };
        let mut next = self.read_template(input);
        while !next.is_empty() {
            if let Some(m) = &self.base.matching {
                next.push_str(&input.read(Some(m), None, false));
            } else if let Some(t) = template {
                next.push_str(&input.read_until(t, false));
            }
            result.push_str(&next);
            next = self.read_template(input);
        }
        if self.base.until_after {
            if let Some(u) = &self.base.until {
                result.push_str(&input.read_until_after(u));
            }
        }
        result
    }

    fn read_template(&self, input: &mut InputScanner) -> String {
        let mut s = String::new();
        let p = &self.patterns;
        match input.peek0() {
            Some('<') => {
                let peek1 = input.peek(1);
                if !self.disabled.php && !self.excluded.php && peek1 == Some('?') {
                    s = p.php.read(input);
                }
                if s.is_empty() && !self.disabled.erb && !self.excluded.erb && peek1 == Some('%') {
                    s = p.erb.read(input);
                }
            }
            Some('{') => {
                if !self.disabled.handlebars && !self.excluded.handlebars {
                    s = p.handlebars_comment.read(input);
                    if s.is_empty() {
                        s = p.handlebars_unescaped.read(input);
                    }
                    if s.is_empty() {
                        s = p.handlebars.read(input);
                    }
                }
                if !self.disabled.django {
                    if s.is_empty() && !self.excluded.django && !self.excluded.handlebars {
                        s = p.django_value.read(input);
                    }
                    if !self.excluded.django {
                        if s.is_empty() {
                            s = p.django_comment.read(input);
                        }
                        if s.is_empty() {
                            s = p.django.read(input);
                        }
                    }
                }
                if !self.disabled.smarty && self.disabled.django && self.disabled.handlebars {
                    if s.is_empty() {
                        s = p.smarty_comment.read(input);
                    }
                    if s.is_empty() {
                        s = p.smarty_literal.read(input);
                    }
                    if s.is_empty() {
                        s = smarty(input);
                    }
                }
            }
            _ => {}
        }
        s
    }
}

/// `{(?=[^}{\s\n])` ... `[^\s\n]}` (the lookahead is checked by hand).
fn smarty(input: &mut InputScanner) -> String {
    if input.peek0() != Some('{') || input.peek(1).is_none_or(|c| c == '}' || c == '{' || c.is_whitespace()) {
        return String::new();
    }
    input.next();
    let mut s = String::from("{");
    s.push_str(&input.read_until_after(&Re::new(r"[^\s\n]\}")));
    s
}

/// js-beautify's `WhitespacePattern`.
#[derive(Clone, Debug)]
pub(crate) struct WhitespacePattern {
    chars: String,
    newlines: String,
    pub newline_count: usize,
    pub whitespace_before_token: String,
}

impl WhitespacePattern {
    pub fn new() -> WhitespacePattern {
        WhitespacePattern::matching("", "")
    }

    /// Whitespace and newline characters in addition to `\t ` and `\n\r`.
    pub fn matching(whitespace: &str, newlines: &str) -> WhitespacePattern {
        WhitespacePattern {
            chars: format!("{whitespace}\t "),
            newlines: format!("{newlines}\n\r"),
            newline_count: 0,
            whitespace_before_token: String::new(),
        }
    }

    pub fn read(&mut self, input: &mut InputScanner) -> String {
        self.newline_count = 0;
        self.whitespace_before_token.clear();
        let start = input.position();
        let rest = &input.text()[start..];
        let len: usize = rest
            .chars()
            .take_while(|c| self.chars.contains(*c) || self.newlines.contains(*c))
            .map(char::len_utf8)
            .sum();
        let s = rest[..len].to_string();
        input.set_position(start + len);
        if s == " " {
            self.whitespace_before_token = " ".into();
        } else if !s.is_empty() {
            let parts = self.split_lines(&s);
            self.newline_count = parts.len() - 1;
            self.whitespace_before_token = parts[self.newline_count].clone();
        }
        s
    }

    fn split_lines(&self, s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
                out.push(std::mem::take(&mut current));
            } else if self.newlines.contains(c) {
                out.push(std::mem::take(&mut current));
            } else {
                current.push(c);
            }
        }
        out.push(current);
        out
    }
}

/// `<!-- beautify ignore:start -->` style directives.
pub(crate) struct Directives {
    block: Re,
    end_ignore: Re,
}

impl Directives {
    pub fn new(start: &str, end: &str) -> Directives {
        Directives {
            block: Re::new(&format!(r"{start} beautify( \w+[:]\w+)+ {end}")),
            end_ignore: Re::new(&format!(r"{start}\sbeautify\signore:end\s{end}")),
        }
    }

    pub fn get(&self, text: &str) -> Option<Vec<(String, String)>> {
        if !self.block.is_match(text) {
            return None;
        }
        let pair = Regex::new(r" (\w+)[:](\w+)").expect("valid pattern");
        Some(pair.captures_iter(text).map(|c| (c[1].to_string(), c[2].to_string())).collect())
    }

    pub fn read_ignored(&self, input: &mut InputScanner) -> String {
        input.read_until_after(&self.end_ignore)
    }
}

/// Options shared by all beautifiers.
#[derive(Clone, Debug)]
pub(crate) struct BaseOptions {
    pub end_with_newline: bool,
    pub indent_size: usize,
    pub indent_char: String,
    pub indent_level: usize,
    pub preserve_newlines: bool,
    pub max_preserve_newlines: usize,
    pub indent_with_tabs: bool,
    pub wrap_line_length: usize,
    pub indent_empty_lines: bool,
    pub templating: Vec<String>,
}

impl BaseOptions {
    pub fn new(indent_size: usize, use_tabs: bool) -> BaseOptions {
        let mut o = BaseOptions {
            end_with_newline: false,
            indent_size,
            indent_char: " ".into(),
            indent_level: 0,
            preserve_newlines: true,
            max_preserve_newlines: 32786,
            indent_with_tabs: use_tabs,
            wrap_line_length: 0,
            indent_empty_lines: false,
            templating: vec!["auto".into()],
        };
        if use_tabs {
            o.indent_char = "\t".into();
            if o.indent_size == 1 {
                o.indent_size = 4;
            }
        }
        o
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OutputLine {
    character_count: isize,
    indent_count: isize,
    alignment_count: usize,
    wrap_point_index: usize,
    wrap_point_character_count: isize,
    wrap_point_indent_count: isize,
    wrap_point_alignment_count: usize,
    pub items: Vec<String>,
}

impl OutputLine {
    fn new() -> OutputLine {
        OutputLine {
            character_count: 0,
            indent_count: -1,
            alignment_count: 0,
            wrap_point_index: 0,
            wrap_point_character_count: 0,
            wrap_point_indent_count: -1,
            wrap_point_alignment_count: 0,
            items: Vec::new(),
        }
    }

    pub fn item(&self, index: isize) -> Option<&String> {
        if index < 0 {
            self.items.len().checked_sub((-index) as usize).and_then(|i| self.items.get(i))
        } else {
            self.items.get(index as usize)
        }
    }

    pub fn has_match(&self, pattern: &Re) -> bool {
        self.items.iter().rev().any(|i| pattern.is_match(i))
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn last(&self) -> Option<&String> {
        self.items.last()
    }

    fn push(&mut self, item: String) {
        match item.rfind('\n') {
            Some(i) => self.character_count = js_len(&item[i..]) as isize,
            None => self.character_count += js_len(&item) as isize,
        }
        self.items.push(item);
    }

    fn pop(&mut self) -> Option<String> {
        let item = self.items.pop()?;
        self.character_count -= js_len(&item) as isize;
        Some(item)
    }

    fn trim(&mut self) {
        while self.last().is_some_and(|l| l == " ") {
            self.items.pop();
            self.character_count -= 1;
        }
    }
}

struct IndentCache {
    cache: Vec<String>,
    indent_size: usize,
    indent_string: String,
    base_string: String,
}

impl IndentCache {
    fn new(options: &BaseOptions, base: &str) -> IndentCache {
        let indent_string = if options.indent_with_tabs {
            options.indent_char.clone()
        } else {
            options.indent_char.repeat(options.indent_size)
        };
        let base_string = if options.indent_level > 0 {
            indent_string.repeat(options.indent_level)
        } else {
            base.to_string()
        };
        IndentCache {
            cache: vec![String::new()],
            indent_size: options.indent_size,
            indent_string,
            base_string,
        }
    }

    fn size(&self, indent: isize, column: usize) -> isize {
        let mut result = js_len(&self.base_string) as isize;
        if indent < 0 {
            result = 0;
        }
        result += indent * self.indent_size as isize;
        result += column as isize;
        result
    }

    fn string(&mut self, indent: isize, column: usize) -> String {
        let mut result = self.base_string.clone();
        let mut indent = indent;
        if indent < 0 {
            indent = 0;
            result.clear();
        }
        let column = column + indent as usize * self.indent_size;
        while column >= self.cache.len() {
            let mut c = self.cache.len();
            let mut s = String::new();
            if self.indent_size > 0 && c >= self.indent_size {
                let n = c / self.indent_size;
                c -= n * self.indent_size;
                s = self.indent_string.repeat(n);
            }
            if c > 0 {
                s.push_str(&" ".repeat(c));
            }
            self.cache.push(s);
        }
        result.push_str(&self.cache[column]);
        result
    }
}

/// js-beautify's `Output`.
pub(crate) struct Output {
    indent_cache: IndentCache,
    pub raw: bool,
    end_with_newline: bool,
    pub indent_size: usize,
    pub wrap_line_length: usize,
    indent_empty_lines: bool,
    pub lines: Vec<OutputLine>,
    next_line: OutputLine,
    pub space_before_token: bool,
    pub non_breaking_space: bool,
    pub previous_token_wrapped: bool,
}

impl Output {
    pub fn new(options: &BaseOptions, base_indent: &str) -> Output {
        let mut o = Output {
            indent_cache: IndentCache::new(options, base_indent),
            raw: false,
            end_with_newline: options.end_with_newline,
            indent_size: options.indent_size,
            wrap_line_length: options.wrap_line_length,
            indent_empty_lines: options.indent_empty_lines,
            lines: Vec::new(),
            next_line: OutputLine::new(),
            space_before_token: false,
            non_breaking_space: false,
            previous_token_wrapped: false,
        };
        o.add_outputline();
        o
    }

    fn line_set_indent(&mut self, which: Which, indent: isize, alignment: usize) {
        let size = self.indent_cache.size(indent, alignment);
        let line = self.line_mut(which);
        if line.is_empty() {
            line.indent_count = indent;
            line.alignment_count = alignment;
            line.character_count = size;
        }
    }

    fn line_mut(&mut self, which: Which) -> &mut OutputLine {
        match which {
            Which::Current => self.lines.last_mut().expect("a line"),
            Which::Next => &mut self.next_line,
        }
    }

    pub fn current_line(&self) -> &OutputLine {
        self.lines.last().expect("a line")
    }

    pub fn previous_line(&self) -> Option<&OutputLine> {
        if self.lines.len() > 1 {
            self.lines.get(self.lines.len() - 2)
        } else {
            None
        }
    }

    fn add_outputline(&mut self) {
        let mut line = OutputLine::new();
        let (indent, alignment) = (self.next_line.indent_count, self.next_line.alignment_count);
        let size = self.indent_cache.size(indent, alignment);
        line.indent_count = indent;
        line.alignment_count = alignment;
        line.character_count = size;
        self.lines.push(line);
    }

    pub fn get_line_number(&self) -> usize {
        self.lines.len()
    }

    pub fn get_indent_string(&mut self, indent: isize, column: usize) -> String {
        self.indent_cache.string(indent, column)
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() <= 1 && self.current_line().is_empty()
    }

    pub fn add_new_line(&mut self, force: bool) -> bool {
        if self.is_empty() || (!force && self.just_added_newline()) {
            return false;
        }
        if !self.raw {
            self.add_outputline();
        }
        true
    }

    pub fn get_code(&mut self, eol: &str) -> String {
        self.trim(true);
        if let Some(mut last) = self.lines.last_mut().expect("a line").pop() {
            if last.ends_with('\n') {
                last = last.trim_end_matches('\n').to_string();
            }
            self.lines.last_mut().expect("a line").push(last);
        }
        if self.end_with_newline {
            self.add_outputline();
        }
        let mut lines = Vec::with_capacity(self.lines.len());
        for i in 0..self.lines.len() {
            let line = &self.lines[i];
            let s = if line.is_empty() {
                if self.indent_empty_lines {
                    let indent = line.indent_count;
                    self.indent_cache.string(indent, 0)
                } else {
                    String::new()
                }
            } else {
                let (indent, alignment) = (line.indent_count, line.alignment_count);
                let mut s = self.indent_cache.string(indent, alignment);
                for item in &self.lines[i].items {
                    s.push_str(item);
                }
                s
            };
            lines.push(s);
        }
        let code = lines.join("\n");
        if eol != "\n" {
            code.replace('\n', eol)
        } else {
            code
        }
    }

    pub fn set_wrap_point(&mut self) {
        if self.wrap_line_length > 0 {
            let (ni, na) = (self.next_line.indent_count, self.next_line.alignment_count);
            let line = self.lines.last_mut().expect("a line");
            line.wrap_point_index = line.items.len();
            line.wrap_point_character_count = line.character_count;
            line.wrap_point_indent_count = ni;
            line.wrap_point_alignment_count = na;
        }
    }

    pub fn set_indent(&mut self, indent: isize, alignment: usize) -> bool {
        self.line_set_indent(Which::Next, indent, alignment);
        if self.lines.len() > 1 {
            self.line_set_indent(Which::Current, indent, alignment);
            return true;
        }
        self.line_set_indent(Which::Current, 0, 0);
        false
    }

    pub fn add_raw_token(&mut self, newlines: usize, whitespace_before: &str, text: &str) {
        for _ in 0..newlines {
            self.add_outputline();
        }
        self.line_set_indent(Which::Current, -1, 0);
        let line = self.lines.last_mut().expect("a line");
        line.push(whitespace_before.to_string());
        line.push(text.to_string());
        self.space_before_token = false;
        self.non_breaking_space = false;
        self.previous_token_wrapped = false;
    }

    pub fn add_token(&mut self, text: &str) {
        self.add_space_before_token();
        self.lines.last_mut().expect("a line").push(text.to_string());
        self.space_before_token = false;
        self.non_breaking_space = false;
        self.previous_token_wrapped = self.allow_wrap();
    }

    fn should_wrap(&self) -> bool {
        let line = self.current_line();
        line.wrap_point_index > 0
            && line.character_count > self.wrap_line_length as isize
            && line.wrap_point_character_count > self.next_line.character_count
    }

    fn allow_wrap(&mut self) -> bool {
        if !self.should_wrap() {
            return false;
        }
        self.add_new_line(false);
        let n = self.lines.len();
        let (head, tail) = self.lines.split_at_mut(n - 1);
        let prev = head.last_mut().expect("a previous line");
        let next = &mut tail[0];
        let (wi, wa) = (prev.wrap_point_indent_count, prev.wrap_point_alignment_count);
        let size = self.indent_cache.size(wi, wa);
        if next.is_empty() {
            next.indent_count = wi;
            next.alignment_count = wa;
            next.character_count = size;
        }
        next.items = prev.items.split_off(prev.wrap_point_index);
        next.character_count += prev.character_count - prev.wrap_point_character_count;
        prev.character_count = prev.wrap_point_character_count;
        if next.items.first().is_some_and(|i| i == " ") {
            next.items.remove(0);
            next.character_count -= 1;
        }
        true
    }

    fn add_space_before_token(&mut self) {
        if self.space_before_token && !self.just_added_newline() {
            if !self.non_breaking_space {
                self.set_wrap_point();
            }
            self.lines.last_mut().expect("a line").push(" ".into());
        }
    }

    pub fn remove_indent(&mut self, index: usize) {
        let indent_size = self.indent_size;
        for line in self.lines.iter_mut().skip(index) {
            if line.indent_count > 0 {
                line.indent_count -= 1;
                line.character_count -= indent_size as isize;
            }
        }
        let line = self.lines.last_mut().expect("a line");
        if line.wrap_point_indent_count > 0 {
            line.wrap_point_indent_count -= 1;
        }
    }

    pub fn trim(&mut self, eat_newlines: bool) {
        self.lines.last_mut().expect("a line").trim();
        while eat_newlines && self.lines.len() > 1 && self.current_line().is_empty() {
            self.lines.pop();
            self.lines.last_mut().expect("a line").trim();
        }
    }

    pub fn just_added_newline(&self) -> bool {
        self.current_line().is_empty()
    }

    pub fn just_added_blankline(&self) -> bool {
        self.is_empty()
            || (self.current_line().is_empty() && self.previous_line().is_some_and(|l| l.is_empty()))
    }

    pub fn ensure_empty_line_above(&mut self, starts_with: &str, ends_with: &str) {
        let mut index = self.lines.len() as isize - 2;
        while index >= 0 {
            let line = &self.lines[index as usize];
            if line.is_empty() {
                break;
            } else if !line.item(0).is_some_and(|i| i.starts_with(starts_with))
                && line.item(-1).map(String::as_str) != Some(ends_with)
            {
                self.lines.insert(index as usize + 1, OutputLine::new());
                break;
            }
            index -= 1;
        }
    }

    /// `current_line.set_indent(-1)`: the current line keeps no indentation.
    pub fn set_current_line_unindented(&mut self) {
        self.line_set_indent(Which::Current, -1, 0);
    }

    /// The current line's text (for `has_match` style checks).
    pub fn current_line_has_match(&self, pattern: &Re) -> bool {
        self.current_line().has_match(pattern)
    }
}

#[derive(Clone, Copy)]
enum Which {
    Current,
    Next,
}
