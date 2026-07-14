# 19. Debugging Model

## 19.1 Debugging a replay, not a live process

Because every run is checkpointed and traced by default (§10.4, §18), the primary debugging workflow is not attaching a debugger to a live process — it is `ulx debug <run_id>`, which loads a completed (or crashed, or suspended-on-`human_approval`) run's trace and lets a developer step through it exactly as it happened, forward and backward, with full artifact inspection at every step. This is possible precisely because §10.4/§13.4 made replay-from-trace a language-level guarantee rather than a best-effort feature — most systems in §2.3 can show you a trace after the fact (LangSmith, LangGraph Studio) but cannot *re-execute* it deterministically step by step the way `ulx debug` does.

## 19.2 Breakpoints as ordinary language constructs

A `breakpoint()` statement, or a conditional variant `breakpoint(verdict is Fail)`, suspends interpretation at that IR node when running under `ulx run --debug` or when replaying; the debugger then exposes the current scope's bindings (typed artifacts, in-flight `Draft<T>`/`Verdict` values) for inspection, matching an ordinary language debugger's mental model (breakpoints, locals, step-over/into/out) rather than requiring a bespoke "print statements and hope" workflow, which is the de facto debugging story for every framework in §2.3 absent a paid tracing product.

## 19.3 Time-travel and re-run-from-here

Since every statement is a checkpoint (§10.4), `ulx debug` supports jumping to any `statement_index` and either inspecting state there or **re-running from there with modified inputs** (`ulx fork`, §18.4) — directly exposing LangGraph's `update_state`+fork capability (§2.3) as the default debugging workflow for any Ulexite program, not a LangGraph-specific power-user feature.

## 19.4 Root-cause navigation across nested conversations

A nested conversation's trace records carry `parent_run_id` (§18.2); the debugger renders nested conversations as a navigable call stack (parent conversation → child conversation → the specific step) — a direct fix for the "five layers of abstraction just to find a root-cause traceback" complaint documented against LangChain/LangGraph in practitioner postmortems (§2.3): because the compiler, not a stack of framework wrapper classes, produced the nesting, the debugger's call stack corresponds exactly to the program's own nested-conversation structure (§5.1), with no framework-internal frames to page through.

## 19.5 Non-deterministic failure triage

When a `Draft<T>` resolves to `Refused`/`RateLimited`/`Timeout` (§9.3) or a `judge`/`validator` returns `Fail`/`Escalate` (§9.4), the debugger surfaces the exact typed reason alongside the full input artifacts that produced it — because these are ordinary typed values, not exceptions (§4.7), there is no unwound stack to reconstruct; the debugger shows the value the program's own `match` would have seen, directly, which is the payoff of §9.3–9.4's design choice made concrete as a debugging experience rather than only a type-safety argument.

## 19.6 Live attach for in-flight conversations

For a long-running or suspended-on-`human_approval` conversation, `ulx attach <run_id>` connects to the live execution engine (§12.2) rather than a completed trace — showing the same view as replay debugging, but against the actual in-flight state, useful for inspecting a production conversation waiting on a human approval before deciding how to respond (§7.3, §10.7).

## 19.7 Debugger hooks for tool authors

Tool and provider adapters (§12.4, §12.6) can register debug-inspector callbacks exposed identically through `ulx debug`'s UI — a third-party vector-store provider plugin, for instance, can expose "show me the retrieved candidates and their scores" as a debugger panel without Ulexite's core debugger needing to know anything about vector stores specifically, mirroring how a language server's hover/inspection protocol (§20) is extensible by tooling authors rather than hardcoded per library.
