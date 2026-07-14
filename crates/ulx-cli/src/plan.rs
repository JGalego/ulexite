//! `ulx plan` (§10.5): a static, execution-free walk of a conversation's
//! lowered IR (`ulx-ir`) that reports every capability/judge call it will
//! make, which provider each resolves to under current policy, and a rough
//! token/cost estimate — Terraform's `plan`/`apply` split applied to token
//! spend instead of infrastructure changes.
//!
//! Two things here are deliberately *not* the real thing the full spec
//! describes, and are labeled as such wherever they're printed:
//!
//! - **Token counts are a heuristic**, not a real tokenizer: roughly one
//!   token per four characters of statically-known message/rubric text,
//!   plus a wide, hand-picked allowance for every interpolated value this
//!   pass can't know ahead of time (a prior `assistant ->` binding, a
//!   dataset row, a caller-supplied argument with no `--arg` override).
//! - **Pricing is a small hardcoded table** (`pricing_table` below) of
//!   illustrative, approximate $/1K-token rates — not live vendor pricing,
//!   and nowhere near every model. An unrecognized model still gets a row,
//!   just with its cost columns marked as unpriced rather than guessed at.
//!
//! Capability resolution itself is the one thing *not* approximated here:
//! every row's provider comes straight from `ProviderRegistry::resolve`/
//! `resolve_named` (`ulx-runtime`'s `provider::mod`), the exact same
//! capability -> provider lookup `ask`/`match judge` execution uses — an
//! `Ambiguous`/`UnknownCapability`/etc. resolution failure is surfaced as an
//! error cell on that row, not a crash, and not silently skipped either.

use std::collections::{BTreeMap, HashSet};

use ulx_ir::{IrArmBody, IrBlock, IrConversation, IrEffect, IrExpr, IrProgram, IrTextPart};
use ulx_runtime::ProviderRegistry;

use crate::providers::ProviderInfo;

/// ~4 English characters per token — the same rough ratio OpenAI's own
/// tokenizer docs quote as a ballpark, not a real BPE tokenizer run.
const CHARS_PER_TOKEN: f64 = 4.0;

/// A statically-unknown interpolated value's assumed character-length
/// range (a short slug/word up to a hefty paragraph) — deliberately wide
/// since this pass has no way to know what a prior `assistant ->` binding,
/// dataset row, or un-overridden argument will actually contain at runtime.
const UNKNOWN_CHARS_LOW: usize = 20;
const UNKNOWN_CHARS_HIGH: usize = 800;

/// One capability/judge invocation site found by statically walking a
/// conversation's IR, before it's been resolved against a `ProviderRegistry`.
struct PlannedCall {
    /// The capability `ProviderRegistry::resolve`/`resolve_named` is asked
    /// about — `"chat"`, `"embed"`, `"judge"`, etc.
    capability: String,
    /// What the table prints in the "capability" column — usually the same
    /// as `capability`, except judge calls also name the judge.
    label: String,
    /// An explicit `ask cap(provider: "name")` arg, if this call pinned one.
    provider_arg: Option<String>,
    prompt_chars_low: usize,
    prompt_chars_high: usize,
}

/// A resolved (or failed-to-resolve) row of the printed plan table.
#[derive(Debug)]
pub struct PlannedRow {
    pub label: String,
    pub resolution: Result<ResolvedTarget, String>,
    pub tokens_low: u64,
    pub tokens_high: u64,
    pub cost_low: f64,
    pub cost_high: f64,
    /// `false` when the resolved model has no entry in `pricing_table`
    /// (or a prefix/substring of it) — `cost_low`/`cost_high` are `0.0` in
    /// that case and should be displayed as "unpriced", not as a real zero.
    pub priced: bool,
}

#[derive(Debug)]
pub struct ResolvedTarget {
    pub provider: String,
    pub vendor: String,
    pub model: String,
}

