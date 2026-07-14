//! Byte offset ↔ LSP `Position` conversion. `ulx_ast::Span` is a byte
//! `Range<usize>` (see `ulx-ast/src/lib.rs`), but LSP positions are
//! `{line, character}` pairs where `character` counts UTF-16 code units —
//! not bytes, not Unicode scalar values — so a naive byte-column mapping
//! would misplace hovers/diagnostics on any line containing non-ASCII text.
//!
//! Owns a copy of the document text alongside the line-start table so
//! callers (backend.rs) don't need to thread the source string through
//! every lookup — this doubles as the document store's value type.

use tower_lsp::lsp_types::{Position, Range};
use ulx_ast::Span;

pub struct LineIndex {
    text: String,
    /// Byte offset of the first byte of each line; `line_starts[0]` is
    /// always 0.
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        LineIndex { text, line_starts }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Byte offset -> LSP `Position`. Offsets past the end of the text are
    /// clamped rather than panicking (a stale offset from a just-edited
    /// document is a routine race, not a bug worth crashing over).
    pub fn offset_to_position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(l) => l,
            Err(l) => l - 1,
        };
        let line_start = self.line_starts[line];
        let character = self.text[line_start..offset].encode_utf16().count() as u32;
        Position {
            line: line as u32,
            character,
        }
    }

    /// LSP `Position` -> byte offset. A line/character past the end of the
    /// document clamps to the document/line end.
    pub fn position_to_offset(&self, pos: Position) -> usize {
        let line = pos.line as usize;
        let Some(&line_start) = self.line_starts.get(line) else {
            return self.text.len();
        };
        let line_end = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.text.len());
        let line_text = &self.text[line_start..line_end];

        let mut utf16_count = 0u32;
        for (byte_idx, ch) in line_text.char_indices() {
            if utf16_count >= pos.character {
                return line_start + byte_idx;
            }
            utf16_count += ch.len_utf16() as u32;
        }
        line_end
    }

    pub fn span_to_range(&self, span: &Span) -> Range {
        Range::new(
            self.offset_to_position(span.start),
            self.offset_to_position(span.end),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_round_trip() {
        let idx = LineIndex::new("conversation Foo() {\n  \"hi\"\n}\n");
        let offset = 21; // just after the '{' on line 0... actually the '\n'
        let pos = idx.offset_to_position(offset);
        assert_eq!(idx.position_to_offset(pos), offset);
    }

    #[test]
    fn multi_line_offsets() {
        let text = "line one\nline two\nline three";
        let idx = LineIndex::new(text);
        // Start of "line two"
        let offset = text.find("line two").unwrap();
        assert_eq!(idx.offset_to_position(offset), Position::new(1, 0));
        assert_eq!(idx.position_to_offset(Position::new(1, 0)), offset);
        // Start of "line three"
        let offset3 = text.find("line three").unwrap();
        assert_eq!(idx.offset_to_position(offset3), Position::new(2, 0));
    }

    #[test]
    fn multi_byte_utf16_columns() {
        // "é" is 2 UTF-8 bytes but 1 UTF-16 unit; "𝕊" (U+1D54A) is 4 UTF-8
        // bytes but a UTF-16 *surrogate pair* (2 units) — both must be
        // counted correctly, not as byte counts.
        let text = "café 𝕊 end";
        let idx = LineIndex::new(text);

        let space_after_e_acute = text.find(" 𝕊").unwrap();
        let pos = idx.offset_to_position(space_after_e_acute);
        // "caf" (3) + "é" (1 utf16 unit) = 4
        assert_eq!(pos, Position::new(0, 4));
        assert_eq!(idx.position_to_offset(pos), space_after_e_acute);

        let end_offset = text.find(" end").unwrap();
        let pos_end = idx.offset_to_position(end_offset);
        // "caf"(3) + "é"(1) + " "(1) + "𝕊"(2 surrogate units) = 7
        assert_eq!(pos_end, Position::new(0, 7));
        assert_eq!(idx.position_to_offset(pos_end), end_offset);
    }

    #[test]
    fn out_of_bounds_clamps() {
        let idx = LineIndex::new("short");
        assert_eq!(idx.offset_to_position(1000), Position::new(0, 5));
        assert_eq!(idx.position_to_offset(Position::new(50, 50)), "short".len());
    }
}
