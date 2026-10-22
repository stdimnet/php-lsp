//! PHP code formatter (PSR-12 / PER coding style).
//!
//! The formatter only rewrites whitespace *between* tokens (and the
//! indentation of doc comment lines), so it can never change the meaning of
//! the code. It follows Intelephense's formatter: it re-indents lines,
//! normalizes spacing around operators, commas, keywords and parentheses
//! (extra spaces used for alignment are kept), places braces, puts each
//! statement on its own line, and puts each element of a list that already
//! spans several lines on its own line.
//!
//! ```
//! let edits = php_fmt::format("<?php\nif($a==1){\necho 'x' ;\n}\n", &php_fmt::FormatOptions::default()).unwrap();
//! assert_eq!(php_fmt::apply("<?php\nif($a==1){\necho 'x' ;\n}\n", &edits), "<?php\nif ($a == 1) {\n    echo 'x';\n}\n");
//! ```

use php_syntax::{PhpVersion, SyntaxKind, SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange, TextSize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BraceStyle {
    /// Class and function braces on their own line, control structures K&R.
    #[default]
    Psr12,
    /// All opening braces on the same line.
    KAndR,
    /// All opening braces on their own line.
    Allman,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOptions {
    pub indent_size: usize,
    pub use_tabs: bool,
    pub braces: BraceStyle,
    /// Maximum number of consecutive blank lines kept (unlimited by
    /// default, like Intelephense).
    pub max_blank_lines: usize,
    pub php_version: PhpVersion,
    /// Ensure the file ends with exactly one newline.
    pub insert_final_newline: bool,
    /// Spaces around the concatenation operator `.` (default, like PER and
    /// Intelephense); `None` keeps the existing style.
    pub space_around_concat: Option<bool>,
    /// Space after `!`; `None` keeps the existing style.
    pub space_after_not: Option<bool>,
}

impl Default for FormatOptions {
    fn default() -> Self {
        FormatOptions {
            indent_size: 4,
            use_tabs: false,
            braces: BraceStyle::Psr12,
            max_blank_lines: usize::MAX,
            php_version: PhpVersion::LATEST,
            insert_final_newline: true,
            space_around_concat: Some(true),
            space_after_not: None,
        }
    }
}

/// A whitespace replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: TextRange,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The file has syntax errors; formatting is refused to stay safe.
    SyntaxErrors,
}

/// Applies edits (sorted or not) to `text`.
pub fn apply(text: &str, edits: &[Edit]) -> String {
    let mut edits: Vec<&Edit> = edits.iter().collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
    let mut out = text.to_string();
    for e in edits {
        out.replace_range(usize::from(e.range.start())..usize::from(e.range.end()), &e.new_text);
    }
    out
}

/// Formats a whole document, returning minimal whitespace edits.
pub fn format(text: &str, options: &FormatOptions) -> Result<Vec<Edit>, FormatError> {
    let parse = php_syntax::parse(text, options.php_version);
    if !parse.errors().is_empty() {
        return Err(FormatError::SyntaxErrors);
    }
    let root = parse.syntax_node();
    let mut current = apply(text, &Formatter::new(text, &root, options).run());
    // moving braces changes the line structure the indentation is based on;
    // iterate to a fixed point (a few passes at most)
    for _ in 0..3 {
        let parse = php_syntax::parse(&current, options.php_version);
        let root = parse.syntax_node();
        let edits = Formatter::new(&current, &root, options).run();
        if edits.is_empty() {
            break;
        }
        current = apply(&current, &edits);
    }
    Ok(whitespace_diff(text, &current, options.php_version))
}

/// Edits turning `original` into `formatted`, which differ only in
/// whitespace between tokens (and inside re-indented comments).
fn whitespace_diff(original: &str, formatted: &str, version: PhpVersion) -> Vec<Edit> {
    let sig = |text: &str| -> Vec<(SyntaxKind, usize, usize)> {
        let (tokens, _) = php_syntax::tokenize(text, version);
        let mut pos = 0usize;
        let mut out = Vec::new();
        for t in tokens {
            let end = pos + t.len as usize;
            if t.kind != WHITESPACE {
                out.push((t.kind, pos, end));
            }
            pos = end;
        }
        out
    };
    let a = sig(original);
    let b = sig(formatted);
    let mut edits = Vec::new();
    let (mut pa, mut pb) = (0usize, 0usize);
    for ((_, sa, ea), (_, sb, eb)) in a.iter().zip(b.iter()) {
        if original[pa..*sa] != formatted[pb..*sb] {
            edits.push(Edit {
                range: TextRange::new((pa as u32).into(), (*sa as u32).into()),
                new_text: formatted[pb..*sb].to_string(),
            });
        }
        if original[*sa..*ea] != formatted[*sb..*eb] {
            edits.push(Edit {
                range: TextRange::new((*sa as u32).into(), (*ea as u32).into()),
                new_text: formatted[*sb..*eb].to_string(),
            });
        }
        pa = *ea;
        pb = *eb;
    }
    if original[pa..] != formatted[pb..] {
        edits.push(Edit {
            range: TextRange::new((pa as u32).into(), (original.len() as u32).into()),
            new_text: formatted[pb..].to_string(),
        });
    }
    edits
}

/// Formats only the lines touched by `range`.
pub fn format_range(text: &str, range: TextRange, options: &FormatOptions) -> Result<Vec<Edit>, FormatError> {
    let edits = format(text, options)?;
    Ok(edits
        .into_iter()
        .filter(|e| e.range.intersect(range).is_some() || range.contains_range(e.range))
        .collect())
}

struct Container {
    open: TextSize,
    open_end: TextSize,
    /// Start of the closing delimiter (or end of the region).
    close: TextSize,
    open_line: usize,
    /// The closing token belongs to this container (it is dedented).
    has_close_token: bool,
    /// Kind of the opening token (`{`, `(`, `[`, or `:` for case bodies).
    open_kind: SyntaxKind,
}

struct Formatter<'a> {
    text: &'a str,
    root: &'a SyntaxNode,
    opts: &'a FormatOptions,
    line_starts: Vec<usize>,
    containers: Vec<Container>,
    edits: Vec<Edit>,
    /// First non-whitespace token starting on each line.
    line_tokens: Vec<Option<SyntaxToken>>,
    /// Memoized new indentation depth per line.
    line_depth: std::cell::RefCell<Vec<Option<usize>>>,
    /// Memoized delimiters of lists and whether they expand.
    lists: std::cell::RefCell<std::collections::HashMap<TextRange, ListLayout>>,
}

