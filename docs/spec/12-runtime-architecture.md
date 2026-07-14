# 12. Runtime Architecture

## 12.1 Module boundaries

Per the mission's implementation requirements, the runtime is deliberately partitioned into independently testable, independently replaceable modules — no module reaches into another's internals:

```
┌─────────────┐   IR    ┌───────────────┐  effects  ┌──────────────────┐
│  Compiler   │────────▶│ Execution      │──────────▶│ Provider adapters │
│ (§13)       │         │ Engine         │           │ (§12.4)           │
└─────────────┘         │ (scheduler,    │           └──────────────────┘
                         │  §10)          │
                         │                │──────────▶┌──────────────────┐
                         │                │  effects  │ Tool adapters    │
                         │                │           │ (§12.6)          │
                         │                └───┬───┬────└──────────────────┘
                         │                    │   │
                         │            ┌───────┘   └────────┐
                         │            ▼                    ▼
                         │     ┌─────────────┐      ┌──────────────┐
                         │     │ Trace engine │      │ Artifact     │
                         │     │ (§12.5, §18) │      │ storage      │
                         │     └─────────────┘      │ (§11.2)      │
                         │                          └──────────────┘
                         │            ┌─────────────┐
                         └───────────▶│ Cache        │
                                      │ (§10.3)     │
                                      └─────────────┘
                                      ┌─────────────┐
                                      │ Evaluation   │
                                      │ engine (§17) │
                                      └─────────────┘
```

Each box is a separate compiled unit with its own test suite (§25 testing plan); the Execution Engine depends on interfaces (provider capability trait, tool trait, trace sink trait, cache trait, artifact store trait), never on a concrete provider/tool implementation — this is the structural guarantee behind §4.3 and the mission's "adding a provider should not require changing compiler code."

## 12.2 Execution engine

The execution engine consumes compiled IR (§13.4) and drives §10's semantics: it walks the two-pass schedule (§10.2), dispatches capability calls to provider adapters, dispatches tool calls to tool adapters, writes a checkpoint after every statement (§10.4) via the trace engine, and consults the cache (§10.3) before every effectful call. It is itself deterministic given a fixed trace log — replay (§10.4, §19.3) re-runs the exact same engine code path against recorded results instead of live calls, which is why the engine has no ambient non-determinism (no direct wall-clock reads, no unseeded randomness) outside the explicit `Draft<T>`-typed effect boundary (§9.3).

## 12.3 Scheduler and concurrency

A bounded worker pool (default: CPU core count, configurable per §14's manifest or `ulx run --concurrency`) executes the declarative region's independent bindings (§10.2). Backpressure and per-provider rate limits are enforced per provider adapter (§12.4), not globally — a slow/rate-limited provider throttles only the steps resolved to it, mirroring Beam's runner-level concern separation (§2.4) rather than a single global rate limiter every framework in §2.3 tends to bolt on ad hoc.

## 12.4 Provider adapters and capability resolution

A provider adapter is a plugin implementing the capability trait from §9.6: it declares which capabilities (`chat`, `vision`, `embed`, ...) it satisfies, at which guarantee tier (`guaranteed`/`negotiated`/`unsupported` structured output, §9.6), and its cost/latency metadata. The runtime's provider registry resolves an unqualified `ask <capability>(...)` call against live policy — cost ceiling, latency budget, an explicit pin, or availability/circuit-breaker state — recomputed per call, not fixed at compile time (only the *capability requirement* is fixed at compile time, per §9.6). Registering a new provider is implementing the trait and adding it to the registry; no compiler, grammar, or IR change is required — directly satisfying the mission's provider-independence requirement and avoiding the fate of every framework in §2.3, where "provider-agnostic" meant a hand-maintained adapter (LiteLLM, `init_chat_model`, SK connectors) with silently uneven feature parity.

## 12.5 Trace engine

The trace engine is the single writer of the checkpoint/trace log (§10.4, §18) — every statement's inputs, resolved provider, cache hit/miss, effect result, and timing are appended as one immutable, content-addressed record. Because it is one engine serving both debugging (§19) and compliance/audit needs, Ulexite does not repeat the fragmentation documented in §2.3 (LangSmith for tracing, a separate checkpointer for durability, `git blame`-equivalent audit nonexistent) — one log serves all three.

## 12.6 Tool adapters and middleware

A tool adapter registers a callable capability (a `function`/`tool` message target, §5.2) with a declared input/output artifact schema, checked the same way capability `accepts`/`produces` are checked (§9.2, §11.5). Cross-cutting middleware — PII redaction, semantic caching, injection defense — attaches as a `next(context)`-style filter around any capability or tool call, directly adapting Semantic Kernel's Filters (§2.3, §2.7) as a runtime-level extension point rather than an SK-specific class hierarchy.

## 12.7 Artifact storage

A pluggable content-addressed store (local filesystem by default; S3-/GCS-compatible or a remote cache service for team use) backs §11.2. The store's interface is a trait (`put(bytes) -> hash`, `get(hash) -> bytes`, `has(hash) -> bool`) — swapping local-disk for a shared remote cache (Bazel remote-cache style, §2.4) is a configuration change in the manifest (§14), never a code change.

## 12.8 Evaluation engine

A separate engine (§17) drives `benchmark`/`dataset`/`expect`/`snapshot` execution: it resolves a `dataset`'s versioned rows (§11.6), runs the referenced conversation once per row (parallelized per §12.3), collects `Verdict`s, and aggregates/report per §16–17. It reuses the execution engine (§12.2) for each row's conversation run rather than a separate interpreter, so a benchmark run produces the exact same trace/checkpoint artifacts (§12.5) an ordinary run would — a benchmark is not a second execution model bolted onto the side (contrast Promptfoo/OpenAI Evals' entirely separate runner processes, §2.2).

## 12.9 Failure isolation

Per §10.6–10.7, a step's failure is contained to its own evaluation context; the execution engine propagates it as a typed value, never an unwound host-language exception escaping the engine's own process — the engine itself has one top-level failure mode (a compiler/runtime bug), distinct in kind from a conversation's own typed `Draft<T>`/`Verdict` failures, so a crash in "my program's logic" and a crash in "the runtime" are never confusable the way an unhandled Python exception from deep inside a framework's internals is in every system surveyed in §2.3.
