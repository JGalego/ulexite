//! In-browser execution: `ulxStart`/`UlxRun.step`/`UlxRun.provideAnswer`
//! run a conversation against a local model driven entirely from JS (the
//! website's Playground run panel), reusing the same suspend/resume
//! generalization built for `escalate` — `ulx-runtime`'s
//! `RunContext::suspend_on_provider_miss` — instead of a human decision.
//! `ulx-runtime` is a dependency here with `default-features = false`: no
//! `ureq`-backed real vendor adapters, no OS-thread `with`-block
//! parallelism, neither of which targets `wasm32-unknown-unknown` (see its
//! Cargo.toml/module docs) — so this crate only ever resolves `chat`/
//! `judge` to a `BrowserLocalProvider` standing in for the JS-driven model,
//! and a `with` block runs its branches sequentially.
//!
//! The whole bridge stays synchronous on the Rust side: `step()` never
//! blocks and never calls into JS itself. Every actual model call happens
//! entirely in JS, between one `step()` returning `"suspended"` and the
//! next `step()` call after `provideAnswer()` has recorded a reply.

use std::collections::BTreeMap;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use ulx_ast::TopDecl;
use ulx_runtime::provider::Message;
use ulx_runtime::{Cache, ProviderRegistry, RunContext, RuntimeError, TraceWriter, Value};

fn collect_provider_names(program: &ulx_ast::Program) -> Vec<String> {
    program
        .decls
        .iter()
        .filter_map(|(decl, _)| match decl {
            TopDecl::Provider(p) => Some(p.name.clone()),
            _ => None,
        })
        .collect()
}

fn parse_and_lower(source: &str) -> Result<(ulx_ast::Program, ulx_ir::IrProgram), JsValue> {
    let program = ulx_syntax::parse_source(source)
        .map_err(|errs| JsValue::from_str(&format!("{} parse error(s)", errs.len())))?;
    let errors: Vec<_> = ulx_sema::analyze(&program)
        .into_iter()
        .filter(|d| d.severity == ulx_sema::Severity::Error)
        .collect();
    if !errors.is_empty() {
        return Err(JsValue::from_str(&format!(
            "{} semantic error(s) — run check() first",
            errors.len()
        )));
    }
    let ir = ulx_ir::lower_program(&program)
        .map_err(|e| JsValue::from_str(&format!("could not lower program: {e:?}")))?;
    Ok((program, ir))
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum StepResult {
    Done {
        value: Value,
    },
    Suspended {
        cache_key: String,
        target: String,
        reason: String,
        messages: Vec<Message>,
    },
    Error {
        message: String,
    },
}

/// Every declared `conversation` name in `source`, for the Run panel's
/// picker when a program declares more than one.
#[wasm_bindgen(js_name = conversationNames)]
pub fn conversation_names(source: &str) -> Result<JsValue, JsValue> {
    let (_, ir) = parse_and_lower(source)?;
    let names: Vec<&str> = ir.conversations.iter().map(|c| c.name.as_str()).collect();
    serde_wasm_bindgen::to_value(&names).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Every `TypeExpr` a shipped example's `conversation` declares for its
/// parameters is `text` — reads `conversation`'s parameter names for the
/// Run panel's argument form (one text input per name).
#[wasm_bindgen(js_name = conversationParams)]
pub fn conversation_params(source: &str, conversation: &str) -> Result<JsValue, JsValue> {
    let (_, ir) = parse_and_lower(source)?;
    let decl = ir
        .conversations
        .iter()
        .find(|c| c.name == conversation)
        .ok_or_else(|| JsValue::from_str(&format!("no conversation named `{conversation}`")))?;
    let names: Vec<&str> = decl.params.iter().map(|(name, _)| name.as_str()).collect();
    serde_wasm_bindgen::to_value(&names).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Parses+analyzes+lowers `source` (same pipeline `check()` runs) and binds
/// `args_json` (a flat `{"param": "value", ...}` JSON object — every
/// argument is treated as `text`) to `conversation`'s parameters, ready to
/// `step()`. Callers should already have run `check()` and fixed any
/// diagnostics first — this doesn't re-report them, it just refuses to
/// start on a program that doesn't even parse/analyze/lower cleanly.
#[wasm_bindgen(js_name = ulxStart)]
pub fn ulx_start(source: &str, conversation: &str, args_json: &str) -> Result<UlxRun, JsValue> {
    let (program, ir) = parse_and_lower(source)?;
    if !ir.conversations.iter().any(|c| c.name == conversation) {
        return Err(JsValue::from_str(&format!(
            "no conversation named `{conversation}`"
        )));
    }

    let provider_names = collect_provider_names(&program);

    let raw_args: BTreeMap<String, serde_json::Value> = serde_json::from_str(args_json)
        .map_err(|e| JsValue::from_str(&format!("invalid args JSON: {e}")))?;
    let args: BTreeMap<String, Value> = raw_args
        .into_iter()
        .map(|(k, v)| {
            let text = match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            (k, Value::Text(text))
        })
        .collect();

    Ok(UlxRun {
        program: ir,
        conversation: conversation.to_string(),
        args,
        provider_names,
        cache: Cache::in_memory(),
    })
}

#[wasm_bindgen]
pub struct UlxRun {
    program: ulx_ir::IrProgram,
    conversation: String,
    args: BTreeMap<String, Value>,
    provider_names: Vec<String>,
    cache: Cache,
}

#[wasm_bindgen]
impl UlxRun {
    /// Drives the conversation one step against a fresh `RunContext` built
    /// from the same in-memory `cache` every step shares — mirroring
    /// `ulx-cli`'s own resume loop, which similarly rebuilds a fresh
    /// context per attempt rather than reusing one across a suspend.
    /// Returns a tagged object: `{status: "done", value}`,
    /// `{status: "suspended", cache_key, target, reason, messages}`, or
    /// `{status: "error", message}`.
    #[wasm_bindgen(js_name = step)]
    pub fn step(&mut self) -> Result<JsValue, JsValue> {
        let registry = ProviderRegistry::with_browser_local_named(self.provider_names.clone());
        let trace = TraceWriter::in_memory("browser");
        let ctx = RunContext::new(
            &self.program,
            registry,
            self.cache.clone(),
            trace,
            "browser".to_string(),
            std::path::PathBuf::new(),
        )
        .with_suspend_on_provider_miss();

        let result =
            match ulx_runtime::run_conversation(&ctx, &self.conversation, self.args.clone()) {
                Ok(value) => StepResult::Done { value },
                Err(RuntimeError::Suspended {
                    cache_key,
                    target,
                    reason,
                    messages,
                }) => StepResult::Suspended {
                    cache_key,
                    target,
                    reason,
                    messages,
                },
                Err(e) => StepResult::Error {
                    message: e.to_string(),
                },
            };
        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Writes an out-of-band answer for a pending suspend (from a prior
    /// `step()`'s `"suspended"` result) into the shared cache, so the next
    /// `step()` call hits it instead of suspending on the same call again.
    /// `target == "judge"` needs its raw model reply parsed into a
    /// `Verdict` first (`judge_reply_to_value`); anything else (`"chat"`)
    /// is cached as plain text.
    #[wasm_bindgen(js_name = provideAnswer)]
    pub fn provide_answer(&mut self, cache_key: &str, target: &str, text: &str) {
        let value = if target == "judge" {
            ulx_runtime::provider::judge_reply_to_value(text)
        } else {
            Value::Text(text.to_string())
        };
        let _ = self.cache.put(cache_key, &value);
    }
}
