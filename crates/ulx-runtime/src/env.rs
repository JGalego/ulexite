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

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.frames.iter().rev().find_map(|f| f.get(name))
    }
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}
