//! Front end of the Ulexite compiler (§13): lexer + parser, producing the
//! AST defined in `ulx-ast`. Deliberately independent of any provider or
//! runtime concept (§4.3, §13.1).

pub mod lexer;
pub mod parser;

pub use lexer::Token;
pub use parser::{parse_source, Err};
