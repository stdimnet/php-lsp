//! Selection ranges ("expand selection").

use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange, TextSize, TokenAtOffset};

pub(crate) fn selection_ranges(root: &SyntaxNode, offset: TextSize) -> Vec<TextRange> {
    let token = match root.token_at_offset(offset) {
        TokenAtOffset::None => return vec![TextRange::empty(offset)],
        TokenAtOffset::Single(t) => t,
        TokenAtOffset::Between(l, r) => {
            if r.kind().is_trivia() && !l.kind().is_trivia() {
                l
            } else {
                r
            }
        }
    };
    let mut ranges = Vec::new();
    let mut push = |r: TextRange| {
        if ranges.last() != Some(&r) {
            ranges.push(r);
        }
    };
    // Inside string literals first select the contents.
    if matches!(token.kind(), STRING | ENCAPSED_TEXT) {
        let r = token.text_range();
        let text = token.text();
        if token.kind() == STRING && text.len() >= 2 {
            push(TextRange::new(r.start() + TextSize::from(1), r.end() - TextSize::from(1)));
        }
    }
    if !token.kind().is_trivia() || token.kind() == DOC_COMMENT || token.kind() == COMMENT {
        push(token.text_range());
    }
    let mut node = token.parent();
    while let Some(n) = node {
        push(n.text_range());
        node = n.parent();
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_outward() {
        let text = "<?php function f() { return $a + 'xy'; }";
        let root = php_syntax::parse(text, php_syntax::PhpVersion::LATEST).syntax_node();
        let offset = TextSize::from(text.find("xy").unwrap() as u32);
        let ranges: Vec<&str> = selection_ranges(&root, offset).into_iter().map(|r| &text[r]).collect();
        assert_eq!(ranges[0], "xy");
        assert_eq!(ranges[1], "'xy'");
        assert!(ranges.contains(&"$a + 'xy'"));
        assert_eq!(*ranges.last().unwrap(), text);
    }
}
