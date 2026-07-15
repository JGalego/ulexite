//! Front end of the Ulexite compiler (§13): lexer + parser, producing the
//! AST defined in `ulx-ast`. Deliberately independent of any provider or
//! runtime concept (§4.3, §13.1).

pub mod fmt;
pub mod lexer;
pub mod parser;

pub use fmt::{format_program, format_source};
pub use lexer::Token;
pub use parser::{parse_source, split_text_block, Err};
