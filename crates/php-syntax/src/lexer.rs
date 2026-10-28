//! Lexer turning PHP source text into a flat list of tokens.
//!
//! The lexer is lossless: concatenating the text of all tokens yields the
//! original input. It never fails; invalid input produces `ERROR_TOKEN`s and
//! lexer errors.

use crate::{PhpVersion, SyntaxKind, SyntaxKind::*};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token {
    pub kind: SyntaxKind,
    pub len: u32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LexError {
    pub msg: String,
    pub offset: u32,
    pub len: u32,
}

/// Tokenizes `text` according to the rules of PHP `version`.
pub fn tokenize(text: &str, version: PhpVersion) -> (Vec<Token>, Vec<LexError>) {
    tokenize_from(text, version, false)
}

/// Like [`tokenize`]; with `php_code` the text starts in PHP mode (as if
/// preceded by `<?php`).
pub fn tokenize_from(text: &str, version: PhpVersion, php_code: bool) -> (Vec<Token>, Vec<LexError>) {
    let mut lexer = Lexer {
        src: text.as_bytes(),
        pos: 0,
        version,
        tokens: Vec::new(),
        errors: Vec::new(),
        states: vec![if php_code { State::Php { braces: 0, in_string: false } } else { State::Html }],
        halt_pending: false,
        last_significant: EOF,
    };
    lexer.run();
    (lexer.tokens, lexer.errors)
}

#[derive(Clone, Debug)]
enum State {
    Html,
    Php { braces: u32, in_string: bool },
    DoubleQuote,
    Backtick,
    Heredoc { label: Vec<u8>, nowdoc: bool, body_start: usize },
    VarOffset,
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    version: PhpVersion,
    tokens: Vec<Token>,
    errors: Vec<LexError>,
    states: Vec<State>,
    halt_pending: bool,
    last_significant: SyntaxKind,
}

fn is_label_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn is_label_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

const CASTS: &[&str] = &[
    "int", "integer", "bool", "boolean", "float", "double", "real", "string", "binary", "array", "object",
    "unset", "void",
];

impl<'a> Lexer<'a> {
    fn run(&mut self) {
        while self.pos < self.src.len() {
            let state = self.states.last().cloned().unwrap_or(State::Html);
            let before = (self.pos, self.states.len());
            match state {
                State::Html => self.lex_html(),
                State::Php { .. } => self.lex_php(),
                State::DoubleQuote => self.lex_string_body(b'"'),
                State::Backtick => self.lex_string_body(b'`'),
                State::Heredoc { .. } => self.lex_string_body(0),
                State::VarOffset => self.lex_var_offset(),
            }
            if (self.pos, self.states.len()) == before {
                // Safety net: never loop forever.
                self.error_char();
            }
            if self.states.is_empty() {
                self.states.push(State::Html);
            }
        }
        if self
            .states
            .iter()
            .any(|s| matches!(s, State::DoubleQuote | State::Backtick | State::Heredoc { .. }))
        {
            let offset = self.src.len() as u32;
            self.errors.push(LexError { msg: "unterminated string".into(), offset, len: 0 });
        }
    }

    fn peek(&self, n: usize) -> u8 {
        self.src.get(self.pos + n).copied().unwrap_or(0)
    }

    fn rest(&self) -> &'a [u8] {
        &self.src[self.pos..]
    }

    fn starts_with(&self, s: &[u8]) -> bool {
        self.rest().starts_with(s)
    }

    fn push(&mut self, kind: SyntaxKind, len: usize) {
        debug_assert!(len > 0);
        self.tokens.push(Token { kind, len: len as u32 });
        self.pos += len;
        if !kind.is_trivia() {
            self.last_significant = kind;
        }
    }

    fn error(&mut self, msg: impl Into<String>, offset: usize, len: usize) {
        self.errors.push(LexError { msg: msg.into(), offset: offset as u32, len: len as u32 });
    }

    fn error_char(&mut self) {
        let len = utf8_len(self.peek(0)).min(self.src.len() - self.pos);
        self.error("unexpected character", self.pos, len);
        self.push(ERROR_TOKEN, len);
    }

    fn set_top(&mut self, state: State) {
        if let Some(top) = self.states.last_mut() {
            *top = state;
        } else {
            self.states.push(state);
        }
    }

    // ---------------------------------------------------------------- html

    fn lex_html(&mut self) {
        let start = self.pos;
        let mut i = self.pos;
        let src = self.src;
        while i < src.len() {
            if src[i] == b'<' && src.get(i + 1) == Some(&b'?') {
                if let Some((kind, len)) = self.open_tag_at(i) {
                    if i > start {
                        self.push(INLINE_HTML, i - start);
                    }
                    self.push(kind, len);
                    self.set_top(State::Php { braces: 0, in_string: false });
                    return;
                }
            }
            i += 1;
        }
        self.push(INLINE_HTML, src.len() - start);
    }

    fn open_tag_at(&self, i: usize) -> Option<(SyntaxKind, usize)> {
        let src = self.src;
        let after = |n: usize| src.get(i + n).copied();
        if after(2) == Some(b'=') {
            return Some((OPEN_TAG_WITH_ECHO, 3));
        }
        if src.len() >= i + 5 && src[i + 2..i + 5].eq_ignore_ascii_case(b"php") {
            match after(5) {
                None => return Some((OPEN_TAG, 5)),
                Some(b'\r') if after(6) == Some(b'\n') => return Some((OPEN_TAG, 7)),
                Some(b) if is_ws(b) => return Some((OPEN_TAG, 6)),
                _ => {}
            }
        }
        // short open tag `<?`
        match after(2) {
            None => Some((OPEN_TAG, 2)),
            Some(b'\r') if after(3) == Some(b'\n') => Some((OPEN_TAG, 4)),
            Some(b) if is_ws(b) => Some((OPEN_TAG, 3)),
            _ => None,
        }
    }

    // ---------------------------------------------------------------- php

    fn lex_php(&mut self) {
        let b = self.peek(0);
        let start = self.pos;
        match b {
            _ if is_ws(b) => {
                let len = self.rest().iter().take_while(|&&c| is_ws(c)).count();
                self.push(WHITESPACE, len);
            }
            b'?' if self.peek(1) == b'>' => {
                let mut len = 2;
                if self.peek(2) == b'\n' {
                    len = 3;
                } else if self.peek(2) == b'\r' && self.peek(3) == b'\n' {
                    len = 4;
                }
                self.push(CLOSE_TAG, len);
                if self.check_halt(CLOSE_TAG) {
                    return;
                }
                self.set_top(State::Html);
            }
            b'#' if self.peek(1) == b'[' && self.version >= PhpVersion::PHP_8_0 => self.push(HASH_BRACK, 2),
            b'#' => self.line_comment(),
            b'/' if self.peek(1) == b'/' => self.line_comment(),
            b'/' if self.peek(1) == b'*' => self.block_comment(),
            b'$' if is_label_start(self.peek(1)) => {
                let len = 1 + self.label_len(1);
                self.push(VARIABLE, len);
            }
            _ if is_label_start(b) => self.ident(),
            b'0'..=b'9' => self.number(),
            b'.' if self.peek(1).is_ascii_digit() => self.number(),
            b'\'' => self.single_quoted(),
            b'"' => self.double_quoted(),
            b'`' => {
                self.push(BACKTICK, 1);
                self.states.push(State::Backtick);
            }
            b'<' if self.starts_with(b"<<<") && self.heredoc_start() => {}
            b'(' if self.cast() => {}
            b'{' => {
                if let Some(State::Php { braces, .. }) = self.states.last_mut() {
                    *braces += 1;
                }
                self.push(L_CURLY, 1);
            }
            b'}' => {
                let mut pop = false;
                if let Some(State::Php { braces, in_string }) = self.states.last_mut() {
                    if *braces == 0 {
                        pop = *in_string;
                    } else {
                        *braces -= 1;
                    }
                }
                self.push(R_CURLY, 1);
                if pop {
                    self.states.pop();
                }
            }
            _ => {
                if let Some((kind, len)) = self.operator() {
                    self.push(kind, len);
                    self.check_halt(kind);
                } else {
                    debug_assert_eq!(start, self.pos);
                    self.error_char();
                }
            }
        }
    }

    /// After `__halt_compiler` the first `;` or `?>` ends the PHP code; the
    /// remaining bytes are raw data.
    fn check_halt(&mut self, kind: SyntaxKind) -> bool {
        if self.halt_pending && matches!(kind, SEMICOLON | CLOSE_TAG) {
            self.halt_pending = false;
            if self.pos < self.src.len() {
                let len = self.src.len() - self.pos;
                self.push(HALT_COMPILER_DATA, len);
            }
            return true;
        }
        false
    }

    fn label_len(&self, offset: usize) -> usize {
        self.src[self.pos + offset..].iter().take_while(|&&c| is_label_char(c)).count()
    }

    fn ident(&mut self) {
        let len = self.label_len(0);
        let text = std::str::from_utf8(&self.src[self.pos..self.pos + len]).unwrap_or("");
        let after_arrow = matches!(self.last_significant, ARROW | NULLSAFE_ARROW);
        let kind = if after_arrow { None } else { SyntaxKind::from_keyword(text) };
        match kind {
            Some(HALT_COMPILER_KW) if !matches!(self.last_significant, COLON2 | FUNCTION_KW) => {
                self.halt_pending = true;
                self.push(HALT_COMPILER_KW, len);
            }
            Some(kw) => self.push(kw, len),
            None => self.push(IDENT, len),
        }
    }

    fn line_comment(&mut self) {
        let rest = self.rest();
        let mut len = 0;
        while len < rest.len() {
            match rest[len] {
                b'\n' | b'\r' => break,
                b'?' if rest.get(len + 1) == Some(&b'>') => break,
                _ => len += 1,
            }
        }
        self.push(COMMENT, len);
    }

    fn block_comment(&mut self) {
        let rest = self.rest();
        let is_doc = rest.starts_with(b"/**") && rest.get(3).is_some_and(|&b| is_ws(b));
        let len = match find(&rest[2..], b"*/") {
            Some(i) => i + 4,
            None => {
                self.error("unterminated comment", self.pos, rest.len());
                rest.len()
            }
        };
        self.push(if is_doc { DOC_COMMENT } else { COMMENT }, len);
    }

    fn number(&mut self) {
        let rest = self.rest();
        let digits = |from: usize, pred: fn(u8) -> bool| -> usize {
            from + rest[from..].iter().take_while(|&&c| pred(c) || c == b'_').count()
        };
        let lower1 = rest.get(1).map(|b| b.to_ascii_lowercase());
        if rest[0] == b'0' && matches!(lower1, Some(b'x' | b'b' | b'o')) {
            let pred: fn(u8) -> bool = match lower1 {
                Some(b'x') => |c| c.is_ascii_hexdigit(),
                Some(b'b') => |c| c == b'0' || c == b'1',
                _ => |c| (b'0'..=b'7').contains(&c),
            };
            let len = digits(2, pred);
            if len > 2 {
                if lower1 == Some(b'o') && self.version < PhpVersion::PHP_8_1 {
                    self.error("PHP 8.1 or newer is required for explicit octal notation", self.pos, len);
                }
                self.check_separator(&rest[..len]);
                self.push(INT_NUMBER, len);
                return;
            }
        }
        let mut len = digits(0, |c| c.is_ascii_digit());
        let mut float = false;
        if rest.get(len) == Some(&b'.') {
            float = true;
            len = digits(len + 1, |c| c.is_ascii_digit());
        }
        if matches!(rest.get(len), Some(b'e' | b'E')) {
            let mut j = len + 1;
            if matches!(rest.get(j), Some(b'+' | b'-')) {
                j += 1;
            }
            if rest.get(j).is_some_and(|b| b.is_ascii_digit()) {
                float = true;
                len = digits(j, |c| c.is_ascii_digit());
            }
        }
        self.check_separator(&rest[..len]);
        self.push(if float { FLOAT_NUMBER } else { INT_NUMBER }, len);
    }

    fn check_separator(&mut self, text: &[u8]) {
        if text.contains(&b'_') && self.version < PhpVersion::PHP_7_4 {
            self.error("PHP 7.4 or newer is required for numeric literal separators", self.pos, text.len());
        }
    }

    fn single_quoted(&mut self) {
        let rest = self.rest();
        let mut i = 1;
        while i < rest.len() {
            match rest[i] {
                b'\\' => i += 2,
                b'\'' => {
                    self.push(STRING, i + 1);
                    return;
                }
                _ => i += 1,
            }
        }
        self.error("unterminated string", self.pos, rest.len());
        self.push(STRING, rest.len());
    }

    fn double_quoted(&mut self) {
        let rest = self.rest();
        let mut i = 1;
        while i < rest.len() {
            match rest[i] {
                b'\\' => i += 2,
                b'"' => {
                    self.push(STRING, i + 1);
                    return;
                }
                b'$' if rest.get(i + 1).is_some_and(|&b| is_label_start(b) || b == b'{') => break,
                b'{' if rest.get(i + 1) == Some(&b'$') => break,
                _ => i += 1,
            }
        }
        if i >= rest.len() {
            self.error("unterminated string", self.pos, rest.len());
            self.push(STRING, rest.len());
            return;
        }
        self.push(DOUBLE_QUOTE, 1);
        self.states.push(State::DoubleQuote);
    }

    fn heredoc_start(&mut self) -> bool {
        let rest = self.rest();
        let mut i = 3;
        while matches!(rest.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        let quote = match rest.get(i) {
            Some(&q @ (b'"' | b'\'')) => {
                i += 1;
                Some(q)
            }
            _ => None,
        };
        if !rest.get(i).is_some_and(|&b| is_label_start(b)) {
            return false;
        }
        let label_start = i;
        while rest.get(i).is_some_and(|&b| is_label_char(b)) {
            i += 1;
        }
        let label = rest[label_start..i].to_vec();
        if let Some(q) = quote {
            if rest.get(i) != Some(&q) {
                return false;
            }
            i += 1;
        }
        match rest.get(i) {
            Some(b'\n') => i += 1,
            Some(b'\r') if rest.get(i + 1) == Some(&b'\n') => i += 2,
            Some(b'\r') => i += 1,
            _ => return false,
        }
        let nowdoc = quote == Some(b'\'');
        self.push(HEREDOC_START, i);
        self.states.push(State::Heredoc { label, nowdoc, body_start: self.pos });
        true
    }

    fn cast(&mut self) -> bool {
        let rest = self.rest();
        let mut i = 1;
        while matches!(rest.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        let word_start = i;
        while rest.get(i).is_some_and(|b| b.is_ascii_alphabetic()) {
            i += 1;
        }
        let word = &rest[word_start..i];
        if word.is_empty() || !CASTS.iter().any(|c| c.as_bytes().eq_ignore_ascii_case(word)) {
            return false;
        }
        while matches!(rest.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        if rest.get(i) != Some(&b')') {
            return false;
        }
        self.push(CAST, i + 1);
        true
    }

    fn operator(&self) -> Option<(SyntaxKind, usize)> {
        let three: &[(&[u8], SyntaxKind)] = &[
            (b"**=", STAR2_EQ),
            (b"...", DOT3),
            (b"<=>", SPACESHIP),
            (b"===", EQ3),
            (b"!==", NEQ2),
            (b"??=", QUESTION2_EQ),
            (b"<<=", SHL_EQ),
            (b">>=", SHR_EQ),
        ];
        for &(s, k) in three {
            if self.starts_with(s) {
                return Some((k, 3));
            }
        }
        // also before PHP 8 (never valid code there): the parser reports the
        // version instead of a cascade of syntax errors
        if self.starts_with(b"?->") {
            return Some((NULLSAFE_ARROW, 3));
        }
        if self.starts_with(b"|>") {
            return Some((PIPE_GT, 2));
        }
        let two: &[(&[u8], SyntaxKind)] = &[
            (b"->", ARROW),
            (b"=>", FAT_ARROW),
            (b"::", COLON2),
            (b"??", QUESTION2),
            (b"+=", PLUS_EQ),
            (b"-=", MINUS_EQ),
            (b"*=", STAR_EQ),
            (b"/=", SLASH_EQ),
            (b".=", DOT_EQ),
            (b"%=", PERCENT_EQ),
            (b"&=", AMP_EQ),
            (b"|=", PIPE_EQ),
            (b"^=", CARET_EQ),
            (b"==", EQ2),
            (b"!=", NEQ),
            (b"<>", LT_GT),
            (b"<=", LT_EQ),
            (b">=", GT_EQ),
            (b"**", STAR2),
            (b"++", PLUS2),
            (b"--", MINUS2),
            (b"&&", AMP2),
            (b"||", PIPE2),
            (b"<<", SHL),
            (b">>", SHR),
        ];
        for &(s, k) in two {
            if self.starts_with(s) {
                return Some((k, 2));
            }
        }
        let kind = match self.peek(0) {
            b';' => SEMICOLON,
            b',' => COMMA,
            b'(' => L_PAREN,
            b')' => R_PAREN,
            b'[' => L_BRACK,
            b']' => R_BRACK,
            b'.' => DOT,
            b':' => COLON,
            b'?' => QUESTION,
            b'=' => EQ,
            b'<' => LT,
            b'>' => GT,
            b'+' => PLUS,
            b'-' => MINUS,
            b'*' => STAR,
            b'/' => SLASH,
            b'%' => PERCENT,
            b'!' => BANG,
            b'~' => TILDE,
            b'&' => AMP,
            b'|' => PIPE,
            b'^' => CARET,
            b'@' => AT,
            b'$' => DOLLAR,
            b'\\' => BACKSLASH,
            _ => return None,
        };
        Some((kind, 1))
    }

    // ---------------------------------------------------------------- strings

    /// Lexes the body of an interpolated string. `term` is the terminating
    /// byte (`"` or `` ` ``) or `0` for heredocs.
    fn lex_string_body(&mut self, term: u8) {
        let (label, nowdoc, body_start) = match self.states.last() {
            Some(State::Heredoc { label, nowdoc, body_start }) => (label.clone(), *nowdoc, *body_start),
            _ => (Vec::new(), false, 0),
        };
        let src = self.src;
        let start = self.pos;
        let mut i = self.pos;
        while i < src.len() {
            if term == 0 {
                let at_line_start = i == body_start || src[i - 1] == b'\n' || src[i - 1] == b'\r';
                if at_line_start {
                    if let Some(len) = heredoc_end_at(src, i, &label) {
                        if i > start {
                            self.push(ENCAPSED_TEXT, i - start);
                        }
                        self.push(HEREDOC_END, len);
                        self.states.pop();
                        return;
                    }
                }
            } else if src[i] == term {
                if i > start {
                    self.push(ENCAPSED_TEXT, i - start);
                }
                self.push(if term == b'"' { DOUBLE_QUOTE } else { BACKTICK }, 1);
                self.states.pop();
                return;
            }
            if nowdoc {
                i += 1;
                continue;
            }
            match src[i] {
                b'\\' => i += 2,
                b'$' if src.get(i + 1).is_some_and(|&b| is_label_start(b)) => {
                    if i > start {
                        self.push(ENCAPSED_TEXT, i - start);
                    }
                    let len = 1 + self.label_len(1);
                    self.push(VARIABLE, len);
                    if self.peek(0) == b'[' {
                        self.states.push(State::VarOffset);
                    } else if self.peek(0) == b'-' && self.peek(1) == b'>' && is_label_start(self.peek(2)) {
                        self.push(ARROW, 2);
                        let len = self.label_len(0);
                        self.push(IDENT, len);
                    } else if self.version >= PhpVersion::PHP_8_0
                        && self.starts_with(b"?->")
                        && is_label_start(self.peek(3))
                    {
                        self.push(NULLSAFE_ARROW, 3);
                        let len = self.label_len(0);
                        self.push(IDENT, len);
                    }
                    return;
                }
                b'$' if src.get(i + 1) == Some(&b'{') => {
                    if i > start {
                        self.push(ENCAPSED_TEXT, i - start);
                    }
                    self.push(DOLLAR_CURLY, 2);
                    self.states.push(State::Php { braces: 0, in_string: true });
                    return;
                }
                b'{' if src.get(i + 1) == Some(&b'$') => {
                    if i > start {
                        self.push(ENCAPSED_TEXT, i - start);
                    }
                    self.push(L_CURLY, 1);
                    self.states.push(State::Php { braces: 0, in_string: true });
                    return;
                }
                _ => i += 1,
            }
        }
        let end = src.len();
        if end > start {
            self.push(ENCAPSED_TEXT, end - start);
        }
        self.error("unterminated string", start, end - start);
        self.states.pop();
    }

    /// Simple array offset inside a string: `"$a[0]"`, `"$a[key]"`, `"$a[$b]"`.
    fn lex_var_offset(&mut self) {
        let b = self.peek(0);
        match b {
            b'[' => self.push(L_BRACK, 1),
            b']' => {
                self.push(R_BRACK, 1);
                self.states.pop();
            }
            b'-' => self.push(MINUS, 1),
            b'$' if is_label_start(self.peek(1)) => {
                let len = 1 + self.label_len(1);
                self.push(VARIABLE, len);
            }
            b'0'..=b'9' => {
                let len = self.label_len(0);
                self.push(INT_NUMBER, len);
            }
            _ if is_label_start(b) => {
                let len = self.label_len(0);
                self.push(IDENT, len);
            }
            _ => {
                self.states.pop();
            }
        }
    }
}

fn heredoc_end_at(src: &[u8], mut i: usize, label: &[u8]) -> Option<usize> {
    let start = i;
    while matches!(src.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    if !src[i.min(src.len())..].starts_with(label) {
        return None;
    }
    let end = i + label.len();
    if src.get(end).is_some_and(|&b| is_label_char(b)) {
        return None;
    }
    Some(end - start)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn utf8_len(first: u8) -> usize {
    match first {
        0xf0..=0xff => 4,
        0xe0..=0xef => 3,
        0xc0..=0xdf => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(text: &str) -> String {
        let (tokens, errors) = tokenize(text, PhpVersion::LATEST);
        let mut out = String::new();
        let mut pos = 0usize;
        for t in tokens {
            let end = pos + t.len as usize;
            out.push_str(&format!("{:?} {:?}\n", t.kind, &text[pos..end]));
            pos = end;
        }
        for e in errors {
            out.push_str(&format!("error {}: {}\n", e.offset, e.msg));
        }
        out
    }

    #[test]
    fn lossless_and_basic() {
        let text = "<html><?php $a = 1 + 2.5; // hi\n echo \"x $a {$b->c} ${d}\"; ?>\n</html>";
        let (tokens, errors) = tokenize(text, PhpVersion::LATEST);
        assert!(errors.is_empty(), "{errors:?}");
        let total: u32 = tokens.iter().map(|t| t.len).sum();
        assert_eq!(total as usize, text.len());
        expect_test::expect![[r##"
            INLINE_HTML "<html>"
            OPEN_TAG "<?php "
            VARIABLE "$a"
            WHITESPACE " "
            EQ "="
            WHITESPACE " "
            INT_NUMBER "1"
            WHITESPACE " "
            PLUS "+"
            WHITESPACE " "
            FLOAT_NUMBER "2.5"
            SEMICOLON ";"
            WHITESPACE " "
            COMMENT "// hi"
            WHITESPACE "\n "
            ECHO_KW "echo"
            WHITESPACE " "
            DOUBLE_QUOTE "\""
            ENCAPSED_TEXT "x "
            VARIABLE "$a"
            ENCAPSED_TEXT " "
            L_CURLY "{"
            VARIABLE "$b"
            ARROW "->"
            IDENT "c"
            R_CURLY "}"
            ENCAPSED_TEXT " "
            DOLLAR_CURLY "${"
            IDENT "d"
            R_CURLY "}"
            DOUBLE_QUOTE "\""
            SEMICOLON ";"
            WHITESPACE " "
            CLOSE_TAG "?>\n"
            INLINE_HTML "</html>"
        "##]]
        .assert_eq(&lex(text));
    }

    #[test]
    fn heredoc_and_nowdoc() {
        let text = "<?php\n$x = <<<EOT\n  a $b[0] c\n  EOT;\n$y = <<<'N'\n$raw\nN;\n";
        expect_test::expect![[r##"
            OPEN_TAG "<?php\n"
            VARIABLE "$x"
            WHITESPACE " "
            EQ "="
            WHITESPACE " "
            HEREDOC_START "<<<EOT\n"
            ENCAPSED_TEXT "  a "
            VARIABLE "$b"
            L_BRACK "["
            INT_NUMBER "0"
            R_BRACK "]"
            ENCAPSED_TEXT " c\n"
            HEREDOC_END "  EOT"
            SEMICOLON ";"
            WHITESPACE "\n"
            VARIABLE "$y"
            WHITESPACE " "
            EQ "="
            WHITESPACE " "
            HEREDOC_START "<<<'N'\n"
            ENCAPSED_TEXT "$raw\n"
            HEREDOC_END "N"
            SEMICOLON ";"
            WHITESPACE "\n"
        "##]]
        .assert_eq(&lex(text));
    }

    #[test]
    fn casts_attributes_keywords() {
        let text = "<?php #[Attr] (int) $a; ( string )$b; $o->class; Foo::CLASS; __halt_compiler(); data";
        expect_test::expect![[r##"
            OPEN_TAG "<?php "
            HASH_BRACK "#["
            IDENT "Attr"
            R_BRACK "]"
            WHITESPACE " "
            CAST "(int)"
            WHITESPACE " "
            VARIABLE "$a"
            SEMICOLON ";"
            WHITESPACE " "
            CAST "( string )"
            VARIABLE "$b"
            SEMICOLON ";"
            WHITESPACE " "
            VARIABLE "$o"
            ARROW "->"
            IDENT "class"
            SEMICOLON ";"
            WHITESPACE " "
            IDENT "Foo"
            COLON2 "::"
            CLASS_KW "CLASS"
            SEMICOLON ";"
            WHITESPACE " "
            HALT_COMPILER_KW "__halt_compiler"
            L_PAREN "("
            R_PAREN ")"
            SEMICOLON ";"
            HALT_COMPILER_DATA " data"
        "##]]
        .assert_eq(&lex(text));
    }

    #[test]
    fn attributes_are_comments_before_php8() {
        let (tokens, _) = tokenize("<?php #[Attr]\n", PhpVersion::PHP_7_4);
        assert_eq!(tokens[1].kind, COMMENT);
    }
}
