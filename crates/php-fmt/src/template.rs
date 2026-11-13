//! HTML formatting of templates (files mixing HTML and PHP), done like
//! Intelephense: every PHP region becomes a placeholder `<?php N ?>` (`N`:
//! how many blocks the region opens or closes), the result is formatted
//! with js-beautify's HTML beautifier, then the PHP code is put back.

use php_syntax::{SyntaxKind::*, SyntaxNode, TextRange, TextSize};

/// Whether the file contains HTML besides PHP code.
pub(crate) fn is_template(root: &SyntaxNode) -> bool {
    root.descendants_with_tokens().filter_map(|e| e.into_token()).any(|t| {
        t.kind() == INLINE_HTML
            && !t.text().trim().is_empty()
            && !(t.text_range().start() == TextSize::from(0) && t.text().trim().is_empty())
    })
}

/// Block open (+1) and close (-1) positions of statement lists.
fn block_events(root: &SyntaxNode) -> Vec<(TextSize, i32)> {
    let mut events = Vec::new();
    for node in root.descendants() {
        let tokens = || node.children_with_tokens().filter_map(|e| e.into_token());
        match node.kind() {
            BLOCK | MEMBER_LIST | SWITCH_BODY => {
                let open = tokens().find(|t| matches!(t.kind(), L_CURLY | COLON));
                let close = tokens().filter(|t| matches!(t.kind(), R_CURLY | ENDSWITCH_KW)).last();
                if let Some(o) = open {
                    events.push((o.text_range().start(), 1));
                    let end = close.map_or(node.text_range().end(), |c| c.text_range().start());
                    events.push((end, -1));
                }
            }
            MATCH_EXPR => {
                let open = tokens().find(|t| t.kind() == L_CURLY);
                let close = tokens().filter(|t| t.kind() == R_CURLY).last();
                if let (Some(o), Some(c)) = (open, close) {
                    events.push((o.text_range().start(), 1));
                    events.push((c.text_range().start(), -1));
                }
            }
            STMT_LIST => {
                if let Some(colon) = tokens().next().filter(|t| t.kind() == COLON) {
                    events.push((colon.text_range().start(), 1));
                    events.push((node.text_range().end(), -1));
                }
            }
            CASE_CLAUSE | DEFAULT_CLAUSE => {
                // the statements after `case x:`
                let colon = tokens().find(|t| matches!(t.kind(), COLON | SEMICOLON));
                let first = colon.and_then(|colon| {
                    node.children().find(|c| c.text_range().start() >= colon.text_range().end())
                });
                if let Some(first) = first {
                    events.push((first.text_range().start(), 1));
                    events.push((node.text_range().end(), -1));
                }
            }
            _ => {}
        }
    }
    events.sort_by_key(|(at, delta)| (*at, *delta));
    events
}

/// The HTML of the file with placeholders, and the PHP regions they stand
/// for (from `<?php` to `?>` or the end of the file).
fn virtual_document(text: &str, root: &SyntaxNode) -> (String, Vec<TextRange>) {
    let events = block_events(root);
    let mut out = String::new();
    let mut regions = Vec::new();
    let mut open: Option<TextSize> = None;
    let placeholder = |from: TextSize, to: TextSize| -> String {
        let deltas: Vec<i32> =
            events.iter().filter(|(at, _)| *at >= from && *at <= to).map(|(_, d)| *d).collect();
        let sum: i32 = deltas.iter().sum();
        if sum != 0 {
            format!("<?php {sum} ?>")
        } else if deltas.first() == Some(&-1) {
            "<?php 0 ?>".into()
        } else {
            "<?php ?>".into()
        }
    };
    for t in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        match t.kind() {
            INLINE_HTML if open.is_none() => out.push_str(t.text()),
            OPEN_TAG | OPEN_TAG_WITH_ECHO if open.is_none() => open = Some(t.text_range().start()),
            CLOSE_TAG => {
                if let Some(o) = open.take() {
                    // `?>` swallows one newline; it belongs to the HTML
                    let close = t.text_range().start() + TextSize::from(2);
                    out.push_str(&placeholder(o, t.text_range().start()));
                    regions.push(TextRange::new(o, close));
                    out.push_str(&t.text()[2..]);
                }
            }
            _ => {}
        }
    }
    if let Some(o) = open {
        let end = TextSize::from(text.len() as u32);
        out.push_str(&placeholder(o, end));
        regions.push(TextRange::new(o, end));
    }
    (out, regions)
}

/// The template with its HTML formatted (`None` when it is not a template
/// or the result would change more than whitespace).
pub(crate) fn format_html(
    text: &str,
    root: &SyntaxNode,
    indent_size: usize,
    use_tabs: bool,
) -> Option<String> {
    if !is_template(root) {
        return None;
    }
    let (virtual_text, regions) = virtual_document(text, root);
    let html = crate::beautify::html(&virtual_text, indent_size, use_tabs);
    // put the PHP code back
    let placeholder = regex::Regex::new(r"<\?php (?:-?\d+ )?\?>").expect("valid pattern");
    let mut regions = regions.into_iter();
    let result = placeholder.replace_all(&html, |c: &regex::Captures| match regions.next() {
        Some(r) => text[r].to_string(),
        None => c[0].to_string(),
    });
    let result = result.into_owned();
    let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    (squeeze(&result) == squeeze(text)).then_some(result)
}
