//! Lossless, error tolerant PHP syntax tree.
//!
//! ```
//! use php_syntax::{parse, PhpVersion};
//!
//! let parse = parse("<?php function hello() { echo 'hi'; }", PhpVersion::LATEST);
//! assert!(parse.errors().is_empty());
//! assert_eq!(parse.syntax_node().text().to_string(), "<?php function hello() { echo 'hi'; }");
//! ```

mod grammar;
mod lexer;
mod parser;
mod syntax_kind;
mod version;

pub mod ast;

use std::fmt::Write;

pub use lexer::{tokenize, tokenize_from, LexError, Token};
pub use rowan::{Direction, NodeOrToken, TextRange, TextSize, TokenAtOffset, WalkEvent};
pub use syntax_kind::SyntaxKind;
pub use version::PhpVersion;

/// rowan language tag for PHP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PhpLanguage {}

impl rowan::Language for PhpLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        SyntaxKind::from(raw.0)
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind.into())
    }
}

pub type SyntaxNode = rowan::SyntaxNode<PhpLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<PhpLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<PhpLanguage>;
pub type SyntaxNodeChildren = rowan::SyntaxNodeChildren<PhpLanguage>;
pub type GreenNode = rowan::GreenNode;

/// A syntax (or lexical) error.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SyntaxError {
    pub message: String,
    pub range: TextRange,
}

/// The result of parsing: a syntax tree that always covers the whole input,
/// plus the errors encountered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    green: GreenNode,
    errors: Vec<SyntaxError>,
}

impl Parse {
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    pub fn syntax_node(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn tree(&self) -> ast::SourceFile {
        use ast::AstNode;
        ast::SourceFile::cast(self.syntax_node()).expect("root is always SOURCE_FILE")
    }

    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    /// Debug representation of the tree (and errors), used in tests.
    pub fn debug_tree(&self) -> String {
        let mut out = debug_tree(&self.syntax_node());
        for e in &self.errors {
            let _ = writeln!(out, "error {:?}: {}", e.range, e.message);
        }
        out
    }
}

/// Parses PHP source text.
pub fn parse(text: &str, version: PhpVersion) -> Parse {
    parse_from(text, version, false)
}

/// Parses PHP code without an open tag (the text starts in PHP mode), e.g.
/// code generated from templates.
pub fn parse_php_code(text: &str, version: PhpVersion) -> Parse {
    parse_from(text, version, true)
}

fn parse_from(text: &str, version: PhpVersion, php_code: bool) -> Parse {
    let (tokens, lex_errors) = tokenize_from(text, version, php_code);
    let mut p = parser::Parser::new(text, &tokens, version);
    grammar::source_file(&mut p);
    let (green, parse_errors) = p.finish();
    let mut errors: Vec<SyntaxError> = lex_errors
        .into_iter()
        .map(|e| SyntaxError { message: e.msg, range: TextRange::at(e.offset.into(), e.len.into()) })
        .collect();
    errors.extend(parse_errors);
    errors.sort_by_key(|e| e.range.start());
    Parse { green, errors }
}

pub fn debug_tree(node: &SyntaxNode) -> String {
    let mut out = String::new();
    let mut indent = 0;
    for event in node.preorder_with_tokens() {
        match event {
            WalkEvent::Enter(element) => {
                let _ = write!(out, "{:indent$}", "", indent = indent);
                match element {
                    NodeOrToken::Node(n) => {
                        let _ = writeln!(out, "{:?}@{:?}", n.kind(), n.text_range());
                    }
                    NodeOrToken::Token(t) => {
                        let _ = writeln!(out, "{:?}@{:?} {:?}", t.kind(), t.text_range(), t.text());
                    }
                }
                indent += 2;
            }
            WalkEvent::Leave(_) => indent -= 2,
        }
    }
    out
}
