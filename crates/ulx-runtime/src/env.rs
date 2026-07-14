use std::collections::HashMap;

use crate::value::Value;

/// A stack of lexical scopes holding runtime values, mirroring
/// `ulx-sema::scope::Scope` but for values instead of inferred types.
#[derive(Clone)]
pub struct Env {
    frames: Vec<HashMap<String, Value>>,
}

impl Env {
    pub fn new() -> Self {
        Env {
            frames: vec![HashMap::new()],
        }
    }

    pub fn push(&mut self) {
        self.frames.push(HashMap::new());
    }

    pub fn pop(&mut self) {
        self.frames.pop();
    }

    pub fn declare(&mut self, name: impl Into<String>, value: Value) {
        if let Some(top) = self.frames.last_mut() {
            top.insert(name.into(), value);
        }
    }

    /// Mutates an *existing* binding in place, in whichever frame it was
    /// originally declared (walking innermost-to-outermost) — unlike
    /// `declare`, which always writes into the current (innermost) frame
    /// and would therefore lose the update once a nested block's frame is
    /// popped (e.g. a `for` loop body mutating a variable declared in the
    /// enclosing conversation scope, as `results.append(...)` does). Falls
    /// back to `declare` in the current frame if `name` isn't bound
    /// anywhere yet.
    pub fn set(&mut self, name: &str, value: Value) {
        for frame in self.frames.iter_mut().rev() {
            if frame.contains_key(name) {
                frame.insert(name.to_string(), value);
                return;
            }
        }
        self.declare(name.to_string(), value);
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.frames.iter().rev().find_map(|f| f.get(name))
    }
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}