type ListLayout = Option<(SyntaxToken, SyntaxToken, bool)>;

fn is_open(k: SyntaxKind) -> bool {
    matches!(k, L_CURLY | L_PAREN | L_BRACK)
}

fn closer_of(k: SyntaxKind) -> SyntaxKind {
    match k {
        L_CURLY => R_CURLY,
        L_PAREN => R_PAREN,
        _ => R_BRACK,
    }
}

/// The string node (interpolated string, heredoc, shell command) a token is part of.
fn string_node(token: &SyntaxToken) -> Option<SyntaxNode> {
    token.parent_ancestors().find(|a| matches!(a.kind(), INTERPOLATED_STRING | HEREDOC | SHELL_EXEC))
}

/// Whitespace between these tokens must be kept as is.
fn verbatim_between(prev: &SyntaxToken, next: &SyntaxToken) -> bool {
    if matches!(prev.kind(), INLINE_HTML | HALT_COMPILER_DATA)
        || matches!(next.kind(), INLINE_HTML | HALT_COMPILER_DATA)
    {
        return true;
    }
    match (string_node(prev), string_node(next)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

const CONTROL_KEYWORDS: &[SyntaxKind] =
    &[IF_KW, ELSEIF_KW, WHILE_KW, FOR_KW, FOREACH_KW, SWITCH_KW, CATCH_KW, MATCH_KW];

impl<'a> Formatter<'a> {
    fn new(text: &'a str, root: &'a SyntaxNode, opts: &'a FormatOptions) -> Formatter<'a> {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        let lines = line_starts.len();
        let mut f = Formatter {
            text,
            root,
            opts,
            line_starts,
            containers: Vec::new(),
            edits: Vec::new(),
            line_tokens: vec![None; lines],
            line_depth: std::cell::RefCell::new(vec![None; lines]),
            lists: Default::default(),
        };
        for t in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
            if t.kind() == WHITESPACE {
                continue;
            }
            let line = f.line_of(t.text_range().start());
            if f.line_tokens[line].is_none() {
                f.line_tokens[line] = Some(t);
            }
        }
        f.collect_containers();
        f
    }

    fn line_of(&self, offset: TextSize) -> usize {
        self.line_starts.partition_point(|&s| s <= usize::from(offset)) - 1
    }

    fn collect_containers(&mut self) {
        let mut containers = Vec::new();
        for node in self.root.descendants() {
            let kind = node.kind();
            if matches!(kind, INTERPOLATED_STRING | HEREDOC | SHELL_EXEC) {
                continue;
            }
            let tokens: Vec<SyntaxToken> =
                node.children_with_tokens().filter_map(|e| e.into_token()).collect();
            let mut stack: Vec<&SyntaxToken> = Vec::new();
            for t in &tokens {
                if is_open(t.kind()) {
                    stack.push(t);
                } else if matches!(t.kind(), R_CURLY | R_PAREN | R_BRACK) {
                    if let Some(open) = stack.pop() {
                        if closer_of(open.kind()) == t.kind() {
                            containers.push(Container {
                                open: open.text_range().start(),
                                open_end: open.text_range().end(),
                                close: t.text_range().start(),
                                open_line: self.line_of(open.text_range().start()),
                                has_close_token: true,
                                open_kind: open.kind(),
                            });
                        }
                    }
                }
            }
            for open in stack {
                containers.push(Container {
                    open: open.text_range().start(),
                    open_end: open.text_range().end(),
                    close: node.text_range().end(),
                    open_line: self.line_of(open.text_range().start()),
                    has_close_token: false,
                    open_kind: open.kind(),
                });
            }
            // `case x:` bodies and alternative syntax blocks
            match kind {
                CASE_CLAUSE | DEFAULT_CLAUSE => {
                    if let Some(colon) = tokens.iter().find(|t| matches!(t.kind(), COLON | SEMICOLON)) {
                        containers.push(Container {
                            open: colon.text_range().start(),
                            open_end: colon.text_range().end(),
                            close: node.text_range().end(),
                            open_line: self.line_of(colon.text_range().start()),
                            has_close_token: false,
                            open_kind: COLON,
                        });
                    }
                }
                STMT_LIST => {
                    if let Some(colon) = tokens.first().filter(|t| t.kind() == COLON) {
                        containers.push(Container {
                            open: colon.text_range().start(),
                            open_end: colon.text_range().end(),
                            close: node.text_range().end(),
                            open_line: self.line_of(colon.text_range().start()),
                            has_close_token: false,
                            open_kind: COLON,
                        });
                    }
                }
                SWITCH_BODY => {
                    if let Some(colon) = tokens.first().filter(|t| t.kind() == COLON) {
                        let end =
                            tokens.iter().find(|t| t.kind() == ENDSWITCH_KW).map(|t| t.text_range().start());
                        containers.push(Container {
                            open: colon.text_range().start(),
                            open_end: colon.text_range().end(),
                            close: end.unwrap_or(node.text_range().end()),
                            open_line: self.line_of(colon.text_range().start()),
                            has_close_token: end.is_some(),
                            open_kind: COLON,
                        });
                    }
                }
                _ => {}
            }
        }
        containers.sort_by_key(|c| c.open);
        self.containers = containers;
    }

    /// New indentation depth of a line (that of its first token).
    fn depth_of_line(&self, line: usize) -> usize {
        if let Some(d) = self.line_depth.borrow()[line] {
            return d;
        }
        let d = match &self.line_tokens[line] {
            Some(t) if self.line_of(t.text_range().start()) == line && self.starts_line(t) => self.depth(t),
            Some(t) => {
                // the line continues after a token of an earlier line
                let first = self.line_of(t.text_range().start());
                if first < line {
                    self.depth_of_line(first)
                } else {
                    self.depth_of_line_start_token(line)
                }
            }
            None => self.depth_of_line_start_token(line),
        };
        self.line_depth.borrow_mut()[line] = Some(d);
        d
    }

    /// Depth of a line whose first token does not start it (e.g. code after
    /// a multi-line string or comment).
    fn depth_of_line_start_token(&self, line: usize) -> usize {
        let offset = TextSize::from(self.line_starts[line] as u32);
        match self.root.token_at_offset(offset).right_biased() {
            Some(t) => {
                let l = self.line_of(t.text_range().start());
                if l < line {
                    self.depth_of_line(l)
                } else {
                    0
                }
            }
            None => 0,
        }
    }

    /// Whether only whitespace precedes `token` on its line.
    fn starts_line(&self, token: &SyntaxToken) -> bool {
        let start = usize::from(token.text_range().start());
        let line_start = self.text[..start].rfind('\n').map_or(0, |i| i + 1);
        self.text[line_start..start].trim().is_empty()
    }

    /// Indentation depth of a line starting with `token`: one level deeper
    /// than the line opening the innermost enclosing container.
    fn depth(&self, token: &SyntaxToken) -> usize {
        let pos = token.text_range().start();
        // a closing delimiter goes back to the indentation of its opener
        if let Some(c) = self.containers.iter().rev().find(|c| c.has_close_token && c.close == pos) {
            return self.depth_of_line(c.open_line);
        }
        let inner = self
            .containers
            .iter()
            .filter(|c| c.open_end <= pos && (pos < c.close || !c.has_close_token && pos <= c.close))
            .max_by_key(|c| c.open);
        let base = match inner {
            Some(c) => self.depth_of_line(c.open_line) + 1,
            None => 0,
        };
        // comments at the end of a `case` body (`// no break`)
        if matches!(token.kind(), COMMENT | DOC_COMMENT) {
            let next_sig = {
                let mut t = token.next_token();
                while let Some(tok) = &t {
                    if !matches!(tok.kind(), WHITESPACE | COMMENT | DOC_COMMENT) {
                        break;
                    }
                    t = tok.next_token();
                }
                t
            };
            // the previous token is directly in a `case` body
            let prev_in_case = Self::prev_significant(token).is_some_and(|p| {
                let at = p.text_range().start();
                self.containers
                    .iter()
                    .filter(|c| c.open_end <= at && at <= c.close)
                    .max_by_key(|c| c.open)
                    .is_some_and(|c| c.open_kind == COLON)
            });
            if prev_in_case && next_sig.is_some_and(|n| matches!(n.kind(), CASE_KW | DEFAULT_KW | R_CURLY)) {
                return base + 1;
            }
        }
        if !self.is_continuation(token) {
            return base;
        }
        let Some(c) = inner.filter(|c| matches!(c.open_kind, L_PAREN | L_BRACK)) else { return base + 1 };
        if matches!(token.kind(), ARROW | NULLSAFE_ARROW) {
            return base + 1;
        }
        let open = self.root.covering_element(TextRange::at(c.open, 1.into())).into_token();
        // inside a condition's (...) only method chains get an extra level
        if open.as_ref().is_some_and(|o| {
            o.parent().is_some_and(|p| p.kind() == PAREN_EXPR)
                || o.parent().and_then(|stmt| condition_open(&stmt)).as_ref() == Some(o)
        }) {
            return base;
        }
        // an expression starting right after `(` continues one level deeper
        // than that line, one starting on its own line one level deeper
        // than itself
        let mut first = open.and_then(|o| o.next_token());
        while let Some(t) = first.as_ref().filter(|t| matches!(t.kind(), WHITESPACE | COMMENT | DOC_COMMENT))
        {
            first = t.next_token();
        }
        if first.is_some_and(|f| self.line_of(f.text_range().start()) == c.open_line) {
            base
        } else {
            base + 1
        }
    }

    fn prev_significant(token: &SyntaxToken) -> Option<SyntaxToken> {
        let mut t = token.prev_token();
        while let Some(tok) = t {
            if !matches!(tok.kind(), WHITESPACE | COMMENT | DOC_COMMENT) {
                return Some(tok);
            }
            t = tok.prev_token();
        }
        None
    }

    fn is_continuation(&self, token: &SyntaxToken) -> bool {
        if matches!(token.kind(), R_CURLY | R_PAREN | R_BRACK | L_CURLY | HASH_BRACK | CLOSE_TAG | SEMICOLON)
        {
            return false;
        }
        if matches!(
            token.kind(),
            ELSE_KW
                | ELSEIF_KW
                | CATCH_KW
                | FINALLY_KW
                | ENDIF_KW
                | ENDWHILE_KW
                | ENDFOR_KW
                | ENDFOREACH_KW
                | ENDSWITCH_KW
                | ENDDECLARE_KW
                | CASE_KW
                | DEFAULT_KW
        ) && token.parent().is_some_and(|p| !matches!(p.kind(), MATCH_ARM))
        {
            return false;
        }
        let Some(prev) = Self::prev_significant(token) else { return false };
        // lists without delimiters: `use A,\n B;`, `implements A,\n B`
        if prev.kind() == COMMA {
            return prev.parent().is_some_and(|p| {
                matches!(
                    p.kind(),
                    TRAIT_USE
                        | IMPLEMENTS_CLAUSE
                        | EXTENDS_CLAUSE
                        | USE_DECL
                        | GLOBAL_STMT
                        | STATIC_STMT
                        | ECHO_STMT
                        | CONST_DECL
                        | CLASS_CONST_DECL
                        | PROPERTY_DECL
                )
            });
        }
        match prev.kind() {
            SEMICOLON | L_CURLY | R_CURLY | COMMA | L_PAREN | L_BRACK | OPEN_TAG | CLOSE_TAG
            | INLINE_HTML | OPEN_TAG_WITH_ECHO => false,
            // `case 1:`, `else:`, labels
            COLON => prev
                .parent()
                .is_some_and(|p| matches!(p.kind(), TERNARY_EXPR | RETURN_TYPE | ARG | ENUM_BACKING_TYPE)),
            // attributes before declarations
            R_BRACK => !prev.parent().is_some_and(|p| p.kind() == ATTRIBUTE_GROUP),
            // `else` / `do` without braces: the statement is indented
            _ => true,
        }
    }

    fn indent(&self, depth: usize) -> String {
        if self.opts.use_tabs {
            "\t".repeat(depth)
        } else {
            " ".repeat(depth * self.opts.indent_size)
        }
    }

    fn push(&mut self, start: TextSize, end: TextSize, new_text: String) {
        let range = TextRange::new(start, end);
        if self.text[range] != new_text {
            self.edits.push(Edit { range, new_text });
        }
    }

    fn run(mut self) -> Vec<Edit> {
        // templates mixing HTML and PHP are left alone
        let has_html = self.root.descendants_with_tokens().filter_map(|e| e.into_token()).any(|t| {
            t.kind() == INLINE_HTML
                && !(t.text_range().start() == TextSize::from(0) && t.text().trim().is_empty())
        });
        if has_html {
            return Vec::new();
        }
        let tokens: Vec<SyntaxToken> =
            self.root.descendants_with_tokens().filter_map(|e| e.into_token()).collect();
        let mut prev: Option<SyntaxToken> = None;
        let mut i = 0;
        while i < tokens.len() {
            let t = &tokens[i];
            if t.kind() == WHITESPACE {
                i += 1;
                continue;
            }
            let ws_start = prev.as_ref().map(|p| p.text_range().end()).unwrap_or(t.text_range().start());
            let ws_end = t.text_range().start();
            if let Some(p) = &prev {
                if !verbatim_between(p, t) {
                    let original = &self.text[usize::from(ws_start)..usize::from(ws_end)];
                    if let Some(new) =
                        self.separator(p, t, original).map(|new| self.after_line_comment(p, t, new))
                    {
                        self.push(ws_start, ws_end, new);
                    }
                }
            }
            // `<?php declare(...)`: the statement moves to its own line
            if t.kind() == OPEN_TAG && !t.text().ends_with('\n') && t.text().starts_with("<?php") {
                if let Some(next) = tokens[i + 1..].iter().find(|n| n.kind() != WHITESPACE) {
                    if self.blank_after_open_tag(next)
                        && self.line_of(next.text_range().start()) == self.line_of(t.text_range().start())
                    {
                        self.push(t.text_range().start(), t.text_range().end(), "<?php\n".into());
                    }
                }
            }
            // block comments other than doc comments are kept as written
            // (and `/**#@+` marker comments)
            if t.kind() == DOC_COMMENT && t.text()[3..].starts_with(char::is_whitespace) {
                self.reindent_comment(t);
            }
            prev = Some(t.clone());
            i += 1;
        }
        // trailing whitespace at the end of the file
        if let Some(p) = &prev {
            let end = TextSize::from(self.text.len() as u32);
            let start = p.text_range().end();
            if p.kind() != INLINE_HTML && p.kind() != HALT_COMPILER_DATA && self.opts.insert_final_newline {
                let new = if p.kind() == CLOSE_TAG && p.text().ends_with('\n') {
                    String::new()
                } else {
                    "\n".to_string()
                };
                self.push(start, end, new);
            }
        }
        self.edits
    }

    fn reindent_comment(&mut self, t: &SyntaxToken) {
        // the new indentation of the comment's first line
        let depth = self.depth(t);
        let indent = self.indent(depth);
        let start = usize::from(t.text_range().start());
        let text = t.text();
        let mut offset = 0;
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                let line_start = start + offset;
                let ws: usize = line.len() - line.trim_start_matches([' ', '\t']).len();
                let rest = &line[ws..];
                if rest.starts_with('*') {
                    self.push(
                        TextSize::from(line_start as u32),
                        TextSize::from((line_start + ws) as u32),
                        format!("{indent} "),
                    );
                }
            }
            offset += line.len() + 1;
        }
    }

    /// The whitespace between `prev` and `next`; `None` keeps the original.
    fn separator(&self, prev: &SyntaxToken, next: &SyntaxToken, original: &str) -> Option<String> {
        if prev.kind() == OPEN_TAG {
            // `<?php` already contains one whitespace character
            let newlines = original.matches('\n').count();
            let blank = self.blank_after_open_tag(next);
            if !prev.text().ends_with('\n') && newlines == 0 {
                // `<?php declare(...)`: the tag itself is rewritten in `run`
                return if blank {
                    Some("\n".into())
                } else if original.is_empty() {
                    None
                } else {
                    Some(String::new())
                };
            }
            let lines = newlines.min(self.opts.max_blank_lines).max(usize::from(blank));
            if lines == 0 {
                return if original.is_empty() { None } else { Some(String::new()) };
            }
            return Some("\n".repeat(lines) + &self.indent(self.depth(next)));
        }
        if next.kind() == CLOSE_TAG || prev.kind() == CLOSE_TAG {
            return None;
        }
        let newlines = original.matches('\n').count();
        // `;` ends the line of its statement
        if next.kind() == SEMICOLON
            && !matches!(prev.kind(), COMMENT | DOC_COMMENT)
            && next.parent().is_some_and(|p| p.first_token().as_ref() != Some(next) && p.kind() != FOR_STMT)
            && !(newlines > 0 && next.parent().is_some_and(|p| is_variable_call_chain(&p)))
        {
            return Some(String::new());
        }
        // empty declaration bodies: `function f() {}`, `class A {}`
        if self.opts.braces == BraceStyle::Psr12 {
            if next.kind() == L_CURLY && self.empty_decl_body(next) {
                return Some(" ".into());
            }
            if prev.kind() == L_CURLY && next.kind() == R_CURLY && self.empty_decl_body(prev) {
                return Some(String::new());
            }
        }
        // brace placement
        if next.kind() == L_CURLY {
            if let Some(own_line) = self.brace_on_own_line(next) {
                if own_line {
                    let decl = next.parent().and_then(|p| p.parent());
                    let depth = decl.and_then(|d| d.first_token()).map(|t| self.depth(&t)).unwrap_or(0);
                    return Some(format!("\n{}", self.indent(depth)));
                }
                return Some(" ".into());
            }
        }
        // `} else {`, `} catch`, `} while (...)`
        if prev.kind() == R_CURLY
            && matches!(next.kind(), ELSE_KW | ELSEIF_KW | CATCH_KW | FINALLY_KW)
            && self.opts.braces != BraceStyle::Allman
        {
            return Some(" ".into());
        }
        if prev.kind() == R_CURLY
            && next.kind() == WHILE_KW
            && next.parent().is_some_and(|p| p.kind() == DO_WHILE_STMT)
        {
            return Some(" ".into());
        }
        // line breaks the layout requires (a trailing comment stays put)
        let required = if newlines == 0 && matches!(next.kind(), COMMENT | DOC_COMMENT) {
            0
        } else {
            self.required_newlines(prev, next)
        };
        if newlines > 0 || required > 0 {
            let lines = newlines.min(self.opts.max_blank_lines.saturating_add(1)).max(required);
            // no blank lines before `}`
            let lines = if next.kind() == R_CURLY { 1 } else { lines };
            return Some("\n".repeat(lines) + &self.indent(self.depth(next)));
        }
        // trailing comments: at least one space
        if next.kind() == COMMENT || next.kind() == DOC_COMMENT {
            return Some(if original.is_empty() { " ".into() } else { original.to_string() });
        }
        if prev.kind() == COMMENT || prev.kind() == DOC_COMMENT {
            return Some(if original.is_empty() { String::new() } else { " ".into() });
        }
        // spaces beyond the required one (alignment) are kept
        let keep =
            !original.is_empty() && !(next.kind() == L_PAREN && CONTROL_KEYWORDS.contains(&prev.kind()));
        Some(match self.spaces(prev, next, original) {
            Some(0) => String::new(),
            _ if keep => original.to_string(),
            Some(n) => " ".repeat(n),
            None => String::new(),
        })
    }

    fn list_layout(&self, list: &SyntaxNode) -> ListLayout {
        if !matches!(list.kind(), ARG_LIST | PARAM_LIST | ARRAY_EXPR | LIST_EXPR | MATCH_EXPR) {
            return None;
        }
        if let Some(l) = self.lists.borrow().get(&list.text_range()) {
            return l.clone();
        }
        let layout = list_delimiters(list).map(|(open, close)| {
            let expands = list_expands(self.text, list, &open, &close);
            (open, close, expands)
        });
        self.lists.borrow_mut().insert(list.text_range(), layout.clone());
        layout
    }

    /// A line comment always ends its line.
    fn after_line_comment(&self, prev: &SyntaxToken, next: &SyntaxToken, new: String) -> String {
        let line_comment = prev.kind() == COMMENT && !prev.text().starts_with("/*");
        if line_comment && !new.contains('\n') {
            format!("\n{}", self.indent(self.depth(next)))
        } else {
            new
        }
    }

    /// `<?php` is followed by a blank line before these.
    fn blank_after_open_tag(&self, next: &SyntaxToken) -> bool {
        next.kind() == DOC_COMMENT
            || statement_starting(next)
                .is_some_and(|s| matches!(s.kind(), NAMESPACE_DECL | DECLARE_STMT | USE_DECL))
    }

    /// Whether `brace` opens an empty function or class body.
    fn empty_decl_body(&self, brace: &SyntaxToken) -> bool {
        let Some(body) = brace.parent() else { return false };
        let Some(owner) = body.parent() else { return false };
        let decl = match body.kind() {
            MEMBER_LIST => {
                matches!(owner.kind(), CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL | ANON_CLASS)
            }
            BLOCK => matches!(owner.kind(), FUNCTION_DECL | METHOD_DECL | CLOSURE_EXPR),
            _ => false,
        };
        decl && body.children_with_tokens().all(|e| matches!(e.kind(), L_CURLY | R_CURLY | WHITESPACE))
    }

    /// Line breaks required between `prev` and `next`: 0, 1, or 2 for a
    /// blank line.
    fn required_newlines(&self, prev: &SyntaxToken, next: &SyntaxToken) -> usize {
        // statements and class members on their own lines
        if let (Some(a), Some(b)) = (statement_ending(prev), statement_starting(next)) {
            // `;;`
            if b.kind() == EMPTY_STMT && newlines_in(self.text, prev, next) == 0 {
                return 0;
            }
            if a.parent() == b.parent() {
                let blank = matches!(b.kind(), NAMESPACE_DECL)
                    || a.kind() == NAMESPACE_DECL && prev.kind() == SEMICOLON
                    || a.kind() == DECLARE_STMT && prev.kind() == SEMICOLON
                    || (a.kind() == USE_DECL) != (b.kind() == USE_DECL);
                return if blank { 2 } else { 1 };
            }
        }
        // the `use` block ends before a doc comment
        if next.kind() == DOC_COMMENT && statement_ending(prev).is_some_and(|a| a.kind() == USE_DECL) {
            return 2;
        }
        // `/** doc */` before `namespace`, `namespace A {` before `use`
        if let Some(b) = statement_starting(next) {
            if prev.kind() == DOC_COMMENT && b.kind() == NAMESPACE_DECL {
                return 2;
            }
            if prev.kind() == L_CURLY
                && b.kind() == USE_DECL
                && prev.parent().and_then(|p| p.parent()).is_some_and(|p| p.kind() == NAMESPACE_DECL)
            {
                return 2;
            }
        }
        // blocks span several lines
        if prev.kind() == L_CURLY && is_block(prev) {
            return usize::from(next.kind() != R_CURLY || !self.empty_decl_body(prev));
        }
        if next.kind() == R_CURLY && is_block(next) {
            return 1;
        }
        // lists that already span several lines: one element per line
        if let Some(list) =
            prev.parent().filter(|_| matches!(prev.kind(), COMMA | L_PAREN | L_BRACK | L_CURLY))
        {
            if let Some((open, close, expands)) = self.list_layout(&list) {
                let delimiter = prev.kind() == COMMA || *prev == open;
                if delimiter && *next != close && expands {
                    return 1;
                }
                if *prev == open && *next == close && list.kind() == MATCH_EXPR {
                    return 1;
                }
            }
        }
        if let Some(list) = next.parent().filter(|_| matches!(next.kind(), R_PAREN | R_BRACK | R_CURLY)) {
            if let Some((open, close, expands)) = self.list_layout(&list) {
                if *next == close && *prev != open && expands {
                    return 1;
                }
            }
        }
        // multi-line conditions: `if (\n    $a &&\n    $b\n) {`
        if let Some(stmt) = prev.parent().filter(|_| prev.kind() == L_PAREN) {
            if let Some(close) = condition_close(&stmt, prev) {
                // `if ((...) ||` keeps the parentheses together
                if *next != close && next.kind() != L_PAREN && top_level_newline(prev, &close) {
                    return 1;
                }
            }
        }
        if next.kind() == R_PAREN {
            if let Some(stmt) = next.parent() {
                if let Some(open) = condition_open(&stmt) {
                    if condition_close(&stmt, &open).as_ref() == Some(next)
                        && *prev != open
                        && top_level_newline(&open, next)
                    {
                        return 1;
                    }
                }
            }
        }
        0
    }

    /// `Some(true)` if this opening brace must be on its own line,
    /// `Some(false)` if it must be on the same line, `None` to keep.
    fn brace_on_own_line(&self, brace: &SyntaxToken) -> Option<bool> {
        let body = brace.parent()?;
        let owner = body.parent()?;
        let is_decl_body = match (body.kind(), owner.kind()) {
            (MEMBER_LIST, CLASS_DECL | INTERFACE_DECL | TRAIT_DECL | ENUM_DECL) => true,
            (BLOCK, FUNCTION_DECL | METHOD_DECL) => true,
            (MEMBER_LIST, ANON_CLASS) => return None,
            (BLOCK, CLOSURE_EXPR) => false,
            (
                BLOCK,
                IF_STMT | ELSE_IF_CLAUSE | ELSE_CLAUSE | WHILE_STMT | DO_WHILE_STMT | FOR_STMT | FOREACH_STMT
                | TRY_STMT | CATCH_CLAUSE | FINALLY_CLAUSE | DECLARE_STMT,
            ) => false,
            (SWITCH_BODY, _) | (_, MATCH_EXPR) => false,
            _ => return None,
        };
        match self.opts.braces {
            BraceStyle::Allman => Some(true),
            BraceStyle::KAndR => Some(false),
            BraceStyle::Psr12 => {
                if !is_decl_body {
                    return Some(false);
                }
                // multi-line parameter lists with `)` on its own line: `) {`
                let params = owner.children().find(|c| c.kind() == PARAM_LIST);
                if let Some(p) = params.filter(|p| self.text[p.text_range()].contains('\n')) {
                    if let Some(close) = p.last_token().filter(|t| t.kind() == R_PAREN) {
                        if self.starts_line(&close) {
                            return Some(false);
                        }
                    }
                }
                // anonymous / one-line declarations: `{}` directly after keep
                Some(true)
            }
        }
    }

    /// Number of spaces between two tokens on the same line; `None` = keep
    /// a single space if there was whitespace.
    fn spaces(&self, prev: &SyntaxToken, next: &SyntaxToken, original: &str) -> Option<usize> {
        let p = prev.kind();
        let n = next.kind();
        let pp = prev.parent().map(|x| x.kind());
        let np = next.parent().map(|x| x.kind());
        // never touch the inside of names, strings or casts
        if matches!(p, BACKSLASH)
            || matches!(n, BACKSLASH) && np == Some(QUALIFIED_NAME) && pp == Some(QUALIFIED_NAME)
        {
            return Some(0);
        }
        match (p, n) {
            (_, COMMA) | (_, SEMICOLON) => return Some(0),
            (COMMA, R_PAREN | R_BRACK) => return Some(0),
            (COMMA, _) => return Some(1),
            (L_PAREN | L_BRACK | HASH_BRACK, _) => return Some(0),
            (_, R_PAREN | R_BRACK) => return Some(0),
            (ARROW | NULLSAFE_ARROW | COLON2 | DOLLAR | DOT3 | DOLLAR_CURLY, _) => return Some(0),
            (_, ARROW | NULLSAFE_ARROW | COLON2) => return Some(0),
            (CAST, _) => return None,
            (SEMICOLON, _) if pp == Some(FOR_STMT) => return Some(1),
            (_, L_BRACK) if !matches!(np, Some(ARRAY_EXPR | LIST_EXPR)) => return Some(0),
            _ => {}
        }
        // `?` of nullable types
        if p == QUESTION && pp == Some(NULLABLE_TYPE) {
            return Some(0);
        }
        // `&` in parameters / by-reference, `|` and `&` in types
        if p == AMP
            && matches!(
                pp,
                Some(
                    PARAM
                        | CLOSURE_USE_VAR
                        | ARRAY_ELEMENT
                        | FOREACH_VALUE
                        | FUNCTION_DECL
                        | METHOD_DECL
                        | CLOSURE_EXPR
                        | ARROW_FN_EXPR
                        | ASSIGN_EXPR
                        | PREFIX_EXPR
                )
            )
        {
            if pp == Some(ASSIGN_EXPR) {
                return Some(0);
            }
            return Some(0);
        }
        if p == PIPE && pp == Some(CATCH_TYPES) || n == PIPE && np == Some(CATCH_TYPES) {
            return Some(1);
        }
        if matches!(p, PIPE | AMP) && matches!(pp, Some(UNION_TYPE | INTERSECTION_TYPE))
            || matches!(n, PIPE | AMP) && matches!(np, Some(UNION_TYPE | INTERSECTION_TYPE))
        {
            return None;
        }
        // prefix operators
        if p == BANG && is_prefix_op(prev) {
            return match self.opts.space_after_not {
                Some(true) => Some(1),
                Some(false) => Some(0),
                None => Some(if original.is_empty() { 0 } else { 1 }),
            };
        }
        if pp == Some(PREFIX_EXPR) && is_prefix_op(prev) {
            return Some(0);
        }
        if (p == DOT && pp == Some(BIN_EXPR)) || (n == DOT && np == Some(BIN_EXPR)) {
            return match self.opts.space_around_concat {
                Some(true) => Some(1),
                Some(false) => Some(0),
                None => Some(if original.is_empty() { 0 } else { 1 }),
            };
        }
        if np == Some(POSTFIX_EXPR) && matches!(n, PLUS2 | MINUS2) {
            return Some(0);
        }
        // `(` after names and keywords
        if n == L_PAREN && is_binary_op(prev) {
            return Some(1);
        }
        if n == L_PAREN {
            return Some(match np {
                Some(ARG_LIST) => {
                    // `new class(...)`, calls
                    0
                }
                Some(PARAM_LIST) => {
                    if p == FUNCTION_KW
                        || matches!(pp, Some(CLOSURE_EXPR | ARROW_FN_EXPR)) && matches!(p, AMP)
                    {
                        1
                    } else {
                        0
                    }
                }
                Some(CLOSURE_USE) => 1,
                _ if CONTROL_KEYWORDS.contains(&p) => 1,
                _ if matches!(
                    p,
                    ARRAY_KW
                        | LIST_KW
                        | ISSET_KW
                        | EMPTY_KW
                        | EVAL_KW
                        | EXIT_KW
                        | DIE_KW
                        | UNSET_KW
                        | DECLARE_KW
                        | PRINT_KW
                ) =>
                {
                    0
                }
                _ if p.is_keyword() => 1,
                Some(PAREN_TYPE) => 0,
                _ if matches!(p, IDENT | VARIABLE | R_PAREN | R_BRACK | STRING) => 0,
                _ => return None,
            });
        }
        if p == R_PAREN && n == L_CURLY {
            return Some(1);
        }
        // return types `): int`, enum backing types, named arguments, labels
        if n == COLON {
            return Some(match np {
                Some(RETURN_TYPE | ENUM_BACKING_TYPE | ARG | LABEL_STMT | CASE_CLAUSE | DEFAULT_CLAUSE) => 0,
                // alternative syntax `if (...):` keeps its style
                Some(
                    STMT_LIST | SWITCH_BODY | ELSE_IF_CLAUSE | ELSE_CLAUSE | IF_STMT | WHILE_STMT | FOR_STMT
                    | FOREACH_STMT | DECLARE_STMT,
                ) => return None,
                Some(TERNARY_EXPR) if p == QUESTION => 0,
                Some(TERNARY_EXPR) => 1,
                _ => return None,
            });
        }
        if p == COLON {
            return Some(match pp {
                Some(RETURN_TYPE | ENUM_BACKING_TYPE | ARG | TERNARY_EXPR) => 1,
                _ => return None,
            });
        }
        // `declare(strict_types=1)`
        if (p == EQ || n == EQ) && (pp == Some(DECLARE_DIRECTIVE) || np == Some(DECLARE_DIRECTIVE)) {
            return Some(0);
        }
        // binary / assignment operators
        if is_binary_op(prev) || is_binary_op(next) {
            return Some(1);
        }
        if p == L_CURLY && pp == Some(USE_GROUP) || n == R_CURLY && np == Some(USE_GROUP) {
            return Some(0);
        }
        // `$this->{$name}`, `${'x'}`
        let plain = |kind: Option<SyntaxKind>| !matches!(kind, Some(PROPERTY_HOOK_LIST | MATCH_EXPR));
        if (p == L_CURLY && plain(pp) && !is_block(prev) || n == R_CURLY && plain(np) && !is_block(next))
            && !matches!(pp, Some(MATCH_EXPR))
            && !matches!(np, Some(MATCH_EXPR))
        {
            return Some(0);
        }
        if p == L_CURLY || n == R_CURLY {
            // `{ }` on one line: `{}` stays, otherwise one space
            return Some(if p == L_CURLY && n == R_CURLY { 0 } else { 1 });
        }
        if n == L_CURLY {
            return Some(if matches!(p, ARROW | NULLSAFE_ARROW | COLON2 | DOLLAR) { 0 } else { 1 });
        }
        if p.is_keyword() || n.is_keyword() {
            return Some(1);
        }
        None
    }
}

