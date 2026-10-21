//! Conversions between php-ide and LSP types.

use lsp_types as lsp;
use php_ide::{FoldKind, LineCol, LineIndex, PositionEncoding, Severity, SymbolKind, TextRange, TextSize};

pub fn position(index: &LineIndex, enc: PositionEncoding, offset: TextSize) -> lsp::Position {
    let lc = index.to_encoding(index.line_col(offset), enc);
    lsp::Position::new(lc.line, lc.col)
}

pub fn range(index: &LineIndex, enc: PositionEncoding, range: TextRange) -> lsp::Range {
    lsp::Range::new(position(index, enc, range.start()), position(index, enc, range.end()))
}

pub fn offset(index: &LineIndex, enc: PositionEncoding, pos: lsp::Position) -> TextSize {
    let lc = index.from_encoding(LineCol { line: pos.line, col: pos.character }, enc);
    index.offset(lc)
}

pub fn text_range(index: &LineIndex, enc: PositionEncoding, range: lsp::Range) -> TextRange {
    let start = offset(index, enc, range.start);
    let end = offset(index, enc, range.end);
    TextRange::new(start.min(end), end.max(start))
}

pub fn severity(s: Severity) -> lsp::DiagnosticSeverity {
    match s {
        Severity::Error => lsp::DiagnosticSeverity::ERROR,
        Severity::Warning => lsp::DiagnosticSeverity::WARNING,
        Severity::Information => lsp::DiagnosticSeverity::INFORMATION,
        Severity::Hint => lsp::DiagnosticSeverity::HINT,
    }
}

pub fn symbol_kind(k: SymbolKind) -> lsp::SymbolKind {
    match k {
        SymbolKind::Namespace => lsp::SymbolKind::NAMESPACE,
        SymbolKind::Class | SymbolKind::Trait => lsp::SymbolKind::CLASS,
        SymbolKind::Interface => lsp::SymbolKind::INTERFACE,
        SymbolKind::Enum => lsp::SymbolKind::ENUM,
        SymbolKind::EnumMember => lsp::SymbolKind::ENUM_MEMBER,
        SymbolKind::Method => lsp::SymbolKind::METHOD,
        SymbolKind::Constructor => lsp::SymbolKind::CONSTRUCTOR,
        SymbolKind::Property => lsp::SymbolKind::PROPERTY,
        SymbolKind::Constant => lsp::SymbolKind::CONSTANT,
        SymbolKind::Function => lsp::SymbolKind::FUNCTION,
        SymbolKind::Variable => lsp::SymbolKind::VARIABLE,
    }
}

pub fn document_symbol(index: &LineIndex, enc: PositionEncoding, s: php_ide::Symbol) -> lsp::DocumentSymbol {
    #[allow(deprecated)]
    lsp::DocumentSymbol {
        name: s.name,
        detail: s.detail,
        kind: symbol_kind(s.kind),
        tags: s.deprecated.then(|| vec![lsp::SymbolTag::DEPRECATED]),
        deprecated: None,
        range: range(index, enc, s.range),
        selection_range: range(index, enc, s.selection_range),
        children: if s.children.is_empty() {
            None
        } else {
            Some(s.children.into_iter().map(|c| document_symbol(index, enc, c)).collect())
        },
    }
}

pub fn folding_range(index: &LineIndex, text: &str, fold: php_ide::Fold) -> Option<lsp::FoldingRange> {
    let start = index.line_col(fold.range.start());
    let end = index.line_col(fold.range.end());
    let mut end_line = end.line;
    if fold.kind == FoldKind::Block && end.line > start.line {
        // Keep the line with the closing delimiter visible if it starts the line.
        let line_start = index.offset(LineCol { line: end.line, col: 0 });
        if text[usize::from(line_start)..usize::from(fold.range.end())].trim().is_empty() {
            end_line -= 1;
        }
    }
    if end_line <= start.line {
        return None;
    }
    let kind = match fold.kind {
        FoldKind::Comment => Some(lsp::FoldingRangeKind::Comment),
        FoldKind::Imports => Some(lsp::FoldingRangeKind::Imports),
        FoldKind::Region => Some(lsp::FoldingRangeKind::Region),
        FoldKind::Block => None,
    };
    Some(lsp::FoldingRange {
        start_line: start.line,
        start_character: None,
        end_line,
        end_character: None,
        kind,
        collapsed_text: None,
    })
}
