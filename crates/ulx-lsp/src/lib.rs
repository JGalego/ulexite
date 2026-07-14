//! Library surface for `ulx-lsp` (§20.2's language server) — split from
//! `main.rs` (the stdio entrypoint) purely so `tests/lsp.rs` can construct
//! a `Backend` directly and call its `tower_lsp::LanguageServer` methods
//! in-process, without spawning a real process and speaking the wire
//! protocol over stdio.

pub mod analysis;
pub mod backend;
pub mod capabilities;
pub mod index;
pub mod line_index;