/// Statically walks `conversation`'s IR body and resolves every capability/
/// judge call it finds against `registry`, using `infos` to recover the
/// model each resolved provider is configured with. `known_vars` seeds the
/// character-length heuristic for any `--arg name=value` the caller passed
/// that matches a top-level conversation parameter by name — every other
/// dynamic value (bindings, dataset rows, nested calls' own parameters) is
/// necessarily unknown to a static pass and falls back to the generic
/// `UNKNOWN_CHARS_*` allowance.
pub fn build_plan(
    ir: &IrProgram,
    conversation: &str,
    registry: &ProviderRegistry,
    infos: &BTreeMap<String, ProviderInfo>,
    known_vars: &BTreeMap<String, String>,
) -> Result<Vec<PlannedRow>, String> {
    let conv = ir
        .conversations
        .iter()
        .find(|c| c.name == conversation)
        .ok_or_else(|| {
            format!(
                "no conversation named `{conversation}` in this file (only the entry file's own \
                 conversations are inspected — not conversations reached only through an import)"
            )
        })?;

    let mut calls = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(conv.name.clone());
    walk_block(&conv.body, ir, known_vars, &mut visited, &mut calls);

    Ok(calls
        .into_iter()
        .map(|call| resolve_row(call, registry, infos))
        .collect())
}

fn resolve_row(
    call: PlannedCall,
    registry: &ProviderRegistry,
    infos: &BTreeMap<String, ProviderInfo>,
) -> PlannedRow {
    // The actual capability -> provider decision is never reimplemented
    // here — `resolve`/`resolve_named` are the exact same calls
    // `ulx-runtime`'s `eval_ask`/`eval_rubric_call` make, so an `Ambiguous`
    // capability plans exactly as ambiguous as it would run.
    let resolved_name: Result<String, String> = match &call.provider_arg {
        Some(name) => registry
            .resolve_named(&call.capability, name)
            .map(|_| name.clone())
            .map_err(|e| e.to_string()),
        None => registry
            .resolve(&call.capability)
            .map_err(|e| e.to_string())
            .map(|_| {
                // `resolve` succeeding means exactly one registered entry
                // supports this capability (that's its own uniqueness
                // guarantee) — `infos` was built from that same merged
                // config, so exactly one entry here supports it too.
                infos
                    .iter()
                    .find(|(_, info)| info.models.contains_key(&call.capability))
                    .map(|(name, _)| name.clone())
                    .unwrap_or_else(|| "?".to_string())
            }),
    };

    match resolved_name {
        Ok(provider_name) => {
            let info = infos.get(&provider_name);
            let vendor = info.map(|i| i.vendor.clone()).unwrap_or_default();
            let model = info
                .and_then(|i| i.models.get(&call.capability).cloned())
                .unwrap_or_else(|| "?".to_string());
            let cost = estimate_cost(
                &call.capability,
                &vendor,
                &model,
                call.prompt_chars_low,
                call.prompt_chars_high,
            );
            PlannedRow {
                label: call.label,
                resolution: Ok(ResolvedTarget {
                    provider: provider_name,
                    vendor,
                    model,
                }),
                tokens_low: cost.tokens_low,
                tokens_high: cost.tokens_high,
                cost_low: cost.cost_low,
                cost_high: cost.cost_high,
                priced: cost.priced,
            }
        }
        Err(msg) => PlannedRow {
            label: call.label,
            resolution: Err(msg),
            tokens_low: 0,
            tokens_high: 0,
            cost_low: 0.0,
            cost_high: 0.0,
            priced: false,
        },
    }
}

// --- IR traversal -----------------------------------------------------
//
// Mirrors `ulx-sema/src/typecheck.rs`'s `check_block`/`check_stmt`/
// `check_expr` recursive-descent shape (one match arm per node variant,
// recursing into every nested block/expr) — adapted to the already-lowered
// IR (rather than the raw AST) so `system:`/`user:`/`assistant ->`
// message-literal sugar is walked as the single explicit `chat` effect it
// desugars to (`ulx-ir/src/lower.rs`), not missed entirely the way an
// AST-only walk would miss it.

fn walk_block(
    block: &IrBlock,
    program: &IrProgram,
    known_vars: &BTreeMap<String, String>,
    visited: &mut HashSet<String>,
    out: &mut Vec<PlannedCall>,
) {
    for inst in &block.insts {
        walk_expr(&inst.expr, program, known_vars, visited, out);
    }
    if let Some(tail) = &block.tail {
        walk_expr(tail, program, known_vars, visited, out);
    }
}

