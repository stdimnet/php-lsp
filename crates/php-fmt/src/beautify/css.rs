//! js-beautify's CSS beautifier (`js/src/css`), used for `<style>`
//! contents of templates.
//!
//! js-beautify is MIT licensed, Copyright (c) 2007-2018 Einar Lielmanis,
//! Liam Newman, and contributors.

use super::core::{BaseOptions, Directives, InputScanner, Output, Re};
use super::html::ChildOptions;

const NESTED_AT_RULE: &[&str] = &["page", "font-face", "keyframes", "media", "supports", "document"];
const CONDITIONAL_GROUP_RULE: &[&str] = &["media", "supports", "document"];
const NON_SEMICOLON_NEWLINE_PROPERTY: &[&str] = &["grid-template-areas", "grid-template"];

struct Options {
    base: BaseOptions,
    selector_separator_newline: bool,
    newline_between_rules: bool,
    space_around_combinator: bool,
    expand_braces: bool,
}

struct Beautifier {
    options: Options,
    input: InputScanner,
    output: Output,
    ch: String,
    indent_level: isize,
    nested_level: usize,
}

fn is_space(c: Option<char>) -> bool {
    c.is_some_and(char::is_whitespace)
}

impl Beautifier {
    fn eat_string(&mut self, end_chars: &str) -> String {
        let mut result = String::new();
        let mut ch = self.input.next();
        while let Some(c) = ch {
            result.push(c);
            if c == '\\' {
                if let Some(n) = self.input.next() {
                    result.push(n);
                }
            } else if end_chars.contains(c) || c == '\n' {
                break;
            }
            ch = self.input.next();
        }
        self.ch = ch.map(String::from).unwrap_or_default();
        result
    }

    fn eat_whitespace(&mut self, allow_newline: bool) -> bool {
        let result = is_space(self.input.peek0());
        let mut newline_count = 0;
        while is_space(self.input.peek0()) {
            let c = self.input.next();
            self.ch = c.map(String::from).unwrap_or_default();
            if allow_newline
                && c == Some('\n')
                && (newline_count == 0 || newline_count < self.options.base.max_preserve_newlines)
            {
                newline_count += 1;
                self.output.add_new_line(true);
            }
        }
        result
    }

