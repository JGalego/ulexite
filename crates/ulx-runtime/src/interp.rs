//! The tree-walking interpreter (§12.2, §13.6 — IR is interpreted, not
//! natively compiled, since network-bound provider latency dwarfs any
//! interpretation overhead).
//!
//! **Escalate/human-approval resume, honestly documented**: a full
//! implementation of §10.4's checkpoint log would capture an arbitrary
//! continuation at the suspend point. This interpreter is an ordinary
//! recursive Rust function, so that would mean a CPS rewrite — real,
//! sizable work. Instead, v0.1 leans on the content-addressed cache
//! (§10.3): every effect (including `escalate`) is keyed by a deterministic
//! hash, and a suspended run's `Err(Suspended{cache_key, ..})` is resolved
//! by writing the human's decision into the cache under that exact key and
//! re-invoking the *same* conversation with the *same* arguments. Every
//! effect before the escalate point is a cache hit (free, no re-invocation
//! of the mock/real provider), and the escalate itself is now a cache hit
//! too — so execution proceeds past it. This is a genuine, working design,
//! not a stub, but it is a narrower guarantee than a real checkpoint log:
//! it requires the run to be deterministically replayable from the start,
//! which is true for everything this runtime executes today (§9.3's
//! `Draft<T>` boundary is exactly the set of things this relies on being
//! memoized) but would need real continuation capture to extend to
//! genuinely unbounded/infinite imperative loops before a suspend point.

use std::collections::BTreeMap;

use ulx_ast::{BinaryOp, MessageRole, UnaryOp};
use ulx_ir::*;

use crate::cache::cache_key;
use crate::env::Env;
use crate::error::RuntimeError;
use crate::provider::{Invocation, Message};
use crate::value::{DraftOutcome, Value, Verdict};
use crate::{stdlib, Args, RunContext};

pub fn run_conversation(ctx: &RunContext, name: &str, args: Args) -> Result<Value, RuntimeError> {
    let conv = ctx
        .program
        .conversations
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| RuntimeError::UnknownConversation(name.to_string()))?;

    let mut env = Env::new();
    for (pname, _) in &conv.params {
        let value = args.get(pname).cloned().ok_or_else(|| {
            RuntimeError::TypeError(format!("missing argument `{pname}` for `{name}`"))
        })?;
        env.declare(pname.clone(), value);
    }
    eval_block(ctx, &conv.body, &mut env)
}

/// Evaluates a block. If it has no explicit tail expression, the last
/// bound instruction's value is used instead (a v0.1 interpreter
/// convention — see module docs in `lib.rs` for why: it's what makes
/// `retry(n) { ...; assistant -> draft }`-style bodies, which never write a
/// trailing bare `draft`, produce the value callers obviously intend).
pub(crate) fn eval_block(
    ctx: &RunContext,
    block: &IrBlock,
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    env.push();
    let mut last_value = Value::Unit;
    for inst in &block.insts {
        last_value = eval_inst(ctx, inst, env)?;
    }
    let result = match &block.tail {
        Some(tail) => eval_expr(ctx, tail, env)?,
        None => last_value,
    };
    env.pop();
    Ok(result)
}

fn eval_inst(ctx: &RunContext, inst: &IrInst, env: &mut Env) -> Result<Value, RuntimeError> {
    let value = eval_expr(ctx, &inst.expr, env)?;
    if let Some(name) = &inst.bind {
        env.declare(name.clone(), value.clone());
    }
    Ok(value)
}