/// Whether a line break between `open` and `close` is outside nested
/// brackets.
fn top_level_newline(open: &SyntaxToken, close: &SyntaxToken) -> bool {
    let mut depth = 0i32;
    let mut t = open.next_token();
    while let Some(tok) = t {
        if tok == *close {
            return false;
        }
        match tok.kind() {
            L_PAREN | L_BRACK | L_CURLY | HASH_BRACK | DOLLAR_CURLY => depth += 1,
            R_PAREN | R_BRACK | R_CURLY => depth -= 1,
            WHITESPACE if depth == 0 && tok.text().contains('\n') => return true,
            _ => {}
        }
        t = tok.next_token();
    }
    false
}

/// `$x->a()\n    ->b()\n;`: a statement calling a chain of methods on a
/// variable, which keeps its `;` on its own line.
fn is_variable_call_chain(stmt: &SyntaxNode) -> bool {
    if stmt.kind() != EXPR_STMT {
        return false;
    }
    let Some(mut e) = stmt.first_child() else { return false };
    let mut calls = 0;
    while e.kind() == METHOD_CALL_EXPR {
        calls += 1;
        let Some(receiver) = e.first_child() else { return false };
        e = receiver;
    }
    calls >= 2 && e.kind() == VARIABLE_EXPR
}

