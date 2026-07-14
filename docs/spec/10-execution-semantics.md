# 10. Execution Semantics

## 10.1 The conversation as the unit of execution

A compiled `conversation` is a value satisfying one uniform protocol — `plan`, `run`, `stream`, `replay` — regardless of whether it is a single turn, a `with`-block of parallel sub-steps, or a parent conversation nesting children. This is deliberately modeled on LCEL's `Runnable` protocol (§2.3, §2.7): the single best idea in that survey is that a uniform interface across heterogeneous units lets streaming, batching, and tracing "fall out" of composition rather than needing bespoke support at every level. Ulexite fixes the one thing LCEL leaves informal: the protocol is a compiler-checked interface every `conversation`/`judge`/`validator` value satisfies by construction, not a convention every custom `Runnable` subclass must remember to implement correctly (§2.3's async/sync-drift criticism of LangChain).

## 10.2 Two schedulable regions: declarative and imperative

Per §4.5 and §7, a conversation body is imperative by default (ordinary sequential statements, `match`, `retry`) with an explicit declarative sub-region (`with`, §7.4, §9.7). The compiler builds an execution graph in two passes:

1. **Static dependency graph over `with` blocks.** Because §9.7 forbids sibling references inside a `with` block, every binding's dependency set is exactly its free variables from enclosing scope — computable without any data-flow analysis. Independent bindings are scheduled concurrently, subject to the runtime's concurrency cap (§12.3) and cached by content hash (§10.3).
2. **Sequential imperative trace outside `with` blocks.** Ordinary statements execute in program order; a step's actual dependency on a prior step is whatever it references, and the compiler still records this as edges in the trace graph (§18) for replay and visualization, even though it does not attempt to parallelize across them — this is the deliberate boundary drawn in §4.5 to avoid Airflow's "DAG must be statically known" failure mode (§2.4) for the genuinely sequential, decision-dependent part of a conversation.

## 10.3 Caching

Every `ask`/message-literal call and every `judge`/`validator` invocation is content-addressed: the cache key is a hash of (capability, resolved provider identity + version, all input artifacts' content hashes, all declared parameters — temperature, rubric text, schema). An identical call, anywhere in the program or across separate runs, is a cache hit — Bazel's hermetic, content-addressed build-action caching (§2.4) applied to model/tool calls. Caching is on by default; `ask ... { cache: off }` opts a specific call out (e.g., a deliberately-sampled call meant to vary run to run), the inverse of every framework surveyed, where caching (if present at all) is opt-in and framework-specific (Promptfoo's is the closest built-in analogue, §2.2).

## 10.4 Checkpointing and replay

Every conversation run persists a checkpoint after each statement — not opt-in, not a `durability=` parameter a caller can leave at a crash-prone default (contrast LangGraph's `exit`/`async`/`sync` knob, §2.3, which is real prior art for the *mechanism* but leaves the *default* a caller's choice). The checkpoint log is Ulexite's adaptation of Temporal's determinism/replay split (§2.4): everything deterministic (control flow, artifact routing, variable bindings) is reconstructed by replaying the log; every non-deterministic effect (a model call, a tool invocation, a wall-clock read) is recorded in the log as its *result*, not re-executed, during replay. Because §9.3 makes non-determinism a type (`Draft<T>`), the compiler can verify a conversation body contains no non-deterministic operation outside a construct the runtime knows how to memoize — the same guarantee Temporal enforces by unenforced convention (§2.4, §2.8), Ulexite enforces by type-checking.

A checkpoint is addressed by `(conversation_run_id, statement_index)`, mirroring LangGraph's `(thread_id, checkpoint_id)` (§2.3) with one difference: because Ulexite's checkpoint log doubles as the trace format (§18), replay, debugging (§19), and audit are the same mechanism, not three separate systems layered by three separate teams the way LangSmith/tracing, checkpointing, and `git blame`-style audit ended up as unrelated bolt-ons across the frameworks in §2.3.

## 10.5 Plan/preview

`ulx plan <conversation> <args>` (§17.6, §20) statically walks the compiled graph and reports, without executing a single call: every capability that will be invoked, the provider each resolves to under current policy, an estimated token/cost range per call (from provider-reported pricing metadata), and any capability-negotiation failures (§9.6) that would abort the run. This is Terraform's `plan`/`apply` split (§2.4) applied to token spend instead of infrastructure changes — a direct answer to the complete absence, across every framework surveyed, of a way to know what a run will cost or touch before spending real tokens on it.

## 10.6 Concurrency and isolation

Steps scheduled concurrently by §10.2's declarative pass run in isolated evaluation contexts — no shared mutable state except through declared merge functions (§9.5). A failing step does not corrupt sibling steps' state; a step's failure is a typed `Draft<T>` variant its own `with` binding carries, handled either at the point of use or propagated per §10.7's supervision rule. This is Elixir's process-isolation half of "let it crash" (§2.5) — the isolation, not yet the restart policy, which §10.7 covers.

## 10.7 Supervision and failure escalation

A `conversation` or a group of steps may declare a supervision policy:

```
supervise steps: [outline, keyfacts] {
  strategy: retry_independent(max: 3)
  on_exhaust: escalate(human_approval)
}
```

modeled directly on Elixir's supervision trees (§2.5, §3.4): a step's failure is isolated to itself (§10.6), retried per the declared strategy, and escalated per a declared policy when retries are exhausted — replacing the ad hoc `try/except`-per-call-site pattern that recurs, uncomposed, across every framework in §2.3. Supervision composes with `retry`/`escalate` (§7.3): the statement-level form is sugar for a single-step supervisor; `supervise` is for a named group.

## 10.8 Streaming

`stream` is a mode of `run`, not a separate code path: any capability invocation may be observed incrementally (token-level for `chat`, frame-level for `generate_image`/`video`), exposed as a typed event stream (`TokenDelta`, `ToolCallStarted`, `ToolCallFinished`, `JudgeStarted`, `VerdictReached`, `CheckpointWritten`) — deliberately closer to LangGraph's multi-mode `stream_mode=["values","updates","messages","custom"]` (§2.3) than to LangChain's separately-implemented sync/async method pairs (`invoke`/`ainvoke`, `stream`/`astream`) that the §2.3 survey flags as prone to drift when a custom component only implements one path. Because Ulexite has no host-language sync/async split (§13, the runtime owns its own concurrency model), there is exactly one `stream` semantics per capability, not two independently-maintained ones.
