//! Parser infrastructure: a token cursor that builds a rowan green tree
//! directly. Trivia is attached lazily so that nodes never start or end with
//! whitespace or comments.

use rowan::{Checkpoint, GreenNode, GreenNodeBuilder, TextRange, TextSize};

use crate::lexer::Token;
use crate::{PhpVersion, SyntaxError, SyntaxKind, SyntaxKind::*};

pub(crate) struct Parser<'t> {
    text: &'t str,
    tokens: &'t [Token],
    offsets: Vec<u32>,
    /// Indices (into `tokens`) of all non-trivia tokens.
    significant: Vec<usize>,
    /// Current position in `significant`.
    pos: usize,
    /// Next raw token to be added to the tree.
    raw_pos: usize,
    builder: GreenNodeBuilder<'static>,
    errors: Vec<SyntaxError>,
    pub(crate) version: PhpVersion,
    steps: u32,
}

impl<'t> Parser<'t> {
    pub(crate) fn new(text: &'t str, tokens: &'t [Token], version: PhpVersion) -> Parser<'t> {
        let mut offsets = Vec::with_capacity(tokens.len() + 1);
        let mut off = 0u32;
        for t in tokens {
            offsets.push(off);
            off += t.len;
        }
        offsets.push(off);
        let significant =
            tokens.iter().enumerate().filter(|(_, t)| !t.kind.is_trivia()).map(|(i, _)| i).collect();
        Parser {
            text,
            tokens,
            offsets,
            significant,
            pos: 0,
            raw_pos: 0,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
            version,
            steps: 0,
        }
    }

    pub(crate) fn finish(mut self) -> (GreenNode, Vec<SyntaxError>) {
        (self.builder.finish(), std::mem::take(&mut self.errors))
    }

    // ------------------------------------------------------------ lookahead

    pub(crate) fn current(&mut self) -> SyntaxKind {
        self.nth(0)
    }

    pub(crate) fn nth(&mut self, n: usize) -> SyntaxKind {
        self.steps += 1;
        debug_assert!(self.steps < 50_000, "parser is stuck");
        self.significant.get(self.pos + n).map(|&i| self.tokens[i].kind).unwrap_or(EOF)
    }

    pub(crate) fn at(&mut self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    pub(crate) fn nth_at(&mut self, n: usize, kind: SyntaxKind) -> bool {
        self.nth(n) == kind
    }

    pub(crate) fn at_any(&mut self, kinds: &[SyntaxKind]) -> bool {
        let cur = self.current();
        kinds.contains(&cur)
    }

    pub(crate) fn nth_text(&self, n: usize) -> &'t str {
        match self.significant.get(self.pos + n) {
            Some(&i) => {
                let start = self.offsets[i] as usize;
                let end = self.offsets[i + 1] as usize;
                &self.text[start..end]
            }
            None => "",
        }
    }

    pub(crate) fn current_text(&self) -> &'t str {
        self.nth_text(0)
    }

    /// Whether the n-th token is an identifier with the given (case
    /// insensitive) text, e.g. contextual keywords such as `enum` or `from`.
    pub(crate) fn nth_at_contextual(&mut self, n: usize, kw: &str) -> bool {
        self.nth(n) == IDENT && self.nth_text(n).eq_ignore_ascii_case(kw)
    }

    /// Whether there is trivia between the current token and the next one.
    pub(crate) fn trivia_after_current(&self) -> bool {
        match (self.significant.get(self.pos), self.significant.get(self.pos + 1)) {
            (Some(&a), Some(&b)) => b != a + 1,
            _ => false,
        }
    }

    /// A position usable for progress checks.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn current_range(&self) -> TextRange {
        match self.significant.get(self.pos) {
            Some(&i) => TextRange::new(self.offsets[i].into(), self.offsets[i + 1].into()),
            None => {
                let end = TextSize::from(*self.offsets.last().unwrap());
                TextRange::empty(end)
            }
        }
    }

    /// Start offset of the current token.
    pub(crate) fn current_start(&self) -> TextSize {
        self.current_range().start()
    }

    /// End offset of the previous significant token.
    pub(crate) fn prev_end(&self) -> TextSize {
        if self.pos == 0 {
            return 0.into();
        }
        let i = self.significant[self.pos - 1];
        self.offsets[i + 1].into()
    }

    // ------------------------------------------------------------ tree building

    fn flush_trivia(&mut self) {
        let until = self.significant.get(self.pos).copied().unwrap_or(self.tokens.len());
        while self.raw_pos < until {
            self.raw_token(self.tokens[self.raw_pos].kind);
        }
    }

    fn raw_token(&mut self, kind: SyntaxKind) {
        let i = self.raw_pos;
        let start = self.offsets[i] as usize;
        let end = self.offsets[i + 1] as usize;
        self.builder.token(kind.into(), &self.text[start..end]);
        self.raw_pos += 1;
    }

    pub(crate) fn start(&mut self, kind: SyntaxKind) {
        self.flush_trivia();
        self.builder.start_node(kind.into());
    }

    /// Starts the root node (which also owns leading trivia).
    pub(crate) fn start_root(&mut self, kind: SyntaxKind) {
        self.builder.start_node(kind.into());
    }

    pub(crate) fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    pub(crate) fn checkpoint(&mut self) -> Checkpoint {
        self.flush_trivia();
        self.builder.checkpoint()
    }

    pub(crate) fn start_at(&mut self, cp: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(cp, kind.into());
    }

    /// Adds all remaining tokens (trailing trivia) to the current node.
    pub(crate) fn flush_rest(&mut self) {
        while self.raw_pos < self.tokens.len() {
            self.raw_token(self.tokens[self.raw_pos].kind);
        }
    }

    pub(crate) fn bump(&mut self) {
        let kind = self.current();
        self.bump_remap(kind);
    }

    /// Consumes the current token but stores it with a different kind.
    pub(crate) fn bump_remap(&mut self, kind: SyntaxKind) {
        if self.pos >= self.significant.len() {
            return;
        }
        self.flush_trivia();
        self.raw_token(kind);
        self.pos += 1;
        self.steps = 0;
    }

    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) {
            return true;
        }
        self.error(format!("expected {}", describe(kind)));
        false
    }

    // ------------------------------------------------------------ errors

    pub(crate) fn error(&mut self, msg: impl Into<String>) {
        // Report missing tokens at the end of the previous token, which is
        // where the user expects them.
        let at = self.prev_end();
        let range = if self.at(EOF) || self.pos == 0 { self.current_range() } else { TextRange::empty(at) };
        self.push_error(msg, range);
    }

    pub(crate) fn error_at_current(&mut self, msg: impl Into<String>) {
        let range = self.current_range();
        self.push_error(msg, range);
    }

    pub(crate) fn push_error(&mut self, msg: impl Into<String>, range: TextRange) {
        let msg = msg.into();
        if let Some(last) = self.errors.last() {
            if last.range == range && last.message == msg {
                return;
            }
        }
        self.errors.push(SyntaxError { message: msg, range });
    }

    /// Wraps the current token in an `ERROR` node.
    pub(crate) fn err_and_bump(&mut self, msg: impl Into<String>) {
        self.error_at_current(msg);
        if self.at(EOF) {
            return;
        }
        self.start(ERROR);
        self.bump();
        self.finish_node();
    }

    /// Reports an error if `version` is newer than the configured version.
    pub(crate) fn require_version(&mut self, version: PhpVersion, feature: &str, range: TextRange) {
        if self.version < version {
            self.push_error(format!("PHP {version} or newer is required for {feature}"), range);
        }
    }

    /// Like [`require_version`] for the current token.
    pub(crate) fn require_version_here(&mut self, version: PhpVersion, feature: &str) {
        let range = self.current_range();
        self.require_version(version, feature, range);
    }
}

pub(crate) fn describe(kind: SyntaxKind) -> String {
    let s = match kind {
        SEMICOLON => "`;`",
        COMMA => "`,`",
        L_PAREN => "`(`",
        R_PAREN => "`)`",
        L_BRACK => "`[`",
        R_BRACK => "`]`",
        L_CURLY => "`{`",
        R_CURLY => "`}`",
        COLON => "`:`",
        COLON2 => "`::`",
        FAT_ARROW => "`=>`",
        EQ => "`=`",
        VARIABLE => "variable",
        IDENT => "identifier",
        AS_KW => "`as`",
        WHILE_KW => "`while`",
        CLASS_KW => "`class`",
        FUNCTION_KW => "`function`",
        HEREDOC_END => "heredoc end marker",
        DOUBLE_QUOTE => "`\"`",
        BACKTICK => "`` ` ``",
        EOF => "end of file",
        _ => return format!("{kind:?}").to_lowercase().trim_end_matches("_kw").replace('_', " "),
    };
    s.to_string()
}