fn newlines_in(text: &str, prev: &SyntaxToken, next: &SyntaxToken) -> usize {
    text[TextRange::new(prev.text_range().end(), next.text_range().start())].matches('\n').count()
}

/// Nodes whose children are statements (or class members).
fn is_statement_list(k: SyntaxKind) -> bool {
    matches!(k, SOURCE_FILE | BLOCK | STMT_LIST | MEMBER_LIST | CASE_CLAUSE | DEFAULT_CLAUSE)
}

/// The statement (or member) `token` is the last token of.
fn statement_ending(token: &SyntaxToken) -> Option<SyntaxNode> {
    token
        .parent_ancestors()
        .take_while(|n| n.last_token().as_ref() == Some(token))
        .find(|n| n.parent().is_some_and(|p| is_statement_list(p.kind())))
}

/// The statement (or member) `token` is the first token of.
fn statement_starting(token: &SyntaxToken) -> Option<SyntaxNode> {
    token
        .parent_ancestors()
        .take_while(|n| n.first_token().as_ref() == Some(token))
        .find(|n| n.parent().is_some_and(|p| is_statement_list(p.kind())))
}

/// Whether this `{` / `}` delimits a statement block or a class body.
fn is_block(brace: &SyntaxToken) -> bool {
    let Some(body) = brace.parent() else { return false };
    match body.kind() {
        MEMBER_LIST | SWITCH_BODY => true,
        BLOCK => body.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                FUNCTION_DECL
                    | METHOD_DECL
                    | CLOSURE_EXPR
                    | IF_STMT
                    | ELSE_IF_CLAUSE
                    | ELSE_CLAUSE
                    | WHILE_STMT
                    | DO_WHILE_STMT
                    | FOR_STMT
                    | FOREACH_STMT
                    | TRY_STMT
                    | CATCH_CLAUSE
                    | FINALLY_CLAUSE
                    | DECLARE_STMT
                    | NAMESPACE_DECL
            )
        }),
        _ => false,
    }
}