pub(crate) fn eval_expr(
    ctx: &RunContext,
    expr: &IrExpr,
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    match expr {
        IrExpr::Int(i) => Ok(Value::Int(*i)),
        IrExpr::Float(f) => Ok(Value::Float(*f)),
        IrExpr::Str(s) => Ok(Value::Text(s.clone())),
        IrExpr::TextBlock(parts) => {
            let mut out = String::new();
            for part in parts {
                match part {
                    IrTextPart::Literal(s) => out.push_str(s),
                    IrTextPart::Interp(e) => out.push_str(&eval_expr(ctx, e, env)?.to_string()),
                }
            }
            Ok(Value::Text(out))
        }
        IrExpr::Var(name) => resolve_var(ctx, name, env),
        IrExpr::RowRef => env
            .get("$")
            .cloned()
            .ok_or_else(|| RuntimeError::UndefinedName("$ (current dataset row)".to_string())),
        IrExpr::Record(fields) => {
            let mut out = BTreeMap::new();
            for (k, e) in fields {
                out.insert(k.clone(), eval_expr(ctx, e, env)?);
            }
            Ok(Value::Record(out))
        }
        IrExpr::FieldAccess { base, field } => {
            let base_val = eval_expr(ctx, base, env)?;
            match base_val {
                Value::Record(fields) => fields
                    .get(field)
                    .cloned()
                    .ok_or_else(|| RuntimeError::TypeError(format!("no field `{field}`"))),
                Value::List(items) if field == "length" => Ok(Value::Int(items.len() as i64)),
                Value::Text(s) if field == "length" => Ok(Value::Int(s.chars().count() as i64)),
                other => Err(RuntimeError::TypeError(format!(
                    "cannot access field `{field}` on {other}"
                ))),
            }
        }
        IrExpr::OpaqueCall { callee, args } => eval_opaque_call(ctx, callee, args, env),
        IrExpr::Index { base, index } => {
            let base_val = eval_expr(ctx, base, env)?;
            let idx_val = eval_expr(ctx, index, env)?;
            match (base_val, idx_val) {
                (Value::List(items), Value::Int(i)) => items
                    .get(i as usize)
                    .cloned()
                    .ok_or_else(|| RuntimeError::TypeError("index out of range".to_string())),
                _ => Err(RuntimeError::TypeError(
                    "indexing requires a list and an int".to_string(),
                )),
            }
        }
        IrExpr::Unary { op, expr } => {
            let v = eval_expr(ctx, expr, env)?;
            match (op, v) {
                (UnaryOp::Not, v) => Ok(Value::Bool(!v.truthy())),
                (UnaryOp::Neg, Value::Int(i)) => Ok(Value::Int(-i)),
                (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
                (UnaryOp::Neg, other) => {
                    Err(RuntimeError::TypeError(format!("cannot negate {other}")))
                }
            }
        }
        IrExpr::Binary { op, lhs, rhs } => eval_binary(ctx, *op, lhs, rhs, env),
        IrExpr::If {
            cond,
            then_block,
            else_block,
        } => {
            if eval_expr(ctx, cond, env)?.truthy() {
                eval_block(ctx, then_block, env)
            } else {
                eval_block(ctx, else_block, env)
            }
        }
        IrExpr::GenericCall { name, args, .. } => match name.as_str() {
            "list" => {
                let mut items = Vec::with_capacity(args.len());
                for a in args {
                    items.push(eval_expr(ctx, &a.value, env)?);
                }
                Ok(Value::List(items))
            }
            other => Err(RuntimeError::NotImplemented(format!(
                "generic constructor `{other}<...>`"
            ))),
        },
        IrExpr::Retry {
            count,
            body,
            else_expr,
        } => eval_retry(ctx, *count, body, else_expr, env),
        IrExpr::Match { scrutinee, arms } => eval_match(ctx, scrutinee, arms, env),
        IrExpr::For { var, iter, body } => {
            let iterable = eval_expr(ctx, iter, env)?;
            let Value::List(items) = iterable else {
                return Err(RuntimeError::TypeError("`for` requires a list".to_string()));
            };
            let mut last = Value::Unit;
            for item in items {
                env.push();
                env.declare(var.clone(), item);
                match eval_block(ctx, body, env) {
                    Ok(v) => last = v,
                    Err(e) => {
                        env.pop();
                        return Err(e);
                    }
                }
                env.pop();
            }
            Ok(last)
        }
        IrExpr::While { cond, body } => {
            let mut last = Value::Unit;
            while eval_expr(ctx, cond, env)?.truthy() {
                last = eval_block(ctx, body, env)?;
            }
            Ok(last)
        }
        IrExpr::Break(e) => match e {
            Some(e) => eval_expr(ctx, e, env),
            None => Ok(Value::Unit),
        },
        IrExpr::Parallel(members) => eval_parallel(ctx, members, env),
        IrExpr::Effect(effect) => eval_effect(ctx, effect, env),
    }
}

fn resolve_var(ctx: &RunContext, name: &str, env: &Env) -> Result<Value, RuntimeError> {
    if let Some(v) = env.get(name) {
        return Ok(v.clone());
    }
    if let Some(ds) = ctx.program.datasets.iter().find(|d| d.name == name) {
        return crate::dataset::load(ctx, ds);
    }
    Err(RuntimeError::UndefinedName(name.to_string()))
}

fn eval_binary(
    ctx: &RunContext,
    op: BinaryOp,
    lhs: &IrExpr,
    rhs: &IrExpr,
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    if matches!(op, BinaryOp::And) {
        let l = eval_expr(ctx, lhs, env)?;
        return if !l.truthy() {
            Ok(l)
        } else {
            eval_expr(ctx, rhs, env)
        };
    }
    if matches!(op, BinaryOp::Or) {
        let l = eval_expr(ctx, lhs, env)?;
        return if l.truthy() {
            Ok(l)
        } else {
            eval_expr(ctx, rhs, env)
        };
    }

    let l = eval_expr(ctx, lhs, env)?;
    let r = eval_expr(ctx, rhs, env)?;
    match op {
        BinaryOp::Eq => Ok(Value::Bool(l == r)),
        BinaryOp::Ne => Ok(Value::Bool(l != r)),
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            let (a, b) = (as_f64(&l), as_f64(&r));
            match (a, b) {
                (Some(a), Some(b)) => Ok(Value::Bool(match op {
                    BinaryOp::Lt => a < b,
                    BinaryOp::Le => a <= b,
                    BinaryOp::Gt => a > b,
                    BinaryOp::Ge => a >= b,
                    _ => unreachable!(),
                })),
                _ => Err(RuntimeError::TypeError(format!(
                    "cannot compare {l} and {r}"
                ))),
            }
        }
        BinaryOp::Add => match (l, r) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Text(a), Value::Text(b)) => Ok(Value::Text(a + &b)),
            (a, b) => Err(RuntimeError::TypeError(format!("cannot add {a} and {b}"))),
        },
        BinaryOp::Sub => match (as_f64(&l), as_f64(&r)) {
            (Some(a), Some(b)) => Ok(Value::Float(a - b)),
            _ => Err(RuntimeError::TypeError(format!(
                "cannot subtract {r} from {l}"
            ))),
        },
        BinaryOp::Mul => match (as_f64(&l), as_f64(&r)) {
            (Some(a), Some(b)) => Ok(Value::Float(a * b)),
            _ => Err(RuntimeError::TypeError(format!(
                "cannot multiply {l} and {r}"
            ))),
        },
        BinaryOp::Div => match (as_f64(&l), as_f64(&r)) {
            (Some(a), Some(b)) => Ok(Value::Float(a / b)),
            _ => Err(RuntimeError::TypeError(format!("cannot divide {l} by {r}"))),
        },
        BinaryOp::And | BinaryOp::Or => unreachable!("handled above"),
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn eval_retry(
    ctx: &RunContext,
    count: u64,
    body: &IrBlock,
    else_expr: &Option<Box<IrExpr>>,
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    let mut last_err = None;
    for _ in 0..count.max(1) {
        match eval_block(ctx, body, env) {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
    }
    match else_expr {
        Some(e) => eval_expr(ctx, e, env),
        None => Err(last_err.unwrap_or(RuntimeError::RetriesExhausted)),
    }
}

fn eval_match(
    ctx: &RunContext,
    scrutinee: &IrExpr,
    arms: &[IrMatchArm],
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    let value = eval_expr(ctx, scrutinee, env)?;
    let (variant_name, payload): (&str, Option<Value>) = match &value {
        Value::Verdict(Verdict::Pass) => ("Pass", None),
        Value::Verdict(Verdict::Fail(reason)) => ("Fail", Some(Value::Text(reason.clone()))),
        Value::Verdict(Verdict::Score(s)) => ("Score", Some(Value::Float(*s))),
        Value::Verdict(Verdict::Escalate) => ("Escalate", None),
        _ => ("", None),
    };

    let mut wildcard = None;
    for arm in arms {
        match &arm.pattern {
            IrPattern::Wildcard => wildcard = Some(arm),
            IrPattern::Variant { name, bindings } if name == variant_name => {
                env.push();
                if let (Some(binding_name), Some(p)) = (bindings.first(), &payload) {
                    env.declare(binding_name.clone(), p.clone());
                }
                let result = eval_arm_body(ctx, &arm.body, env);
                env.pop();
                return result;
            }
            _ => {}
        }
    }
    if let Some(arm) = wildcard {
        return eval_arm_body(ctx, &arm.body, env);
    }
    Err(RuntimeError::TypeError(format!(
        "non-exhaustive match at runtime: no arm for `{variant_name}`"
    )))
}

fn eval_arm_body(ctx: &RunContext, body: &IrArmBody, env: &mut Env) -> Result<Value, RuntimeError> {
    match body {
        IrArmBody::Expr(e) => eval_expr(ctx, e, env),
        IrArmBody::Block(b) => eval_block(ctx, b, env),
    }
}

fn eval_parallel(
    ctx: &RunContext,
    members: &[(String, IrExpr)],
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    let results: Vec<(String, Result<Value, RuntimeError>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = members
            .iter()
            .map(|(name, expr)| {
                let mut local_env = env.clone();
                scope.spawn(move || {
                    let r = eval_expr(ctx, expr, &mut local_env);
                    (name.clone(), r)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("with-block worker thread panicked"))
            .collect()
    });

    let mut last = Value::Unit;
    for (name, r) in results {
        let v = r?;
        env.declare(name, v.clone());
        last = v;
    }
    Ok(last)
}

fn eval_args_named(
    ctx: &RunContext,
    args: &[IrArg],
    env: &mut Env,
) -> Result<Vec<(Option<String>, Value)>, RuntimeError> {
    args.iter()
        .map(|a| Ok((a.name.clone(), eval_expr(ctx, &a.value, env)?)))
        .collect()
}

fn eval_opaque_call(
    ctx: &RunContext,
    callee: &IrExpr,
    args: &[IrArg],
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    // `capability(embed)` / `capability(pinned("..."))` (§5.5, §7.5, §12.4):
    // a compile-time-ish policy hint, not a value-producing call. v0.1 has
    // no real policy resolution (there's only ever `MockProvider`), so this
    // just captures the hint as an opaque, inspectable value rather than
    // erroring on a legitimate, spec-sanctioned expression form.
    if let IrExpr::Var(name) = callee {
        if name == "capability" {
            // The argument is typically a bare capability-kind identifier
            // (`embed`, `chat`, ...) — a symbolic policy hint, not a
            // variable reference, so it's read directly rather than
            // evaluated through `resolve_var` (which would (rightly) treat
            // a bare, undeclared `embed` as an undefined name).
            let label = match args.first().map(|a| &a.value) {
                Some(IrExpr::Var(kind)) => kind.clone(),
                Some(other) => eval_expr(ctx, other, env)?.to_string(),
                None => String::new(),
            };
            return Ok(Value::Text(format!("capability({label})")));
        }
    }
    if let IrExpr::FieldAccess { base, field } = callee {
        if let IrExpr::Var(module) = base.as_ref() {
            let evaluated = eval_args_named(ctx, args, env)?;
            if let Some(v) = stdlib::call(ctx, module, field, &evaluated)? {
                return Ok(v);
            }
            return Err(RuntimeError::NotImplemented(format!(
                "{module}.{field}(...)"
            )));
        }
    }
    Err(RuntimeError::NotImplemented(
        "calls to a computed/unknown callee".to_string(),
    ))
}

fn eval_effect(ctx: &RunContext, effect: &IrEffect, env: &mut Env) -> Result<Value, RuntimeError> {
    match effect {
        IrEffect::Ask {
            capability,
            args,
            messages,
        } => eval_ask(ctx, capability, args, messages, env),
        IrEffect::Judge { name, args } => eval_rubric_call(ctx, RubricKind::Judge, name, args, env),
        IrEffect::Validator { name, args } => {
            eval_rubric_call(ctx, RubricKind::Validator, name, args, env)
        }
        IrEffect::Escalate { target, args } => eval_escalate(ctx, target, args, env),
        IrEffect::ConversationCall { name, args } => eval_conversation_call(ctx, name, args, env),
    }
}

fn eval_ask(
    ctx: &RunContext,
    capability: &str,
    args: &[IrArg],
    messages: &[(MessageRole, IrExpr)],
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    let mut evaluated_messages = Vec::with_capacity(messages.len());
    for (role, expr) in messages {
        let text = eval_expr(ctx, expr, env)?.to_string();
        evaluated_messages.push(Message {
            role: role_name(*role).to_string(),
            text,
        });
    }
    let mut invocation_args = BTreeMap::new();
    for a in args {
        let v = eval_expr(ctx, &a.value, env)?;
        invocation_args.insert(a.name.clone().unwrap_or_else(|| "_".to_string()), v);
    }
    let invocation = Invocation {
        messages: evaluated_messages,
        args: invocation_args,
    };

    let provider = ctx
        .providers
        .resolve(capability)
        .ok_or_else(|| RuntimeError::UnknownCapability(capability.to_string()))?;

    let hash_inputs: Vec<Value> = invocation
        .messages
        .iter()
        .map(|m| Value::Text(format!("{}:{}", m.role, m.text)))
        .chain(invocation.args.values().cloned())
        .collect();
    let refs: Vec<&Value> = hash_inputs.iter().collect();
    let key = cache_key(capability, provider.id(), &refs, &[]);

    invoke_cached(ctx, capability, &key, || {
        provider
            .invoke(capability, &invocation)
            .map_err(RuntimeError::Provider)
    })
}

enum RubricKind {
    Judge,
    Validator,
}

fn eval_rubric_call(
    ctx: &RunContext,
    kind: RubricKind,
    name: &str,
    args: &[IrArg],
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    let rubrics = match kind {
        RubricKind::Judge => &ctx.program.judges,
        RubricKind::Validator => &ctx.program.validators,
    };
    let decl = rubrics
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| RuntimeError::UnknownJudgeOrValidator(name.to_string()))?;

    let mut call_env = Env::new();
    let evaluated: Vec<(Option<String>, Value)> = args
        .iter()
        .map(|a| Ok((a.name.clone(), eval_expr(ctx, &a.value, env)?)))
        .collect::<Result<_, RuntimeError>>()?;
    bind_rubric_params(&decl.params, &evaluated, &mut call_env)?;

    let subject = decl
        .params
        .first()
        .and_then(|(pname, _)| call_env.get(pname))
        .cloned()
        .unwrap_or(Value::Text(String::new()));

    match kind {
        RubricKind::Judge => {
            let rubric_field = decl
                .fields
                .iter()
                .find(|(k, _)| k == "rubric")
                .map(|(_, e)| e)
                .ok_or_else(|| {
                    RuntimeError::TypeError(format!("judge `{name}` has no `rubric` field"))
                })?;
            let rubric_text = eval_expr(ctx, rubric_field, &mut call_env)?;

            let provider = ctx
                .providers
                .resolve("judge")
                .ok_or_else(|| RuntimeError::UnknownCapability("judge".to_string()))?;
            let invocation = Invocation {
                messages: vec![],
                args: BTreeMap::from([
                    ("subject".to_string(), subject.clone()),
                    ("rubric".to_string(), rubric_text.clone()),
                ]),
            };
            let key = cache_key("judge", provider.id(), &[&subject, &rubric_text], &[name]);
            invoke_cached(ctx, "judge", &key, || {
                provider
                    .invoke("judge", &invocation)
                    .map_err(RuntimeError::Provider)
            })
        }
        RubricKind::Validator => {
            let subject_text = subject.as_text().unwrap_or_default().to_string();
            if let Some((_, e)) = decl.fields.iter().find(|(k, _)| k == "regex") {
                let pattern = eval_expr(ctx, e, &mut call_env)?;
                let pattern = pattern.as_text().unwrap_or_default();
                let verdict = crate::validator::run_regex(pattern, &subject_text);
                ctx.trace
                    .record("effect", Some("validator:regex"), None, false, None, None);
                Ok(Value::Verdict(verdict))
            } else if decl.fields.iter().any(|(k, _)| k == "json_schema") {
                let verdict = crate::validator::run_json_wellformed(&subject_text);
                ctx.trace.record(
                    "effect",
                    Some("validator:json_schema"),
                    None,
                    false,
                    None,
                    None,
                );
                Ok(Value::Verdict(verdict))
            } else {
                Err(RuntimeError::NotImplemented(format!(
                    "validator `{name}` has no supported field (only `regex`/`json_schema` are implemented)"
                )))
            }
        }
    }
}

fn bind_rubric_params(
    params: &[(String, ulx_ast::TypeExpr)],
    args: &[(Option<String>, Value)],
    env: &mut Env,
) -> Result<(), RuntimeError> {
    let mut positional = args.iter().filter(|(n, _)| n.is_none());
    for (pname, _) in params {
        if let Some((_, v)) = args
            .iter()
            .find(|(n, _)| n.as_deref() == Some(pname.as_str()))
        {
            env.declare(pname.clone(), v.clone());
        } else if let Some((_, v)) = positional.next() {
            env.declare(pname.clone(), v.clone());
        }
    }
    Ok(())
}

fn eval_escalate(
    ctx: &RunContext,
    target: &str,
    args: &[(String, IrExpr)],
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    let mut evaluated = BTreeMap::new();
    for (k, e) in args {
        evaluated.insert(k.clone(), eval_expr(ctx, e, env)?);
    }
    let reason = evaluated
        .get("reason")
        .map(|v| v.to_string())
        .unwrap_or_else(|| format!("escalation to {target}"));

    // Unlike `ask`/`judge`/`validator` calls (which *should* cache-hit
    // across unrelated runs when their inputs coincide, per §10.3), a human
    // decision belongs to one specific run's specific suspend point —
    // mixing in `run_id` (in addition to the per-context sequence number,
    // which disambiguates multiple escalate call sites within one run)
    // keeps two different runs that happen to reach an identically-worded
    // escalation from silently sharing a decision.
    let seq = ctx.next_seq();
    let refs: Vec<&Value> = evaluated.values().collect();
    let key = cache_key("escalate", target, &refs, &[&ctx.run_id, &seq.to_string()]);

    if let Some(decision) = ctx.cache.get(&key) {
        ctx.trace.record(
            "effect",
            Some("escalate"),
            Some(&key),
            true,
            Some(&decision),
            None,
        );
        return Ok(decision);
    }
    if ctx.replay_only {
        return Err(RuntimeError::ReplayMiss(format!(
            "escalate to `{target}` has no recorded decision and this is a strict replay"
        )));
    }
    ctx.trace.record(
        "effect",
        Some("escalate"),
        Some(&key),
        false,
        None,
        Some("suspended"),
    );
    Err(RuntimeError::Suspended {
        cache_key: key,
        reason,
        target: target.to_string(),
    })
}

fn eval_conversation_call(
    ctx: &RunContext,
    name: &str,
    args: &[IrArg],
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    let conv = ctx
        .program
        .conversations
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| RuntimeError::UnknownConversation(name.to_string()))?;

    let evaluated: Vec<(Option<String>, Value)> = args
        .iter()
        .map(|a| Ok((a.name.clone(), eval_expr(ctx, &a.value, env)?)))
        .collect::<Result<_, RuntimeError>>()?;

    let mut call_env = Env::new();
    let mut positional = evaluated.iter().filter(|(n, _)| n.is_none());
    for (pname, _) in &conv.params {
        if let Some((_, v)) = evaluated
            .iter()
            .find(|(n, _)| n.as_deref() == Some(pname.as_str()))
        {
            call_env.declare(pname.clone(), v.clone());
        } else if let Some((_, v)) = positional.next() {
            call_env.declare(pname.clone(), v.clone());
        } else {
            return Err(RuntimeError::TypeError(format!(
                "missing argument `{pname}` calling `{name}`"
            )));
        }
    }

    ctx.trace
        .record("call", Some(name), None, false, None, None);
    eval_block(ctx, &conv.body, &mut call_env)
}

fn role_name(role: MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
    }
}

