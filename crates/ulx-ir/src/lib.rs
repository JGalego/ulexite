//! Intermediate representation (§13.4): a desugared form of the AST that
//! makes the pure/effect distinction explicit and resolves message-literal
//! sugar (`system:`/`user:`/`assistant ->`) into an explicit `chat` effect.
//!
//! This is a *tree* IR, not the flat SSA form the spec's optimizer section
//! (§13.5) gestures at — a tree is enough to (a) give the runtime a
//! provider-agnostic execution target that never names a vendor and (b)
//! make `with`-blocks a first-class `Parallel` node the runtime can
//! actually schedule concurrently. A flatter SSA form with real CSE/DCE
//! passes is future work (see `docs/spec/25-future-directions.md`); the
//! `optimize` module here does the one pass that's genuinely load-bearing
//! at tree granularity: dead-artifact elimination (§13.5).

mod lower;
mod optimize;
mod types;

pub use lower::{lower_program, LowerError};
pub use optimize::eliminate_dead_bindings;
pub use types::*;