/// The opening and closing delimiters of an argument, parameter or array
/// list, or of the arms of a `match`.
fn list_delimiters(list: &SyntaxNode) -> Option<(SyntaxToken, SyntaxToken)> {
    let (open, close) = match list.kind() {
        ARG_LIST | PARAM_LIST => (L_PAREN, R_PAREN),
        ARRAY_EXPR | LIST_EXPR => {
            let first = list
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| matches!(t.kind(), L_PAREN | L_BRACK))?;
            (first.kind(), closer_of(first.kind()))
        }
        MATCH_EXPR => (L_CURLY, R_CURLY),
        _ => return None,
    };
    let tokens: Vec<SyntaxToken> = list.children_with_tokens().filter_map(|e| e.into_token()).collect();
    let o = tokens.iter().find(|t| t.kind() == open)?.clone();
    let c = tokens.iter().rev().find(|t| t.kind() == close)?.clone();
    (o.text_range().end() <= c.text_range().start()).then_some((o, c))
}

/// Whether a list has a line break between its elements (`match` arms are
/// always on their own lines).
fn list_expands(text: &str, list: &SyntaxNode, open: &SyntaxToken, close: &SyntaxToken) -> bool {
    if list.kind() == MATCH_EXPR {
        return true;
    }
    let inner = TextRange::new(open.text_range().end(), close.text_range().start());
    let elements: Vec<SyntaxNode> =
        list.children().filter(|c| inner.contains_range(c.text_range())).collect();
    if elements.is_empty() {
        return false;
    }
    // the text between two elements (or an element and a delimiter),
    // including commas and comments
    let gap = |from: TextSize, to: TextSize| text[TextRange::new(from, to)].contains('\n');
    let mut from = open.text_range().end();
    for e in &elements {
        if gap(from, e.text_range().start()) {
            return true;
        }
        from = e.text_range().end();
    }
    gap(from, close.text_range().start())
}

