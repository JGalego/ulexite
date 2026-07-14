//! Dataset loading (§7.2, §11.6): a `dataset` declaration is a value —
//! referencing its name anywhere an expression is expected loads it (from
//! inline rows or a JSONL file, resolved relative to the entry program's
//! directory) as a `Value::List` of `Value::Record` rows.

use std::collections::BTreeMap;

use ulx_ir::{IrDataset, IrDatasetSource};

use crate::env::Env;
use crate::error::RuntimeError;
use crate::interp::eval_expr;
use crate::value::Value;
use crate::RunContext;

pub fn load(ctx: &RunContext, decl: &IrDataset) -> Result<Value, RuntimeError> {
    match &decl.source {
        IrDatasetSource::Rows(rows) => {
            let mut env = Env::new();
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                let mut record = BTreeMap::new();
                for (k, expr) in row {
                    record.insert(k.clone(), eval_expr(ctx, expr, &mut env)?);
                }
                out.push(Value::Record(record));
            }
            Ok(Value::List(out))
        }
        IrDatasetSource::FromFile(path) => {
            let full = ctx.base_dir.join(path);
            let content = std::fs::read_to_string(&full).map_err(|e| {
                RuntimeError::Io(format!(
                    "reading dataset `{}` at {}: {e}",
                    decl.name,
                    full.display()
                ))
            })?;
            let mut out = Vec::new();
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let json: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                    RuntimeError::Io(format!("parsing dataset `{}` row: {e}", decl.name))
                })?;
                out.push(from_json(json));
            }
            Ok(Value::List(out))
        }
    }
}

fn from_json(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Unit,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::Text(s),
        serde_json::Value::Array(items) => Value::List(items.into_iter().map(from_json).collect()),
        serde_json::Value::Object(map) => {
            Value::Record(map.into_iter().map(|(k, v)| (k, from_json(v))).collect())
        }
    }
}