    fn found_nested_pseudo_class(&self) -> bool {
        let mut open_paren = 0;
        let mut i = 1;
        while let Some(ch) = self.input.peek(i) {
            match ch {
                '{' => return true,
                '(' => open_paren += 1,
                ')' => {
                    if open_paren == 0 {
                        return false;
                    }
                    open_paren -= 1;
                }
                ';' | '}' => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    fn print_string(&mut self, s: &str) {
        self.output.set_indent(self.indent_level, 0);
        self.output.non_breaking_space = true;
        self.output.add_token(s);
    }

    fn preserve_single_space(&mut self, after_space: bool) {
        if after_space {
            self.output.space_before_token = true;
        }
    }

    fn indent(&mut self) {
        self.indent_level += 1;
    }

    fn outdent(&mut self) {
        if self.indent_level > 0 {
            self.indent_level -= 1;
        }
    }

    fn beautify(&mut self, eol: &str) -> String {
        let whitespace = Re::new(r"(?:\s|\n)+");
        let block_comment = Re::new(r"/\*(?s:.*?)(?:\*/|$)");
        let line_comment = Re::new("//[^\n\r\u{2028}\u{2029}]*");
        let delimiter = Re::new(r#"[: ,;{}()\[\]/='"]"#);
        let directives = Directives::new(r"/\*", r"\*/");
        let mut paren_level = 0usize;
        let mut inside_rule = false;
        let mut inside_property_value = false;
        let mut entering_conditional_group = false;
        let mut inside_non_nested_at_rule = false;
        let mut inside_scss_map = false;
        let mut top = String::new();
        let mut inside_non_semicolon_values = false;
        loop {
            let ws = self.input.match_re(&whitespace).unwrap_or_default();
            let is_after_space = !ws.is_empty();
            let previous = top.clone();
            let mut ch = self.input.next().map(String::from).unwrap_or_default();
            if ch == "\\" && self.input.has_next() {
                if let Some(n) = self.input.next() {
                    ch.push(n);
                }
            }
            self.ch = ch.clone();
            top = ch.clone();
            let peek = self.input.peek0();
            if ch.is_empty() {
                break;
            } else if ch == "/" && peek == Some('*') {
                self.output.add_new_line(false);
                self.input.back();
                let mut comment = self.input.match_re(&block_comment).unwrap_or_default();
                if directives
                    .get(&comment)
                    .is_some_and(|d| d.iter().any(|(k, v)| k == "ignore" && v == "start"))
                {
                    comment.push_str(&directives.read_ignored(&mut self.input));
                }
                self.print_string(&comment);
                self.eat_whitespace(true);
                self.output.add_new_line(false);
            } else if ch == "/" && peek == Some('/') {
                self.output.space_before_token = true;
                self.input.back();
                let c = self.input.match_re(&line_comment).unwrap_or_default();
                self.print_string(&c);
                self.eat_whitespace(true);
            } else if ch == "$" {
                self.preserve_single_space(is_after_space);
                self.print_string(&ch);
                let mut variable = self.input.peek_until_after(&delimiter);
                if variable.ends_with([' ', ':']) {
                    variable = self.eat_string(": ").trim_end().to_string();
                    self.print_string(&variable);
                    self.output.space_before_token = true;
                }
                if paren_level == 0 && variable.contains(':') {
                    inside_property_value = true;
                    self.indent();
                }
            } else if ch == "@" {
                self.preserve_single_space(is_after_space);
                if peek == Some('{') {
                    let s = format!("{ch}{}", self.eat_string("}"));
                    self.print_string(&s);
                } else {
                    self.print_string(&ch);
                    let mut rule = self.input.peek_until_after(&delimiter);
                    if rule.ends_with([' ', ':']) {
                        rule = self.eat_string(": ").trim_end().to_string();
                        self.print_string(&rule);
                        self.output.space_before_token = true;
                    }
                    if paren_level == 0 && rule.contains(':') {
                        inside_property_value = true;
                        self.indent();
                    } else if NESTED_AT_RULE.contains(&rule.as_str()) {
                        self.nested_level += 1;
                        if CONDITIONAL_GROUP_RULE.contains(&rule.as_str()) {
                            entering_conditional_group = true;
                        }
                    } else if paren_level == 0 && !inside_property_value {
                        inside_non_nested_at_rule = true;
                    }
                }
            } else if ch == "#" && peek == Some('{') {
                self.preserve_single_space(is_after_space);
                let s = format!("{ch}{}", self.eat_string("}"));
                self.print_string(&s);
            } else if ch == "{" {
                if inside_property_value {
                    inside_property_value = false;
                    self.outdent();
                }
                inside_non_nested_at_rule = false;
                if entering_conditional_group {
                    entering_conditional_group = false;
                    inside_rule = self.indent_level >= self.nested_level as isize;
                } else {
                    inside_rule = self.indent_level >= self.nested_level as isize - 1;
                }
                if self.options.newline_between_rules
                    && inside_rule
                    && self
                        .output
                        .previous_line()
                        .is_some_and(|l| l.items.last().map(String::as_str) != Some("{"))
                {
                    self.output.ensure_empty_line_above("/", ",");
                }
                self.output.space_before_token = true;
                if self.options.expand_braces {
                    self.output.add_new_line(false);
                    self.print_string(&ch);
                    self.indent();
                    self.output.set_indent(self.indent_level, 0);
                } else {
                    if previous == "(" {
                        self.output.space_before_token = false;
                    } else if previous != "," {
                        self.indent();
                    }
                    self.print_string(&ch);
                }
                self.eat_whitespace(true);
                self.output.add_new_line(false);
            } else if ch == "}" {
                self.outdent();
                self.output.add_new_line(false);
                if previous == "{" {
                    self.output.trim(true);
                }
                if inside_property_value {
                    self.outdent();
                    inside_property_value = false;
                }
                self.print_string(&ch);
                inside_rule = false;
                if self.nested_level > 0 {
                    self.nested_level -= 1;
                }
                self.eat_whitespace(true);
                self.output.add_new_line(false);
                if self.options.newline_between_rules
                    && !self.output.just_added_blankline()
                    && self.input.peek0() != Some('}')
                {
                    self.output.add_new_line(true);
                }
                if self.input.peek0() == Some(')') {
                    self.output.trim(true);
                    if self.options.expand_braces {
                        self.output.add_new_line(true);
                    }
                }
            } else if ch == ":" {
                if NON_SEMICOLON_NEWLINE_PROPERTY.iter().any(|p| self.input.look_back(p)) {
                    inside_non_semicolon_values = true;
                }
                if (inside_rule || entering_conditional_group)
                    && !(self.input.look_back("&") || self.found_nested_pseudo_class())
                    && !self.input.look_back("(")
                    && !inside_non_nested_at_rule
                    && paren_level == 0
                {
                    self.print_string(":");
                    if !inside_property_value {
                        inside_property_value = true;
                        self.output.space_before_token = true;
                        self.eat_whitespace(true);
                        self.indent();
                    }
                } else {
                    if self.input.look_back(" ") {
                        self.output.space_before_token = true;
                    }
                    if self.input.peek0() == Some(':') {
                        self.input.next();
                        self.print_string("::");
                    } else {
                        self.print_string(":");
                    }
                }
            } else if ch == "\"" || ch == "'" {
                let preserve = previous == "\"" || previous == "'";
                self.preserve_single_space(preserve || is_after_space);
                let s = format!("{ch}{}", self.eat_string(&ch));
                self.print_string(&s);
                self.eat_whitespace(true);
            } else if ch == ";" {
                inside_non_semicolon_values = false;
                if paren_level == 0 {
                    if inside_property_value {
                        self.outdent();
                        inside_property_value = false;
                    }
                    inside_non_nested_at_rule = false;
                    self.print_string(&ch);
                    self.eat_whitespace(true);
                    if self.input.peek0() != Some('/') {
                        self.output.add_new_line(false);
                    }
                } else {
                    self.print_string(&ch);
                    self.eat_whitespace(true);
                    self.output.space_before_token = true;
                }
            } else if ch == "(" {
                if self.input.look_back("url") {
                    self.print_string(&ch);
                    self.eat_whitespace(false);
                    paren_level += 1;
                    self.indent();
                    let next = self.input.next();
                    match next {
                        Some(')' | '"' | '\'') => self.input.back(),
                        Some(c) => {
                            let s = format!("{c}{}", self.eat_string(")"));
                            self.print_string(&s);
                            if paren_level > 0 {
                                paren_level -= 1;
                                self.outdent();
                            }
                        }
                        None => {}
                    }
                } else {
                    let space_needed = self.input.look_back("with");
                    self.preserve_single_space(is_after_space || space_needed);
                    self.print_string(&ch);
                    if inside_property_value && previous == "$" && self.options.selector_separator_newline {
                        self.output.add_new_line(false);
                        inside_scss_map = true;
                    } else {
                        self.eat_whitespace(false);
                        paren_level += 1;
                        self.indent();
                    }
                }
            } else if ch == ")" {
                if paren_level > 0 {
                    paren_level -= 1;
                    self.outdent();
                }
                if inside_scss_map
                    && self.input.peek0() == Some(';')
                    && self.options.selector_separator_newline
                {
                    inside_scss_map = false;
                    self.outdent();
                    self.output.add_new_line(false);
                }
                self.print_string(&ch);
            } else if ch == "," {
                self.print_string(&ch);
                self.eat_whitespace(true);
                if self.options.selector_separator_newline
                    && (!inside_property_value || inside_scss_map)
                    && paren_level == 0
                    && !inside_non_nested_at_rule
                {
                    self.output.add_new_line(false);
                } else {
                    self.output.space_before_token = true;
                }
            } else if (ch == ">" || ch == "+" || ch == "~") && !inside_property_value && paren_level == 0 {
                if self.options.space_around_combinator {
                    self.output.space_before_token = true;
                    self.print_string(&ch);
                    self.output.space_before_token = true;
                } else {
                    self.print_string(&ch);
                    self.eat_whitespace(false);
                }
            } else if ch == "]" {
                self.print_string(&ch);
            } else if ch == "[" {
                self.preserve_single_space(is_after_space);
                self.print_string(&ch);
            } else if ch == "=" {
                self.eat_whitespace(false);
                self.print_string("=");
            } else if ch == "!" && !self.input.look_back("\\") {
                self.output.space_before_token = true;
                self.print_string(&ch);
            } else {
                let preserve = previous == "\"" || previous == "'";
                self.preserve_single_space(preserve || is_after_space);
                self.print_string(&ch);
                if !self.output.just_added_newline()
                    && self.input.peek0() == Some('\n')
                    && inside_non_semicolon_values
                {
                    self.output.add_new_line(false);
                }
            }
        }
        self.output.get_code(eol)
    }
}

/// Formats CSS like js-beautify with the options a `<style>` in a
/// template gets from Intelephense.
pub(crate) fn beautify(text: &str, options: &ChildOptions) -> String {
    let mut base = BaseOptions::new(options.indent_size, options.use_tabs);
    base.wrap_line_length = options.wrap_line_length;
    let source = text.replace("\r\n", "\n").replace('\r', "\n");
    let base_indent: String = source.chars().take_while(|c| *c == '\t' || *c == ' ').collect();
    let output = Output::new(&base, &base_indent);
    let mut b = Beautifier {
        options: Options {
            base,
            selector_separator_newline: true,
            newline_between_rules: true,
            space_around_combinator: false,
            expand_braces: false,
        },
        input: InputScanner::new(&source),
        output,
        ch: String::new(),
        indent_level: 0,
        nested_level: 0,
    };
    b.beautify("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn css(s: &str) -> String {
        beautify(s, &ChildOptions { indent_size: 4, use_tabs: false, wrap_line_length: 0 })
    }

    #[test]
    fn rules() {
        assert_eq!(
            css("a{color:red;margin:0}b,i{x:1}"),
            "a {\n    color: red;\n    margin: 0\n}\n\nb,\ni {\n    x: 1\n}"
        );
        assert_eq!(
            css("@media (max-width:10px){a{b:c}}"),
            "@media (max-width:10px) {\n    a {\n        b: c\n    }\n}"
        );
    }
}
