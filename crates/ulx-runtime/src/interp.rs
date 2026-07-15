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

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use ulx_ast::{BinaryOp, MessageRole, UnaryOp};
use ulx_ir::*;

use crate::cache::cache_key;
use crate::env::Env;
use crate::error::RuntimeError;
use crate::provider::{Invocation, Message, ProviderError};
use crate::value::{DraftOutcome, Value, Verdict};
use crate::{stdlib, Args, RunContext};

thread_local! {
    /// The chain of `with`-block branch indices from the run's root down
    /// to whichever branch is currently executing on *this* thread — empty
    /// at the top level. `eval_parallel` extends it (by one more index)
    /// for each spawned branch; `std::thread::scope`'s `spawn` always
    /// starts a genuine new OS thread, so a freshly spawned branch's copy
    /// of this thread-local starts back at its default (empty `Vec`) and
    /// has to be explicitly seeded with its parent's path plus its own
    /// index right away (see `eval_parallel`) — it is never inherited
    /// automatically.
    static ESCALATE_PATH: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    /// How many `escalate` calls *this thread* has evaluated so far along
    /// its current path — disambiguates repeated escalates reached
    /// sequentially (e.g. across `retry` iterations) within one branch,
    /// the same role the old run-wide sequence counter played before a
    /// `with` block's parallel branches could race for the next value.
    static ESCALATE_LOCAL_SEQ: Cell<u64> = const { Cell::new(0) };
}

pub fn run_conversation(ctx: &RunContext, name: &str, args: Args) -> Result<Value, RuntimeError> {
    // `run_conversation` is the one entry point for a fresh top-level run
    // (the CLI's original run, its `--interactive` resume loop, and
    // `approve`/`deny`'s probe-then-resume passes all call it directly —
    // never recursively from within the interpreter itself, since a
    // program calling another conversation goes through
    // `eval_conversation_call` instead). Resetting here, rather than
    // relying on a fresh OS thread/process to start these thread-locals at
    // their defaults, matters because a single process can legitimately
    // call `run_conversation` more than once on the *same* thread (both
    // ends of `--interactive`'s suspend/resume loop, and this crate's own
    // tests) — without an explicit reset, the second call would inherit
    // the first call's already-advanced counters and compute different
    // `escalate` cache keys than the first call recorded.
    ESCALATE_PATH.with(|p| p.borrow_mut().clear());
    ESCALATE_LOCAL_SEQ.with(|c| c.set(0));

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

/// One `assert`/`expect`/`snapshot` check's outcome for a single dataset
/// row (§16.1: all three ultimately produce the same pass/fail-shaped
/// result the compiler and reporter treat uniformly). `kind` is one of
/// `"assert"`, `"expect"`, `"snapshot"` — a plain string rather than a new
/// enum since it's purely a report label, never matched on.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub kind: &'static str,
    pub passed: bool,
    pub message: Option<String>,
}

/// The outcome of running a `benchmark`'s body once for one dataset row
/// (§16.2's "N cases, N reports" ergonomic).
#[derive(Debug, Clone)]
pub struct BenchmarkRowResult {
    pub row_index: usize,
    pub checks: Vec<CheckResult>,
}

impl BenchmarkRowResult {
    /// A row passes iff every check it produced passed (an empty check list
    /// — a benchmark whose body is only `run:` statements — counts as a
    /// pass, same as an empty `all()`).
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }
}

/// The full report for one `benchmark` run: one row per dataset entry.
#[derive(Debug, Clone)]
pub struct BenchmarkReport {
    pub name: String,
    pub rows: Vec<BenchmarkRowResult>,
}

impl BenchmarkReport {
    pub fn total(&self) -> usize {
        self.rows.len()
    }

    pub fn passed_count(&self) -> usize {
        self.rows.iter().filter(|r| r.passed()).count()
    }

    pub fn all_passed(&self) -> bool {
        self.rows.iter().all(|r| r.passed())
    }
}