/// The `(` of a control structure's condition.
fn condition_open(stmt: &SyntaxNode) -> Option<SyntaxToken> {
    if !matches!(stmt.kind(), IF_STMT | ELSE_IF_CLAUSE | WHILE_STMT | DO_WHILE_STMT | SWITCH_STMT) {
        return None;
    }
    stmt.children_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == L_PAREN)
}

/// The `)` matching the condition's `open`.
fn condition_close(stmt: &SyntaxNode, open: &SyntaxToken) -> Option<SyntaxToken> {
    if condition_open(stmt).as_ref() != Some(open) {
        return None;
    }
    stmt.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == R_PAREN && t.text_range().start() >= open.text_range().end())
}

fn is_prefix_op(t: &SyntaxToken) -> bool {
    matches!(t.kind(), BANG | MINUS | PLUS | TILDE | AT | PLUS2 | MINUS2 | AMP)
        && t.parent().is_some_and(|p| p.kind() == PREFIX_EXPR && p.first_token().as_ref() == Some(t))
}

fn is_binary_op(t: &SyntaxToken) -> bool {
    let Some(parent) = t.parent() else { return false };
    match parent.kind() {
        BIN_EXPR | INSTANCEOF_EXPR => {
            // the operator is the first token after the lhs operand
            let k = t.kind();
            k != L_PAREN && k != R_PAREN && !k.is_trivia() && !matches!(k, VARIABLE | IDENT)
        }
        ASSIGN_EXPR => t.kind().is_assign_op(),
        ARRAY_ELEMENT | MATCH_ARM | YIELD_EXPR => t.kind() == FAT_ARROW,
        ARROW_FN_EXPR => t.kind() == FAT_ARROW,
        FOREACH_STMT => t.kind() == FAT_ARROW || t.kind() == AS_KW,
        PARAM | PROPERTY_ELEMENT | CONST_ELEMENT | STATIC_VAR | ENUM_CASE => t.kind() == EQ,
        TERNARY_EXPR => matches!(t.kind(), QUESTION),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(text: &str) -> String {
        let edits = format(text, &FormatOptions::default()).expect("no syntax errors");
        apply(text, &edits)
    }

    #[test]
    fn psr12_class() {
        let input = "<?php\nnamespace App;\nuse A\\B;\nclass Foo extends Bar implements Baz{\nconst X=1;\npublic function __construct(private int $a=0,?string $b=null){\n$this->a=$a+1;\n}\npublic static function make() : static{\nreturn new static();\n}\n}\n";
        let expected = "<?php\n\nnamespace App;\n\nuse A\\B;\n\nclass Foo extends Bar implements Baz\n{\n    const X = 1;\n    public function __construct(private int $a = 0, ?string $b = null)\n    {\n        $this->a = $a + 1;\n    }\n    public static function make(): static\n    {\n        return new static();\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn control_structures() {
        let input = "<?php\nfunction f($x)\n{\nif($x>1){\nreturn  1 ;\n}\nelse if ($x){\nfoo( $x , [1,2] );\n}else{\nforeach($x as $k=>$v){echo $k.$v;}\n}\nswitch($x){\ncase 1:\nbreak;\ndefault:\nreturn;\n}\ntry{\n}catch(A|B $e){\n}\nwhile(true):\nbreak;\nendwhile;\n}\n";
        let expected = "<?php\nfunction f($x)\n{\n    if ($x > 1) {\n        return  1;\n    } else if ($x) {\n        foo($x, [1, 2]);\n    } else {\n        foreach ($x as $k => $v) {\n            echo $k . $v;\n        }\n    }\n    switch ($x) {\n        case 1:\n            break;\n        default:\n            return;\n    }\n    try {\n    } catch (A | B $e) {\n    }\n    while (true):\n        break;\n    endwhile;\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn arrays_closures_and_chains() {
        let input = "<?php\n$a = [\n'x'=>1,\n'y'=>[\n2,\n],\n];\n$f=function($x)use(&$y){ return !$x; };\n$g = fn($x)=>$x*2;\n$r = $q\n->where('a', 1)\n->get();\nfoo([\n1,\n]);\n$m = match($x){\n1,2=>'a',\ndefault=>'b',\n};\n";
        let expected = "<?php\n$a = [\n    'x' => 1,\n    'y' => [\n        2,\n    ],\n];\n$f = function ($x) use (&$y) {\n    return !$x;\n};\n$g = fn($x) => $x * 2;\n$r = $q\n    ->where('a', 1)\n    ->get();\nfoo([\n    1,\n]);\n$m = match ($x) {\n    1, 2 => 'a',\n    default => 'b',\n};\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn keeps_strings_comments_and_heredoc() {
        let input = "<?php\n$s = \"a  {$b->c}  d\";\n$h = <<<EOT\n  keep   this\n  EOT;\n/**\n     * Doc\n     */\nfunction f() {} // trailing\n\n\n\n$x = 1;   # hash\ndeclare(strict_types = 1);\n";
        let expected = "<?php\n$s = \"a  {$b->c}  d\";\n$h = <<<EOT\n  keep   this\n  EOT;\n/**\n * Doc\n */\nfunction f() {} // trailing\n\n\n\n$x = 1;   # hash\ndeclare(strict_types=1);\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn idempotent_and_refuses_broken_code() {
        let input =
            "<?php\nclass A\n{\n    public function f(): void\n    {\n        $x = [1, 2];\n    }\n}\n";
        assert_eq!(fmt(input), input);
        assert_eq!(format("<?php function (", &FormatOptions::default()), Err(FormatError::SyntaxErrors));
    }
}