fn walk_expr(
    expr: &IrExpr,
    program: &IrProgram,
    known_vars: &BTreeMap<String, String>,
    visited: &mut HashSet<String>,
    out: &mut Vec<PlannedCall>,
) {
    match expr {
        IrExpr::Effect(effect) => walk_effect(effect, program, known_vars, visited, out),
        IrExpr::FieldAccess { base, .. } => walk_expr(base, program, known_vars, visited, out),
        IrExpr::Index { base, index } => {
            walk_expr(base, program, known_vars, visited, out);
            walk_expr(index, program, known_vars, visited, out);
        }
        IrExpr::Unary { expr, .. } => walk_expr(expr, program, known_vars, visited, out),
        IrExpr::Binary { lhs, rhs, .. } => {
            walk_expr(lhs, program, known_vars, visited, out);
            walk_expr(rhs, program, known_vars, visited, out);
        }
        IrExpr::If {
            cond,
            then_block,
            else_block,
        } => {
            walk_expr(cond, program, known_vars, visited, out);
            walk_block(then_block, program, known_vars, visited, out);
            walk_block(else_block, program, known_vars, visited, out);
        }
        IrExpr::GenericCall { args, .. } => {
            for a in args {
                walk_expr(&a.value, program, known_vars, visited, out);
            }
        }
        IrExpr::Retry { body, else_expr, .. } => {
            walk_block(body, program, known_vars, visited, out);
            if let Some(e) = else_expr {
                walk_expr(e, program, known_vars, visited, out);
            }
        }
        IrExpr::Match { scrutinee, arms } => {
            walk_expr(scrutinee, program, known_vars, visited, out);
            for arm in arms {
                match &arm.body {
                    IrArmBody::Expr(e) => walk_expr(e, program, known_vars, visited, out),
                    IrArmBody::Block(b) => walk_block(b, program, known_vars, visited, out),
                }
            }
        }
        IrExpr::For { iter, body, .. } => {
            walk_expr(iter, program, known_vars, visited, out);
            walk_block(body, program, known_vars, visited, out);
        }
        IrExpr::While { cond, body } => {
            walk_expr(cond, program, known_vars, visited, out);
            walk_block(body, program, known_vars, visited, out);
        }
        IrExpr::Break(e) => {
            if let Some(e) = e {
                walk_expr(e, program, known_vars, visited, out);
            }
        }
        IrExpr::Parallel(members) => {
            for (_, e) in members {
                walk_expr(e, program, known_vars, visited, out);
            }
        }
        IrExpr::OpaqueCall { callee, args } => {
            walk_expr(callee, program, known_vars, visited, out);
            for a in args {
                walk_expr(&a.value, program, known_vars, visited, out);
            }
        }
        IrExpr::Record(fields) => {
            for (_, e) in fields {
                walk_expr(e, program, known_vars, visited, out);
            }
        }
        IrExpr::TextBlock(parts) => {
            for p in parts {
                if let IrTextPart::Interp(e) = p {
                    walk_expr(e, program, known_vars, visited, out);
                }
            }
        }
        IrExpr::Int(_) | IrExpr::Float(_) | IrExpr::Str(_) | IrExpr::Var(_) | IrExpr::RowRef => {}
    }
}

