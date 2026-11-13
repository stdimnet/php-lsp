//! Conversion between byte offsets and line/column positions (UTF-8 and
//! UTF-16 columns, as required by LSP clients).

use php_syntax::{TextRange, TextSize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    /// Offset of the start of each line.
    line_starts: Vec<TextSize>,
    /// For each line containing non-ASCII characters: (utf8 column, utf8 len) of those chars.
    wide_chars: Vec<(u32, Vec<(u32, u8)>)>,
    len: TextSize,
}

/// Zero based line and UTF-8 byte column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// Which unit columns are measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PositionEncoding {
    Utf8,
    #[default]
    Utf16,
    Utf32,
}

impl LineIndex {
    pub fn new(text: &str) -> LineIndex {
        let mut line_starts = vec![TextSize::from(0)];
        let mut wide_chars = Vec::new();
        let mut current_wide: Vec<(u32, u8)> = Vec::new();
        let mut line_start = 0usize;
        for (i, c) in text.char_indices() {
            if c == '\n' {
                if !current_wide.is_empty() {
                    wide_chars.push((line_starts.len() as u32 - 1, std::mem::take(&mut current_wide)));
                }
                line_start = i + 1;
                line_starts.push(TextSize::from(line_start as u32));
            } else if !c.is_ascii() {
                current_wide.push(((i - line_start) as u32, c.len_utf8() as u8));
            }
        }
        if !current_wide.is_empty() {
            wide_chars.push((line_starts.len() as u32 - 1, current_wide));
        }
        LineIndex { line_starts, wide_chars, len: TextSize::from(text.len() as u32) }
    }

    pub fn len(&self) -> TextSize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == TextSize::from(0)
    }

    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }

    pub fn line_col(&self, offset: TextSize) -> LineCol {
        let offset = offset.min(self.len);
        let line = self.line_starts.partition_point(|&s| s <= offset) - 1;
        LineCol { line: line as u32, col: u32::from(offset - self.line_starts[line]) }
    }

    pub fn offset(&self, lc: LineCol) -> TextSize {
        let Some(&start) = self.line_starts.get(lc.line as usize) else { return self.len };
        let end = self.line_starts.get(lc.line as usize + 1).copied().unwrap_or(self.len);
        (start + TextSize::from(lc.col)).min(end).min(self.len)
    }

    fn wide(&self, line: u32) -> &[(u32, u8)] {
        match self.wide_chars.binary_search_by_key(&line, |(l, _)| *l) {
            Ok(i) => &self.wide_chars[i].1,
            Err(_) => &[],
        }
    }

    /// Converts a UTF-8 column to the given encoding.
    pub fn to_encoding(&self, lc: LineCol, enc: PositionEncoding) -> LineCol {
        if enc == PositionEncoding::Utf8 {
            return lc;
        }
        let mut col = lc.col;
        for &(c, len) in self.wide(lc.line) {
            if c >= lc.col {
                break;
            }
            col -= len as u32 - units(len, enc);
        }
        LineCol { line: lc.line, col }
    }

    /// Converts a column in the given encoding back to a UTF-8 column.
    pub fn from_encoding(&self, lc: LineCol, enc: PositionEncoding) -> LineCol {
        if enc == PositionEncoding::Utf8 {
            return lc;
        }
        let mut col = lc.col;
        // `shift` = utf8 column - encoded column for positions after the char.
        let mut shift = 0;
        for &(c, len) in self.wide(lc.line) {
            let c_enc = c - shift;
            if c_enc >= lc.col {
                break;
            }
            let extra = len as u32 - units(len, enc);
            col += extra;
            shift += extra;
        }
        LineCol { line: lc.line, col }
    }

    pub fn range_to_line_cols(&self, range: TextRange) -> (LineCol, LineCol) {
        (self.line_col(range.start()), self.line_col(range.end()))
    }
}

/// Number of code units a UTF-8 sequence of `len` bytes takes in `enc`.
fn units(len: u8, enc: PositionEncoding) -> u32 {
    match enc {
        PositionEncoding::Utf8 => len as u32,
        PositionEncoding::Utf16 if len == 4 => 2,
        PositionEncoding::Utf16 | PositionEncoding::Utf32 => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_roundtrip() {
        let text = "ab\nä€😀x\n";
        let idx = LineIndex::new(text);
        let x = TextSize::from(text.find('x').unwrap() as u32);
        let lc = idx.line_col(x);
        assert_eq!(lc, LineCol { line: 1, col: 9 });
        let u16 = idx.to_encoding(lc, PositionEncoding::Utf16);
        assert_eq!(u16.col, 4);
        assert_eq!(idx.from_encoding(u16, PositionEncoding::Utf16), lc);
        assert_eq!(idx.offset(lc), x);
    }
}