/// Executes a `benchmark` declaration (§16.4): resolves its `dataset:`
/// statement via `crate::dataset::load` (the same loader `resolve_var` uses
/// for an ordinary `dataset` reference), then runs the benchmark's
/// remaining statements once per row with `$` bound to that row (§16.2) —
/// exactly the way a conversation body runs, reusing `eval_expr` for
/// `run:`'s conversation call, `expect ... satisfies judge ...`'s judge
/// call, and `assert`'s boolean expression.
///
/// Scope, honestly: this is a narrower executor than §16 describes in
/// full. `expect`'s "resample/re-evaluate until the verdict converges"
/// polling (§16.3) isn't implemented — a judge call is evaluated exactly
/// once, same as `match judge ... {}` elsewhere in this interpreter.
/// `snapshot` (§16.5) doesn't record/compare against a golden baseline
/// file yet — it evaluates its expression and key (so a real effectful
/// subexpression still runs and any error still surfaces) and always
/// reports "recorded", since there's no `--update-snapshots` flag or
/// baseline store wired up. There's also no `metrics.*` aggregation
/// (§16.6) or JUnit/JSON report format — `BenchmarkReport` is a plain
/// in-memory pass/fail-per-row structure for `ulx bench` to print.
pub fn run_benchmark(ctx: &RunContext, name: &str) -> Result<BenchmarkReport, RuntimeError> {
    let benchmark = ctx
        .program
        .benchmarks
        .iter()
        .find(|b| b.name == name)
        .ok_or_else(|| RuntimeError::UnknownBenchmark(name.to_string()))?;

    let dataset_name = benchmark
        .steps
        .iter()
        .find_map(|s| match s {
            IrBenchmarkStep::Dataset(n) => Some(n.clone()),
            _ => None,
        })
        .ok_or_else(|| {
            RuntimeError::TypeError(format!("benchmark `{name}` has no `dataset:` statement"))
        })?;

    let dataset = ctx
        .program
        .datasets
        .iter()
        .find(|d| d.name == dataset_name)
        .ok_or_else(|| RuntimeError::UnknownDataset(dataset_name.clone()))?;

    let rows = match crate::dataset::load(ctx, dataset)? {
        Value::List(rows) => rows,
        other => {
            return Err(RuntimeError::TypeError(format!(
                "dataset `{dataset_name}` did not load as a list of rows (got {other})"
            )))
        }
    };

    let mut report = BenchmarkReport {
        name: name.to_string(),
        rows: Vec::with_capacity(rows.len()),
    };

    for (row_index, row) in rows.into_iter().enumerate() {
        let mut env = Env::new();
        env.declare("$", row);
        let mut checks = Vec::new();

        for step in &benchmark.steps {
            match step {
                IrBenchmarkStep::Dataset(_) => {}
                IrBenchmarkStep::Run { expr, bind } => {
                    let value = eval_expr(ctx, expr, &mut env)?;
                    env.declare(bind.clone(), value);
                }
                IrBenchmarkStep::Expect {
                    expr,
                    judge,
                    threshold,
                } => {
                    // `expr` (the subject) is evaluated for effect/error
                    // propagation even though its value isn't otherwise
                    // used here directly — the judge call (almost always
                    // `judge Fluency(result)`) references the same bound
                    // name itself, so re-evaluating `expr` mirrors what a
                    // hand-written `match judge Fluency(result) {...}`
                    // would do.
                    eval_expr(ctx, expr, &mut env)?;
                    let verdict = eval_expr(ctx, judge, &mut env)?;
                    let (passed, message) = evaluate_expect_verdict(&verdict, *threshold);
                    checks.push(CheckResult {
                        kind: "expect",
                        passed,
                        message,
                    });
                }
                IrBenchmarkStep::Assert(expr) => {
                    let value = eval_expr(ctx, expr, &mut env)?;
                    let passed = value.truthy();
                    let message = if passed {
                        None
                    } else {
                        Some(format!("assertion failed (evaluated to {value})"))
                    };
                    checks.push(CheckResult {
                        kind: "assert",
                        passed,
                        message,
                    });
                }
                IrBenchmarkStep::Snapshot { expr, key } => {
                    let value = eval_expr(ctx, expr, &mut env)?;
                    let key_value = eval_expr(ctx, key, &mut env)?;
                    checks.push(CheckResult {
                        kind: "snapshot",
                        passed: true,
                        message: Some(format!("recorded `{key_value}`: {value}")),
                    });
                }
            }
        }

        report.rows.push(BenchmarkRowResult { row_index, checks });
    }

    Ok(report)
}