fn walk_effect(
    effect: &IrEffect,
    program: &IrProgram,
    known_vars: &BTreeMap<String, String>,
    visited: &mut HashSet<String>,
    out: &mut Vec<PlannedCall>,
) {
    match effect {
        IrEffect::Ask {
            capability,
            args,
            messages,
        } => {
            let provider_arg = args.iter().find(|a| a.name.as_deref() == Some("provider"));
            let provider_arg = provider_arg.and_then(|a| match &a.value {
                IrExpr::Str(s) => Some(s.clone()),
                _ => None,
            });
            let (low, high) = messages
                .iter()
                .map(|(_, e)| approx_expr_chars(e, known_vars))
                .fold((0, 0), |acc, (l, h)| (acc.0 + l, acc.1 + h));
            out.push(PlannedCall {
                capability: capability.clone(),
                label: capability.clone(),
                provider_arg,
                prompt_chars_low: low,
                prompt_chars_high: high,
            });
            for a in args {
                walk_expr(&a.value, program, known_vars, visited, out);
            }
        }
        IrEffect::Judge { name, args } => {
            let rubric_chars = program
                .judges
                .iter()
                .find(|j| &j.name == name)
                .and_then(|j| j.fields.iter().find(|(k, _)| k == "rubric"))
                .map(|(_, e)| approx_expr_chars(e, known_vars))
                .unwrap_or((0, 0));
            let subject_chars = args
                .first()
                .map(|a| approx_expr_chars(&a.value, known_vars))
                .unwrap_or((0, 0));
            out.push(PlannedCall {
                capability: "judge".to_string(),
                label: format!("judge {name}"),
                provider_arg: None,
                prompt_chars_low: rubric_chars.0 + subject_chars.0,
                prompt_chars_high: rubric_chars.1 + subject_chars.1,
            });
            for a in args {
                walk_expr(&a.value, program, known_vars, visited, out);
            }
        }
        // Validators run entirely locally (regex/json_schema, see
        // `interp.rs`'s `eval_rubric_call`) — never resolve a provider, so
        // there's no row to plan here, just nested exprs to keep walking.
        IrEffect::Validator { args, .. } => {
            for a in args {
                walk_expr(&a.value, program, known_vars, visited, out);
            }
        }
        // `escalate` suspends for a human decision — it never calls a
        // model/vendor provider either (`eval_escalate` doesn't touch
        // `ctx.providers` at all), so it isn't a capability row.
        IrEffect::Escalate { args, .. } => {
            for (_, e) in args {
                walk_expr(e, program, known_vars, visited, out);
            }
        }
        IrEffect::ConversationCall { name, args } => {
            for a in args {
                walk_expr(&a.value, program, known_vars, visited, out);
            }
            // Only ever expands a call into another conversation declared
            // in *this same file* (`ulx-ir` only lowers the entry file's
            // own `Program` — see `pipeline::load`'s doc comment) and only
            // once per name, so mutual/self recursion terminates instead
            // of looping forever; a cross-file call is silently left
            // unexpanded rather than erroring the whole plan over it.
            if visited.insert(name.clone()) {
                if let Some(target) = find_conversation(program, name) {
                    // A fresh, empty `known_vars`: a callee's own top-level
                    // parameters aren't statically tied to the caller's
                    // `--arg` values by this pass, so they fall back to the
                    // generic unknown-length allowance same as any other
                    // dynamic binding.
                    walk_block(&target.body, program, &BTreeMap::new(), visited, out);
                }
            }
        }
    }
}

fn find_conversation<'a>(program: &'a IrProgram, name: &str) -> Option<&'a IrConversation> {
    program.conversations.iter().find(|c| c.name == name)
}

/// Best-effort static character-length range `(low, high)` of an
/// interpolated text expression: literal text contributes its exact length
/// to both bounds, anything whose runtime value this pass can't know
/// (a variable not covered by `known_vars`, a field access, a nested call's
/// result, a dataset row, ...) contributes the generic unknown-value
/// allowance instead.
fn approx_expr_chars(expr: &IrExpr, known_vars: &BTreeMap<String, String>) -> (usize, usize) {
    match expr {
        IrExpr::Str(s) => (s.len(), s.len()),
        IrExpr::TextBlock(parts) => parts.iter().fold((0, 0), |acc, p| match p {
            IrTextPart::Literal(s) => (acc.0 + s.len(), acc.1 + s.len()),
            IrTextPart::Interp(e) => {
                let (l, h) = approx_expr_chars(e, known_vars);
                (acc.0 + l, acc.1 + h)
            }
        }),
        IrExpr::Var(name) => match known_vars.get(name) {
            Some(v) => (v.len(), v.len()),
            None => (UNKNOWN_CHARS_LOW, UNKNOWN_CHARS_HIGH),
        },
        IrExpr::Int(_) | IrExpr::Float(_) => (1, 20),
        _ => (0, UNKNOWN_CHARS_HIGH),
    }
}

struct CostEstimate {
    tokens_low: u64,
    tokens_high: u64,
    cost_low: f64,
    cost_high: f64,
    priced: bool,
}

/// A flat "typical response length" allowance per capability, since a
/// static pass has no way to know how long a model's reply will actually
/// be — `(low, high)` completion tokens.
fn completion_token_range(capability: &str) -> (u64, u64) {
    match capability {
        "judge" => (10, 80),
        "embed" => (0, 0),
        _ => (100, 600),
    }
}

