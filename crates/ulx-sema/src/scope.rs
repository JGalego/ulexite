use std::collections::HashMap;
use ulx_ast::{Spanned, TypeExpr};

/// A best-effort inferred type for a binding. `Unknown` means "we didn't
/// try hard enough to infer this" and must never cause a false-positive
/// diagnostic downstream (§24.1's honesty principle applied to this pass:
/// silence on uncertainty, not a guess).
#[derive(Debug, Clone, PartialEq)]
pub enum InferredType {
    Known(TypeExpr),
    Unknown,
}

/// A stack of lexical scopes for name resolution within one conversation
/// body (§10.2's imperative region — `with`-block independence is checked
/// separately in `typecheck::check_with_block_independence`, not here).
pub struct Scope {
    frames: Vec<HashMap<String, InferredType>>,
}

impl Scope {
    pub fn new() -> Self {
        Scope {
            frames: vec![HashMap::new()],
        }
    }

    pub fn push(&mut self) {
        self.frames.push(HashMap::new());
    }

    pub fn pop(&mut self) {
        self.frames.pop();
    }

    pub fn declare(&mut self, name: impl Into<String>, ty: InferredType) {
        if let Some(top) = self.frames.last_mut() {
            top.insert(name.into(), ty);
        }
    }

    pub fn declare_typed(&mut self, name: impl Into<String>, ty: &Option<Spanned<TypeExpr>>) {
        let inferred = match ty {
            Some((t, _)) => InferredType::Known(t.clone()),
            None => InferredType::Unknown,
        };
        self.declare(name, inferred);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.frames.iter().rev().any(|f| f.contains_key(name))
    }

    pub fn type_of(&self, name: &str) -> Option<&InferredType> {
        self.frames.iter().rev().find_map(|f| f.get(name))
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}