fn invoke_cached(
    ctx: &RunContext,
    capability: &str,
    key: &str,
    call: impl FnOnce() -> Result<Value, RuntimeError>,
) -> Result<Value, RuntimeError> {
    if let Some(v) = ctx.cache.get(key) {
        ctx.trace
            .record("effect", Some(capability), Some(key), true, Some(&v), None);
        return Ok(v);
    }
    if ctx.replay_only {
        return Err(RuntimeError::ReplayMiss(format!(
            "no cached result for `{capability}` call (key {key}) during strict replay"
        )));
    }
    match call() {
        Ok(v) => {
            let _ = ctx.cache.put(key, &v);
            ctx.trace
                .record("effect", Some(capability), Some(key), false, Some(&v), None);
            Ok(v)
        }
        Err(e) => {
            ctx.trace.record(
                "effect",
                Some(capability),
                Some(key),
                false,
                None,
                Some(&e.to_string()),
            );
            Err(e)
        }
    }
}

/// Surfaced for completeness/documentation purposes: a `Draft<T>` that
/// didn't settle (§9.3) is represented dynamically as `Value::Unsettled`;
/// nothing in v0.1's mock-provider-only world produces one today (the mock
/// provider never refuses/rate-limits), but the constructor exists so a
/// future real provider adapter has somewhere to put this outcome without
/// a `Value` redesign.
#[allow(dead_code)]
fn unsettled(outcome: DraftOutcome) -> Value {
    Value::Unsettled(outcome)
}