/// Illustrative, approximate `$`/1K-token (or `$`/call, for non-token
/// capabilities) rates circa mid-2024 vendor pricing pages — **not** live
/// pricing, and nowhere near a complete model list. Update from each
/// vendor's own pricing page before relying on this for a real budget;
/// matched against a resolved model name case-insensitively, exact match
/// first, then the longest known key that's a substring of it (so e.g.
/// `gpt-4o-mini-2024-07-18` or an Azure deployment name containing
/// `gpt-4o-mini` still finds the `gpt-4o-mini` row).
#[derive(Debug, Clone, Copy)]
enum Rate {
    PerToken { input_per_1k: f64, output_per_1k: f64 },
    PerCall(f64),
    Free,
}

fn pricing_table() -> &'static [(&'static str, Rate)] {
    &[
        (
            "gpt-4o-mini",
            Rate::PerToken {
                input_per_1k: 0.00015,
                output_per_1k: 0.0006,
            },
        ),
        (
            "gpt-4o",
            Rate::PerToken {
                input_per_1k: 0.0025,
                output_per_1k: 0.01,
            },
        ),
        (
            "gpt-3.5-turbo",
            Rate::PerToken {
                input_per_1k: 0.0005,
                output_per_1k: 0.0015,
            },
        ),
        (
            "claude-3-5-sonnet",
            Rate::PerToken {
                input_per_1k: 0.003,
                output_per_1k: 0.015,
            },
        ),
        (
            "claude-3-haiku",
            Rate::PerToken {
                input_per_1k: 0.00025,
                output_per_1k: 0.00125,
            },
        ),
        (
            "gemini-1.5-flash",
            Rate::PerToken {
                input_per_1k: 0.000075,
                output_per_1k: 0.0003,
            },
        ),
        (
            "gemini-1.5-pro",
            Rate::PerToken {
                input_per_1k: 0.00125,
                output_per_1k: 0.005,
            },
        ),
        (
            "llama-3.3-70b-versatile",
            Rate::PerToken {
                input_per_1k: 0.00059,
                output_per_1k: 0.00079,
            },
        ),
        (
            "llama-3-8b",
            Rate::PerToken {
                input_per_1k: 0.0002,
                output_per_1k: 0.0002,
            },
        ),
        ("llama3", Rate::Free), // ollama: local, no vendor cost
        (
            "command-r",
            Rate::PerToken {
                input_per_1k: 0.00015,
                output_per_1k: 0.0006,
            },
        ),
        (
            "text-embedding-3-small",
            Rate::PerToken {
                input_per_1k: 0.00002,
                output_per_1k: 0.0,
            },
        ),
        (
            "text-embedding-004",
            Rate::PerToken {
                input_per_1k: 0.00001,
                output_per_1k: 0.0,
            },
        ),
        (
            "embed-english-v3.0",
            Rate::PerToken {
                input_per_1k: 0.0001,
                output_per_1k: 0.0,
            },
        ),
        ("nomic-embed-text", Rate::Free), // ollama: local
        ("whisper-1", Rate::PerCall(0.006)),
        ("whisper-large-v3", Rate::PerCall(0.0004)), // groq: far cheaper than OpenAI's hosted whisper
        ("tts-1", Rate::PerCall(0.015)),
        ("dall-e-3", Rate::PerCall(0.04)),
    ]
}

fn lookup_rate(model: &str) -> Option<Rate> {
    let table = pricing_table();
    let model_lower = model.to_lowercase();
    if let Some((_, rate)) = table.iter().find(|(k, _)| *k == model_lower) {
        return Some(*rate);
    }
    table
        .iter()
        .filter(|(k, _)| model_lower.contains(k))
        .max_by_key(|(k, _)| k.len())
        .map(|(_, rate)| *rate)
}

/// Non-token capabilities (`transcribe`/`speak`/`generate_image`) are
/// priced per call, not per estimated token — this pass still shows a
/// token count of `0` for those rows rather than hiding the column.
fn is_per_call_capability(capability: &str) -> bool {
    matches!(capability, "transcribe" | "speak" | "generate_image")
}