/// Interprets an `expect ... satisfies judge ... with threshold(t)`
/// verdict into a pass/fail (§16.4): `Pass` always passes, `Fail(reason)`
/// always fails with that reason, `Score(s)` passes iff `s >= threshold`
/// (or `s > 0.0` when no threshold was written), and `Escalate` — a judge
/// declining to decide — fails, since there's no human-in-the-loop
/// resolution inside a benchmark row (§16.3's polling/retry-until-converged
/// semantics aren't implemented either; a judge call happens exactly once).
fn evaluate_expect_verdict(verdict: &Value, threshold: Option<f64>) -> (bool, Option<String>) {
    match verdict {
        Value::Verdict(Verdict::Pass) => (true, None),
        Value::Verdict(Verdict::Fail(reason)) => (false, Some(reason.clone())),
        Value::Verdict(Verdict::Score(s)) => {
            let passed = match threshold {
                Some(t) => *s >= t,
                None => *s > 0.0,
            };
            let message = if passed {
                None
            } else {
                Some(format!(
                    "score {s} did not meet threshold {}",
                    threshold
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "(none)".to_string())
                ))
            };
            (passed, message)
        }
        Value::Verdict(Verdict::Escalate) => {
            (false, Some("judge could not decide (Escalate)".to_string()))
        }
        other => (
            false,
            Some(format!("`satisfies` expects a Verdict, got {other}")),
        ),
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
    // Read once, synchronously, on this (single) thread before spawning
    // anything -- so every branch below is seeded from the same parent
    // path regardless of how the branches themselves get scheduled.
    let parent_path = ESCALATE_PATH.with(|p| p.borrow().clone());
    let results: Vec<(String, Result<Value, RuntimeError>)> = std::thread::scope(|scope| {
        // Paired with its own name up front, since a panicking closure
        // never gets to return `(name.clone(), r)` itself -- the name has
        // to survive independently of the join outcome.
        let handles: Vec<(
            String,
            std::thread::ScopedJoinHandle<Result<Value, RuntimeError>>,
        )> = members
            .iter()
            .enumerate()
            .map(|(branch_index, (name, expr))| {
                let mut local_env = env.clone();
                let mut branch_path = parent_path.clone();
                branch_path.push(branch_index);
                let handle = scope.spawn(move || {
                    ESCALATE_PATH.with(|p| *p.borrow_mut() = branch_path);
                    eval_expr(ctx, expr, &mut local_env)
                });
                (name.clone(), handle)
            })
            .collect();
        handles
            .into_iter()
            .map(|(name, h)| {
                let r = h.join().unwrap_or_else(|payload| {
                    // A panic here (a bug in this interpreter, or in a
                    // provider adapter it calls into) must not abort the
                    // whole `ulx` process -- surface it as an ordinary
                    // error instead, consistent with this crate's own
                    // claim that a failure "surfaces as an unsettled
                    // `Draft<T>`, not a crash."
                    let msg = payload
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic payload".to_string());
                    Err(RuntimeError::Panicked(format!(
                        "with-block member `{name}` panicked: {msg}"
                    )))
                });
                (name, r)
            })
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
        if let IrExpr::Var(name) = base.as_ref() {
            // A local variable bound to a list takes precedence over
            // stdlib-module dispatch: `results.append(x)` (§21.6) mutates
            // the variable `results` in place, it isn't a call into some
            // `results` module.
            if matches!(env.get(name), Some(Value::List(_))) {
                return eval_list_method(ctx, name, field, args, env);
            }
            let evaluated = eval_args_named(ctx, args, env)?;
            if let Some(v) = stdlib::call(ctx, name, field, &evaluated)? {
                return Ok(v);
            }
            return Err(RuntimeError::NotImplemented(format!("{name}.{field}(...)")));
        }
    }
    Err(RuntimeError::NotImplemented(
        "calls to a computed/unknown callee".to_string(),
    ))
}

/// The small set of list-mutation methods `for`-loop bodies actually need
/// (§21.6's `results.append(...)` pattern) — not a general collections
/// library, just enough to make batch-accumulation examples real.
fn eval_list_method(
    ctx: &RunContext,
    var_name: &str,
    method: &str,
    args: &[IrArg],
    env: &mut Env,
) -> Result<Value, RuntimeError> {
    let arg_val = match args.first() {
        Some(a) => Some(eval_expr(ctx, &a.value, env)?),
        None => None,
    };
    let Some(Value::List(mut items)) = env.get(var_name).cloned() else {
        return Err(RuntimeError::TypeError(format!(
            "`{var_name}` is not a list"
        )));
    };
    match method {
        "append" | "push" => {
            items.push(arg_val.unwrap_or(Value::Unit));
            env.set(var_name, Value::List(items));
            Ok(Value::Unit)
        }
        other => Err(RuntimeError::NotImplemented(format!(
            "list method `.{other}(...)` (only `.append`/`.push` are implemented)"
        ))),
    }
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

    let provider = match invocation.args.get("provider").and_then(Value::as_text) {
        Some(name) => ctx
            .providers
            .resolve_named(capability, name)
            .map_err(RuntimeError::ProviderResolution)?,
        None => ctx
            .providers
            .resolve(capability)
            .map_err(RuntimeError::ProviderResolution)?,
    };

    let hash_inputs: Vec<Value> = invocation
        .messages
        .iter()
        .map(|m| Value::Text(format!("{}:{}", m.role, m.text)))
        .chain(invocation.args.values().cloned())
        .collect();
    let refs: Vec<&Value> = hash_inputs.iter().collect();
    let key = cache_key(capability, provider.id(), &refs, &[]);

    invoke_cached(ctx, capability, &key, || {
        settle(provider.invoke(capability, &invocation))
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

            // Judge/validator calls have no args bag to carry a `provider:`
            // selector the way `ask` does (their args are consumed
            // entirely into rubric parameters) — an ambiguous `"judge"`
            // capability can only be disambiguated globally, via
            // `--provider` on the CLI, not per call.
            let provider = ctx
                .providers
                .resolve("judge")
                .map_err(RuntimeError::ProviderResolution)?;
            let invocation = Invocation {
                messages: vec![],
                args: BTreeMap::from([
                    ("subject".to_string(), subject.clone()),
                    ("rubric".to_string(), rubric_text.clone()),
                ]),
            };
            let key = cache_key("judge", provider.id(), &[&subject, &rubric_text], &[name]);
            invoke_cached(ctx, "judge", &key, || {
                settle(provider.invoke("judge", &invocation))
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
    // mixing in `run_id` (in addition to this thread's `with`-block branch
    // path plus a local sequence number, which together disambiguate
    // multiple escalate call sites within one run — see `ESCALATE_PATH`'s
    // docs for why this isn't a single run-wide counter) keeps two
    // different runs that happen to reach an identically-worded escalation
    // from silently sharing a decision.
    let disambiguator = ESCALATE_PATH.with(|p| {
        let path = p.borrow();
        let local_seq = ESCALATE_LOCAL_SEQ.with(|c| {
            let v = c.get();
            c.set(v + 1);
            v
        });
        let path_str = path
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(".");
        format!("{path_str}#{local_seq}")
    });
    let refs: Vec<&Value> = evaluated.values().collect();
    let key = cache_key("escalate", target, &refs, &[&ctx.run_id, &disambiguator]);

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
    if !ctx.no_cache {
        if let Some(v) = ctx.cache.get(key) {
            ctx.trace
                .record("effect", Some(capability), Some(key), true, Some(&v), None);
            return Ok(v);
        }
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

/// A `Draft<T>` that didn't settle (§9.3) is represented dynamically as
/// `Value::Unsettled`. A real provider adapter's rate-limit/timeout/refusal
/// outcomes are legitimate unsettled results, not runtime failures, so they
/// become `Ok(Value::Unsettled(..))` here rather than propagating as
/// `RuntimeError::Provider` — every other `ProviderError` (bad request,
/// auth failure, malformed response) is a genuine hard error.
fn settle(result: Result<Value, ProviderError>) -> Result<Value, RuntimeError> {
    match result {
        Ok(v) => Ok(v),
        Err(ProviderError::RateLimited) => Ok(unsettled(DraftOutcome::RateLimited)),
        Err(ProviderError::Timeout) => Ok(unsettled(DraftOutcome::Timeout)),
        Err(ProviderError::Refused(reason)) => Ok(unsettled(DraftOutcome::Refused(reason))),
        Err(e) => Err(RuntimeError::Provider(e)),
    }
}

fn unsettled(outcome: DraftOutcome) -> Value {
    Value::Unsettled(outcome)
}
