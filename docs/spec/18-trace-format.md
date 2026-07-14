# 18. Trace Format

## 18.1 One log, three consumers

Per §10.4/§12.5, a single append-only, content-addressed log serves replay, debugging (§19), and audit — deliberately not three separate systems the way tracing (LangSmith/LlamaTrace), checkpointing (LangGraph's checkpointer), and audit (nothing, in every framework surveyed, §2.3) ended up as three unrelated bolt-ons. The trace format is therefore specified once and consumed by all three.

## 18.2 Record shape

Every statement execution appends one immutable record:

```
TraceRecord {
  run_id: hash                    // identifies this conversation execution
  statement_index: int            // position in program order (§10.2)
  kind: Effect | Pure              // §13.4's IR distinction, preserved into the trace
  capability: text?                // e.g. "chat", "vision" — present for Effect records
  resolved_provider: text?         // capability resolution result (§12.4), absent for Pure
  input_hashes: [hash]              // content hashes of input artifacts (§11.1)
  output_hash: hash?                // content hash of produced artifact, if any
  verdict: Verdict?                 // present when the statement is a judge/validator call
  cache_hit: bool
  started_at, ended_at: timestamp   // wall-clock, recorded but never replayed as an input (§10.4)
  parent_run_id: hash?              // for nested conversations (§5.1)
}
```

Large artifact bytes are never inlined — a record holds a `hash` pointer into artifact storage (§11.2, §12.7), keeping the trace log itself small and fast to append/scan even for video/PDF-heavy conversations, the same separation of "pointer log" from "content-addressed blob store" as git's commit-object/blob-object split (§2.6).

## 18.3 Replay semantics

`ulx replay <run_id>` re-interprets the compiled IR (§13.6) against the trace log: every `Pure` node is recomputed; every `Effect` node's recorded `output_hash`/`verdict` is substituted for the node's result rather than re-invoking the capability/tool — the Temporal-style determinism/replay split (§2.4, §10.4) made concrete. Replay is exact as long as the IR being replayed against is the one that produced the trace (§13.7's stable node-identity scheme makes this checkable: a trace record's `statement_index` is validated against the current IR's structure before replay begins, and a mismatch is a clear "this trace was produced by a different program version" error rather than a silent misreplay).

## 18.4 Forking

Because a trace is a chain of content-addressed records referencing content-addressed artifacts, `ulx fork <run_id> --at <statement_index> --edit <field>=<value>` produces a new run that replays records `0..statement_index` unchanged and then diverges — git's branch-as-pointer model (§2.6) applied to conversation execution instead of source history, directly answering LangGraph's time-travel `update_state`+fork pattern (§2.3) with the same guarantee generalized to any conversation, not just ones authored against a `StateGraph`.

## 18.5 Trace diffing

`trace.diff(run_a, run_b)` (§15.8) aligns two traces by `statement_index` and reports, per statement: whether the resolved provider differed, whether the cache was hit, and — for `Effect` records with an artifact output — a semantic diff of the two output artifacts (§16.5's mechanism, reused here), not a byte diff. This is the primitive behind §17.4's regression-trend tooling and behind ordinary "why did this run behave differently from that one" debugging (§19.4).

## 18.6 Export and interoperability

A trace can be exported to OpenTelemetry span format (`ulx trace export --otel`) for teams with existing observability pipelines — Ulexite's own trace format is the source of truth (richer: it carries `Verdict`s and content hashes OTel spans don't natively model), with OTel export as an interoperability adapter, not the native format, avoiding the fate of frameworks whose only tracing story *is* a third-party SaaS integration (§2.3).

## 18.7 Storage and retention

Trace records are stored in the same pluggable store as artifacts (§12.7), with retention policy configurable per package (`ulexite.toml`, §14.1) — local developer runs default to a short local retention window; CI/production runs default to durable remote storage, since §17.3/§17.4's shadow-evaluation and regression-trend tooling both depend on production traces being retained.