fn estimate_cost(
    capability: &str,
    vendor: &str,
    model: &str,
    prompt_chars_low: usize,
    prompt_chars_high: usize,
) -> CostEstimate {
    // The mock provider (§the `mock` vendor, `ulx-runtime`'s `MockProvider`)
    // never calls a real vendor at all — it's always free, regardless of
    // whatever placeholder model name a manifest gives it.
    let rate = if vendor == "mock" {
        Some(Rate::Free)
    } else {
        lookup_rate(model)
    };

    if is_per_call_capability(capability) {
        return match rate {
            Some(Rate::PerCall(r)) => CostEstimate {
                tokens_low: 0,
                tokens_high: 0,
                cost_low: r,
                cost_high: r,
                priced: true,
            },
            Some(Rate::Free) => CostEstimate {
                tokens_low: 0,
                tokens_high: 0,
                cost_low: 0.0,
                cost_high: 0.0,
                priced: true,
            },
            _ => CostEstimate {
                tokens_low: 0,
                tokens_high: 0,
                cost_low: 0.0,
                cost_high: 0.0,
                priced: false,
            },
        };
    }

    let prompt_tokens_low = (prompt_chars_low as f64 / CHARS_PER_TOKEN).ceil() as u64;
    let prompt_tokens_high = (prompt_chars_high as f64 / CHARS_PER_TOKEN).ceil() as u64;
    let (completion_low, completion_high) = completion_token_range(capability);
    let tokens_low = prompt_tokens_low + completion_low;
    let tokens_high = prompt_tokens_high + completion_high;

    match rate {
        Some(Rate::PerToken {
            input_per_1k,
            output_per_1k,
        }) => CostEstimate {
            tokens_low,
            tokens_high,
            cost_low: (prompt_tokens_low as f64 / 1000.0) * input_per_1k
                + (completion_low as f64 / 1000.0) * output_per_1k,
            cost_high: (prompt_tokens_high as f64 / 1000.0) * input_per_1k
                + (completion_high as f64 / 1000.0) * output_per_1k,
            priced: true,
        },
        Some(Rate::Free) => CostEstimate {
            tokens_low,
            tokens_high,
            cost_low: 0.0,
            cost_high: 0.0,
            priced: true,
        },
        Some(Rate::PerCall(r)) => CostEstimate {
            tokens_low,
            tokens_high,
            cost_low: r,
            cost_high: r,
            priced: true,
        },
        None => CostEstimate {
            tokens_low,
            tokens_high,
            cost_low: 0.0,
            cost_high: 0.0,
            priced: false,
        },
    }
}

