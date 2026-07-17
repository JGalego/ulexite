//! Byte-offset -> (line, column) conversion for rendering `ulx_sema`/
//! `ulx_syntax` diagnostics in the browser. Deliberately independent of
//! `ulx-lsp`'s `LineIndex` (which returns a `tower_lsp::lsp_types::
//! Position` and lives in an LSP-specific crate that pulls in `tokio`) —
//! reusing it here would be the wrong dependency direction for a
//! `wasm-bindgen` crate with no LSP concept at all, so this is a small,
//! independent sibling instead. Columns are 0-indexed UTF-16 code units,
//! matching how most editors (and LSP) report them.

pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        LineIndex { line_starts }
    }

    /// Byte offset -> 0-indexed (line, UTF-16 column) within `text` (the
    /// same text this index was built from).
    pub fn line_col(&self, text: &str, offset: usize) -> (u32, u32) {
        let offset = offset.min(text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(l) => l,
            Err(l) => l - 1,
        };
        let line_start = self.line_starts[line];
        let column = text[line_start..offset].encode_utf16().count() as u32;
        (line as u32, column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_line_offsets() {
        let text = "line one\nline two\nline three";
        let idx = LineIndex::new(text);
        let offset = text.find("line two").unwrap();
        assert_eq!(idx.line_col(text, offset), (1, 0));
    }

    #[test]
    fn multi_byte_utf16_columns() {
        let text = "café end";
        let idx = LineIndex::new(text);
        let offset = text.find(" end").unwrap();
        assert_eq!(idx.line_col(text, offset), (0, 4));
    }

    #[test]
    fn out_of_bounds_clamps() {
        let idx = LineIndex::new("short");
        assert_eq!(idx.line_col("short", 1000), (0, 5));
    }
}
