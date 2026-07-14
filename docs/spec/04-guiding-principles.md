# 4. Guiding Principles

These principles are load-bearing: every syntax and runtime decision in this spec is checked against them, and any future RFC that violates one must explicitly say why.

## 4.1 Conversation-first, not call-first

The atomic unit of an Ulexite program is a `conversation`, not a single completion call. A conversation is a typed, append-only sequence of `message`s exchanged among typed `participant`s (human, model, tool, judge, system). History is automatic and structural — you do not manually thread a `messages: list[dict]` array through your program (contrast LangGraph's `MessagesState`/`add_messages` reducer, DSPy's manually-appended `dspy.History`, and Promptfoo's per-test `vars`, all of which bolt conversation state onto a data structure the *user* maintains). A single completion is just a conversation of length one; it is not the other way around.

## 4.2 Multimodal-first

Text, images, audio, video, PDF, JSON, and embeddings are all `artifact` types in one closed type lattice, not "text, plus an escape hatch for everything else" (contrast Guidance's thin, backend-conditional `image()` support and DSPy's one-image-per-field limitation). A model, tool, or judge declares the artifact types it accepts and produces; the compiler rejects a program that routes a `video` artifact into a text-only model at compile time, not at request time.

## 4.3 Provider-independent by construction

No syntax in the language names a vendor. Providers are runtime plugins satisfying a capability interface (`chat`, `embed`, `judge`, `transcribe`, ...), resolved by capability and policy, not by import (contrast DSPy's LiteLLM `"provider/model"` strings baked into source, and Guidance's distinct model class per backend). Swapping GPT for Claude for a local model is a configuration change, never a rewrite.

## 4.4 Deterministic where possible, explicit where not

Model calls are the one place genuine non-determinism enters the system, and the language says so explicitly in the type system (§9): a model call's return type is `Draft<T> | Refused | RateLimited | Timeout`, not `T`, so a program cannot accidentally treat a non-deterministic result as settled without a `match` or a judge resolving it. Everything *around* the call — control flow, retries, artifact routing, history construction — is ordinary deterministic language semantics, replayable exactly from a trace (borrowing Temporal's determinism/replay split and git's content-addressed, immutable history, applied to conversations instead of workflows or commits).

## 4.5 Declarative where appropriate, imperative where beneficial

A conversation's shape (participants, artifact dependencies, judges, validators) is declared; the compiler is free to schedule, cache, and parallelize independent branches the way a SQL planner reorders joins (§4.1 of Recommended Syntax; borrowing SQL's CTE-as-named-relation model and Nextflow's implicit dataflow parallelism). But turn-by-turn logic — branching on a judge's verdict, looping until a validator passes, escalating to a human — is ordinary imperative control flow, because forcing genuinely sequential, stateful decision logic into a static DAG is precisely the failure mode that made Airflow's "DAG must be known at parse time" and GitHub Actions' YAML control flow so awkward.

## 4.6 Testable and reproducible as defaults, not add-ons

`expect`, `assert`, `judge`, `dataset`, and `benchmark` are keywords, not an external test-runner's config schema (contrast Promptfoo's YAML matrix and OpenAI Evals' registry-of-YAML, both of which fall back to embedded Python/JS the moment logic gets non-trivial). Every conversation run produces a trace sufficient to replay it exactly, by default, the way `git commit` produces a content-addressed object by default — tracing is not something you turn on in production and hope you remembered to enable before the incident.

## 4.7 Failure is a typed, first-class outcome, not an exception

A refused generation, a failed validator, a judge score below threshold, and a timed-out tool call are values a program pattern-matches over exhaustively (borrowing Rust's `Result<T,E>` + exhaustive `match`, Zig's explicit error unions, and Gleam's total functions over closed message types), not exceptions that unwind a call stack the way a stray Python exception does in every framework surveyed in §2. Retry policy, escalation to a human, and supervision boundaries are declarations attached to a conversation or step (borrowing Elixir's supervision trees), not `try/except` sprinkled through prompt-assembly code.

## 4.8 Composable without framework lock-in

A conversation, a step, or a judge is a value that can be imported, parametrized, and reused across programs the way a SQL view or a Terraform module is — not a class you must subclass (`dspy.Module`, `dspy.Signature`) or a decorator you must apply in a specific order to opt into the framework's control flow.

## 4.9 Legible to tools, not just to runtimes

Because Ulexite has a real grammar and a real type system, a language server can autocomplete artifact fields, flag an unreachable branch, and warn about an unused judge verdict statically (§20) — something no YAML-based or embedded-Python system in §2 can do, because in all of them the "program" is either not parsed as a program (YAML/config) or is parsed as a host language that has no idea what a `Draft<T>` or a `judge` is.
