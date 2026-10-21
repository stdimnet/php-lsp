//! Folding ranges.

use php_syntax::{NodeOrToken, SyntaxKind, SyntaxKind::*, SyntaxNode, SyntaxToken, TextRange, TextSize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoldKind {
    /// Code block, `range` spans from the opening to the closing delimiter
    /// (exclusive), so the closing line stays visible.
    Block,
    Comment,
    Imports,
    /// `#region` ... `#endregion`
    Region,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fold {
    pub range: TextRange,
    pub kind: FoldKind,
}

pub(crate) fn folding_ranges(root: &SyntaxNode, text: &str) -> Vec<Fold> {
    let mut folds = Vec::new();
    let mut regions: Vec<TextSize> = Vec::new();
    let mut line_comments: Option<(TextRange, u32)> = None; // range, last line
    let line_of = |offset: TextSize| text[..usize::from(offset)].matches('\n').count() as u32;
    let multiline = |range: TextRange| text[range].contains('\n');

    for element in root.descendants_with_tokens() {
        match element {
            NodeOrToken::Node(node) => match node.kind() {
                BLOCK
                | MEMBER_LIST
                | SWITCH_BODY
                | MATCH_EXPR
                | ARRAY_EXPR
                | LIST_EXPR
                | ARG_LIST
                | PARAM_LIST
                | USE_GROUP
                | PROPERTY_HOOK_LIST
                | TRAIT_ADAPTATION_LIST
                | CLOSURE_USE => {
                    if let Some(range) = delimited_inner(&node) {
                        if multiline(range) {
                            folds.push(Fold { range, kind: FoldKind::Block });
                        }
                    }
                }
                STMT_LIST | CASE_CLAUSE | DEFAULT_CLAUSE | HEREDOC => {
                    let range = node.text_range();
                    if multiline(range) {
                        folds.push(Fold { range, kind: FoldKind::Block });
                    }
                }
                SOURCE_FILE | NAMESPACE_DECL => imports(&node, text, &mut folds),
                _ => {}
            },
            NodeOrToken::Token(token) => {
                let kind = token.kind();
                if kind == DOC_COMMENT || (kind == COMMENT && token.text().starts_with("/*")) {
                    if multiline(token.text_range()) {
                        folds.push(Fold { range: token.text_range(), kind: FoldKind::Comment });
                    }
                    continue;
                }
                if kind != COMMENT {
                    if kind != WHITESPACE {
                        flush_comments(&mut line_comments, &mut folds);
                    }
                    continue;
                }
                let body = token.text().trim_start_matches(['/', '#']).trim_start();
                if body.starts_with("#region")
                    || body.starts_with("region")
                    || token.text().starts_with("#region")
                {
                    flush_comments(&mut line_comments, &mut folds);
                    regions.push(token.text_range().start());
                    continue;
                }
                if body.starts_with("#endregion")
                    || body.starts_with("endregion")
                    || token.text().starts_with("#endregion")
                {
                    flush_comments(&mut line_comments, &mut folds);
                    if let Some(start) = regions.pop() {
                        folds.push(Fold {
                            range: TextRange::new(start, token.text_range().end()),
                            kind: FoldKind::Region,
                        });
                    }
                    continue;
                }
                let line = line_of(token.text_range().start());
                match &mut line_comments {
                    Some((range, last)) if *last + 1 == line && only_ws_between(&token, *range) => {
                        *range = range.cover(token.text_range());
                        *last = line;
                    }
                    _ => {
                        flush_comments(&mut line_comments, &mut folds);
                        line_comments = Some((token.text_range(), line));
                    }
                }
            }
        }
    }
    flush_comments(&mut line_comments, &mut folds);
    folds.retain(|f| multiline(f.range));
    folds.sort_by_key(|f| (f.range.start(), std::cmp::Reverse(f.range.end())));
    folds
}

fn only_ws_between(token: &SyntaxToken, prev: TextRange) -> bool {
    let mut t = token.prev_token();
    while let Some(tok) = t {
        if tok.text_range().end() <= prev.end() {
            return true;
        }
        if tok.kind() != WHITESPACE {
            return false;
        }
        t = tok.prev_token();
    }
    true
}

fn flush_comments(current: &mut Option<(TextRange, u32)>, folds: &mut Vec<Fold>) {
    if let Some((range, _)) = current.take() {
        folds.push(Fold { range, kind: FoldKind::Comment });
    }
}

/// Range between the first opening and the last closing delimiter.
fn delimited_inner(node: &SyntaxNode) -> Option<TextRange> {
    let is_open = |k: SyntaxKind| matches!(k, L_CURLY | L_PAREN | L_BRACK);
    let is_close = |k: SyntaxKind| matches!(k, R_CURLY | R_PAREN | R_BRACK);
    let tokens = || node.children_with_tokens().filter_map(|e| e.into_token());
    let open = tokens().find(|t| is_open(t.kind()))?;
    let close = tokens().filter(|t| is_close(t.kind())).last()?;
    if close.text_range().start() <= open.text_range().end() {
        return None;
    }
    Some(TextRange::new(open.text_range().start(), close.text_range().start()))
}

fn imports(node: &SyntaxNode, text: &str, folds: &mut Vec<Fold>) {
    let container = if node.kind() == NAMESPACE_DECL {
        match node.children().find(|c| c.kind() == BLOCK) {
            Some(b) => b,
            None => return,
        }
    } else {
        node.clone()
    };
    let mut group: Option<TextRange> = None;
    let mut count = 0;
    let mut push = |group: &mut Option<TextRange>, count: &mut i32| {
        if let Some(r) = group.take() {
            if *count > 1 && text[r].contains('\n') {
                folds.push(Fold { range: r, kind: FoldKind::Imports });
            }
        }
        *count = 0;
    };
    for child in container.children() {
        if child.kind() == USE_DECL {
            group = Some(group.map_or(child.text_range(), |g| g.cover(child.text_range())));
            count += 1;
        } else {
            push(&mut group, &mut count);
        }
    }
    push(&mut group, &mut count);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds() {
        let text = "<?php\nuse A;\nuse B;\n\n/**\n * doc\n */\nclass X {\n    function f() {\n        // a\n        // b\n        return [\n            1,\n        ];\n    }\n}\n// #region foo\n$x = 1;\n// #endregion\n";
        let root = php_syntax::parse(text, php_syntax::PhpVersion::LATEST).syntax_node();
        let folds: Vec<_> = folding_ranges(&root, text)
            .into_iter()
            .map(|f| format!("{:?} {:?}", f.kind, &text[f.range]))
            .collect();
        expect_test::expect![[r#"
            [
                "Imports \"use A;\\nuse B;\"",
                "Comment \"/**\\n * doc\\n */\"",
                "Block \"{\\n    function f() {\\n        // a\\n        // b\\n        return [\\n            1,\\n        ];\\n    }\\n\"",
                "Block \"{\\n        // a\\n        // b\\n        return [\\n            1,\\n        ];\\n    \"",
                "Comment \"// a\\n        // b\"",
                "Block \"[\\n            1,\\n        \"",
                "Region \"// #region foo\\n$x = 1;\\n// #endregion\"",
            ]
        "#]]
        .assert_debug_eq(&folds);
    }
}