/// Prints the plan table to stdout. Returns `true` iff every row resolved
/// to a provider (an unresolved row — `Ambiguous`/`UnknownCapability`/etc.
/// — makes the whole `ulx plan` invocation exit non-zero, same as it would
/// abort a real `ulx run`).
pub fn print_plan(conversation: &str, rows: &[PlannedRow]) -> bool {
    println!("plan for `{conversation}` — static analysis only, nothing was called\n");

    if rows.is_empty() {
        println!("(no `ask`/`judge` capability calls found in this conversation)");
        return true;
    }

    println!(
        "{:<3} {:<16} {:<20} {:<26} {:>13} {:>24}",
        "#", "capability", "provider (vendor)", "model", "est. tokens", "est. cost (USD)"
    );
    let mut total_low = 0.0;
    let mut total_high = 0.0;
    let mut any_unpriced = false;
    let mut ok = true;
    for (i, row) in rows.iter().enumerate() {
        match &row.resolution {
            Ok(target) => {
                let provider_col = format!("{} ({})", target.provider, target.vendor);
                let tokens = format!("{}-{}", row.tokens_low, row.tokens_high);
                let cost = if row.priced {
                    total_low += row.cost_low;
                    total_high += row.cost_high;
                    format!("${:.6}-${:.6}", row.cost_low, row.cost_high)
                } else {
                    any_unpriced = true;
                    "n/a (unpriced model)".to_string()
                };
                println!(
                    "{:<3} {:<16} {:<20} {:<26} {:>13} {:>24}",
                    i + 1,
                    row.label,
                    provider_col,
                    target.model,
                    tokens,
                    cost
                );
            }
            Err(msg) => {
                ok = false;
                println!(
                    "{:<3} {:<16} {:<20} {:<26} {:>13} {:>24}",
                    i + 1,
                    row.label,
                    "ERROR",
                    msg,
                    "-",
                    "-"
                );
            }
        }
    }

    println!();
    println!(
        "naive total across {} call site(s) (a straight sum — does not account for `retry`/`for`/`while` \
         executing a call more or fewer times, or mutually-exclusive `match` arms): ${:.6}-${:.6}{}",
        rows.len(),
        total_low,
        total_high,
        if any_unpriced { " (excludes unpriced row(s) above)" } else { "" }
    );
    println!(
        "\nnote: token counts are a heuristic (prompt character length / {CHARS_PER_TOKEN:.0}, with a wide \
         allowance for statically-unknown interpolated values) — not a real tokenizer. Per-model pricing is \
         a small illustrative table of approximate rates (crates/ulx-cli/src/plan.rs), not live vendor \
         pricing."
    );

    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use ulx_runtime::{build_provider, ProviderSpec};

    const TRANSLATE_SRC: &str = r#"
judge Fluency(subject: text) -> Verdict {
  rubric: """Is this an accurate, fluent translation of the source? Answer Pass, Fail(reason), or Escalate if you cannot tell."""
}

conversation Translate(source: text, target_lang: text) -> text {
  system: """You are a professional translator."""
  user: """Translate to {target_lang}: {source}"""
  assistant -> draft: text

  match judge Fluency(draft) {
    Pass          => draft
    Fail(reason)  => retry(2) {
                        user: """The previous translation was rejected: {reason}. Try again."""
                        assistant -> draft
                      } else escalate(human_approval, reason: reason)
    Escalate      => escalate(human_approval, reason: "judge could not decide")
    Score(_)      => draft
  }
}
"#;

    fn lower(src: &str) -> ulx_ir::IrProgram {
        let program = ulx_syntax::parse_source(src).expect("must parse");
        let diags = ulx_sema::analyze(&program);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == ulx_sema::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "semantic errors: {errors:?}");
        ulx_ir::lower_program(&program).expect("must lower")
    }

    /// A mock-vendor registry configured for both `chat` and `judge`,
    /// registered under one provider name — mirrors what
    /// `providers::resolve_providers_with_info` would build from a
    /// `ulexite.toml` `[providers.testprov]` entry with `vendor = "mock"`.
    ///
    /// Only *one* `MockProvider` instance is registered here even though it
    /// backs two capabilities: `MockProvider::supports` unconditionally
    /// claims every capability regardless of the `ProviderSpec.capability`
    /// it was built with (it's the one deliberately vendor-agnostic,
    /// all-capability stand-in — see `provider::mock`'s docs), so
    /// registering a second instance under the same name would make `chat`
    /// ambiguous against itself.
    fn mock_registry_and_infos() -> (ProviderRegistry, BTreeMap<String, ProviderInfo>) {
        let mut registry = ProviderRegistry::new();
        let spec = ProviderSpec {
            vendor: "mock".to_string(),
            capability: "chat".to_string(),
            ..Default::default()
        };
        registry.register("testprov", build_provider(&spec).unwrap());
        let mut models = BTreeMap::new();
        models.insert("chat".to_string(), "mock-chat-model".to_string());
        models.insert("judge".to_string(), "mock-judge-model".to_string());
        let mut infos = BTreeMap::new();
        infos.insert(
            "testprov".to_string(),
            ProviderInfo {
                vendor: "mock".to_string(),
                models,
            },
        );
        (registry, infos)
    }

    #[test]
    fn plans_translate_examples_chat_and_judge_calls() {
        let ir = lower(TRANSLATE_SRC);
        let (registry, infos) = mock_registry_and_infos();
        let rows = build_plan(&ir, "Translate", &registry, &infos, &BTreeMap::new()).unwrap();

        // The initial `assistant -> draft` chat call and the retry body's
        // second `assistant -> draft` chat call, plus the `judge Fluency`
        // call in the `match` scrutinee.
        let chat_rows: Vec<_> = rows.iter().filter(|r| r.label == "chat").collect();
        assert_eq!(chat_rows.len(), 2, "expected two chat call sites: {rows:?}");
        let judge_rows: Vec<_> = rows.iter().filter(|r| r.label == "judge Fluency").collect();
        assert_eq!(judge_rows.len(), 1, "expected one judge call site: {rows:?}");

        for row in chat_rows.iter().chain(judge_rows.iter()) {
            let target = row.resolution.as_ref().unwrap_or_else(|e| {
                panic!("expected `{}` to resolve, got error: {e}", row.label)
            });
            assert_eq!(target.provider, "testprov");
            assert_eq!(target.vendor, "mock");
            assert!(row.tokens_low > 0);
            assert!(row.tokens_high >= row.tokens_low);
        }
        assert_eq!(
            judge_rows[0].resolution.as_ref().unwrap().model,
            "mock-judge-model"
        );
        assert_eq!(
            chat_rows[0].resolution.as_ref().unwrap().model,
            "mock-chat-model"
        );
        // vendor == "mock" is always free, regardless of the placeholder model name.
        assert_eq!(chat_rows[0].cost_low, 0.0);
        assert_eq!(chat_rows[0].cost_high, 0.0);
        assert!(chat_rows[0].priced);
    }

    #[test]
    fn unknown_capability_is_reported_as_an_error_row_not_a_panic() {
        let ir = lower(TRANSLATE_SRC);
        let registry = ProviderRegistry::new(); // nothing registered at all
        let infos = BTreeMap::new();
        let rows = build_plan(&ir, "Translate", &registry, &infos, &BTreeMap::new()).unwrap();
        assert!(rows.iter().any(|r| r.resolution.is_err()));
        assert!(!print_plan("Translate", &rows));
    }

    #[test]
    fn unknown_conversation_name_is_a_clear_error() {
        let ir = lower(TRANSLATE_SRC);
        let (registry, infos) = mock_registry_and_infos();
        let err = build_plan(&ir, "NoSuchConversation", &registry, &infos, &BTreeMap::new())
            .unwrap_err();
        assert!(err.contains("NoSuchConversation"));
    }

    #[test]
    fn ambiguous_chat_capability_is_reported_not_panicked() {
        let ir = lower(TRANSLATE_SRC);
        let mut registry = ProviderRegistry::new();
        let mut infos = BTreeMap::new();
        // `openai_compatible` (unlike `mock`) actually respects
        // `ProviderSpec.capability` — `OpenAiCompatibleProvider::supports`
        // only claims the one capability it was built for — so this is the
        // vendor to use whenever a test needs a provider that supports
        // exactly one capability and no others.
        for name in ["a", "b"] {
            let spec = ProviderSpec {
                vendor: "openai_compatible".to_string(),
                capability: "chat".to_string(),
                base_url: Some("http://localhost:1/v1".to_string()),
                ..Default::default()
            };
            registry.register(name, build_provider(&spec).unwrap());
            let mut models = BTreeMap::new();
            models.insert("chat".to_string(), "some-chat-model".to_string());
            infos.insert(
                name.to_string(),
                ProviderInfo {
                    vendor: "openai_compatible".to_string(),
                    models,
                },
            );
        }
        // `judge` only has one candidate (registered under "a") so it can
        // still resolve unambiguously — isolating the ambiguity to `chat`.
        let judge_spec = ProviderSpec {
            vendor: "openai_compatible".to_string(),
            capability: "judge".to_string(),
            base_url: Some("http://localhost:1/v1".to_string()),
            ..Default::default()
        };
        registry.register("a", build_provider(&judge_spec).unwrap());
        infos
            .get_mut("a")
            .unwrap()
            .models
            .insert("judge".to_string(), "some-judge-model".to_string());

        let rows = build_plan(&ir, "Translate", &registry, &infos, &BTreeMap::new()).unwrap();
        let chat_rows: Vec<_> = rows.iter().filter(|r| r.label == "chat").collect();
        assert_eq!(chat_rows.len(), 2);
        for row in &chat_rows {
            let err = row.resolution.as_ref().unwrap_err();
            assert!(err.contains("multiple providers") && err.contains("chat"));
        }
        let judge_row = rows.iter().find(|r| r.label == "judge Fluency").unwrap();
        assert_eq!(judge_row.resolution.as_ref().unwrap().provider, "a");
    }

    #[test]
    fn known_arg_replaces_the_generic_unknown_allowance() {
        let ir = lower(TRANSLATE_SRC);
        let (registry, infos) = mock_registry_and_infos();
        let mut known_vars = BTreeMap::new();
        known_vars.insert("source".to_string(), "hi".to_string());
        known_vars.insert("target_lang".to_string(), "French".to_string());

        let rows_unknown = build_plan(&ir, "Translate", &registry, &infos, &BTreeMap::new()).unwrap();
        let rows_known = build_plan(&ir, "Translate", &registry, &infos, &known_vars).unwrap();

        let first_unknown = rows_unknown.iter().find(|r| r.label == "chat").unwrap();
        let first_known = rows_known.iter().find(|r| r.label == "chat").unwrap();
        // Supplying short, known arg values should never make the estimate
        // go *up* relative to the generic unknown-value allowance.
        assert!(first_known.tokens_high <= first_unknown.tokens_high);
    }
}
