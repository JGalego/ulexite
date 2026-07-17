---
title: Core Concepts
description: Conversations, messages, artifacts, judges, and validators — independent of concrete syntax.
---

# Core Concepts

This page defines Ulexite's core concepts independently of concrete syntax — see [Language Syntax](./language/syntax.md) for how they're actually written. Every concept below is a first-class citizen of the type system, not a library convention layered on top of a general-purpose language.

## Conversation

A `conversation` is the unit of compilation and execution — Ulexite's analogue of a "program" or a "query." It's a named, typed value:

- It has a **participant set**, each with a declared role and capability.
- It has an **artifact graph** of everything produced or consumed during the conversation.
- It has an automatic, structural **history** — every message ever exchanged, in order, content-addressed — that you never manually thread through calls the way some frameworks require an explicit history reducer.
- It can be **nested**: a conversation can spawn a child conversation (a sub-agent negotiating with a tool, a judge conversing with itself to refine a rubric) whose history is scoped to the child but linked into the parent's trace.
- It can be **imported and parametrized** like a function or a Terraform module, and invoked repeatedly with different inputs — reuse is "call this conversation," not "subclass this planner."

## Messages and participants

Every message has a statically known **role**, drawn from a closed set the compiler can exhaustively match over:

- `user` — human input.
- `assistant` — model output.
- `system` — standing instructions/context.
- `tool` / `function` — a tool invocation and its result, as distinct message kinds (call vs. result).
- `judge` — an evaluative verdict over another message or artifact.
- `human_approval` — a checkpoint requiring a human decision before the conversation proceeds; a first-class, awaitable message kind, not an out-of-band webhook.

Because roles are closed and exhaustively matchable, adding a new participant type to a conversation and forgetting to handle it in a downstream `match` is a compile error — the same guarantee Rust's enums or Gleam's actor mailboxes give you for an unhandled variant.

## Artifacts

An `artifact` is a typed value flowing through a conversation: `text`, `markdown`, `image`, `audio`, `video`, `pdf`, `json`, `xml`, `html`, `csv`, `embedding`, `vector`, or `tool_output`. Artifacts are:

- **Typed and inspectable** — an `image` artifact carries width/height/mime metadata statically known to the compiler, so routing a `video` into a model whose declared capability only accepts `[text, image]` is a compile-time error, not a runtime 400 from the provider.
- **Content-addressed and immutable** — like a git blob, identical content is deduplicated and hashable, which is what makes caching and exact replay possible.
- **Composable in a graph** — an artifact can declare what it was derived from (a `summary` derived from a `source_pdf` via `ask`), giving the runtime a dependency graph it can use to skip recomputation when an upstream artifact hasn't changed.

See [Type System & Artifacts](./language/type-system.md) for the full type lattice.

## Variables

A step's output is bound to a named variable with `->`, making data flow explicit and referenceable downstream, instead of implicit return-value threading:

```ulexite
assistant -> summary
assistant -> translation
```

`summary` and `translation` are ordinary typed artifact-valued bindings, usable anywhere a value of that artifact type is expected — as input to another step, as the subject of a `judge`, or as the golden output in a `dataset` entry.

## Multiple models

A conversation step names *what capability it needs* (`chat`, `vision`, `embed`, ...), never a vendor. Which concrete provider satisfies that capability is resolved by the runtime's provider registry according to policy (cost, latency, availability, or an explicit pin) — different steps in the same conversation can resolve to different providers transparently, and swapping providers never touches step syntax. See [Providers](./providers.md).

## Judges

LLM-as-judge is a language construct, `judge`, not a hand-written grading prompt glued to a string-matching function. A `judge` takes one or more artifacts/messages, a rubric, and returns a typed `Verdict` that downstream code pattern-matches over — `Pass`, `Fail(reason)`, `Score(value)`, `Escalate` — with the same exhaustiveness guarantee as a participant role. Judges compose: a judge's own output can itself be judged (meta-evaluation), and a judge can be substituted with a deterministic validator wherever a `Verdict`-typed value is expected.

## Deterministic validators

Where a check *can* be deterministic, it should be expressible without invoking a model: `regex`, `json_schema`, `ast`, or an escape hatch to `python`, `javascript`, or `shell` for arbitrary custom logic. Validators return the same `Verdict` type as judges, so a program can freely mix "check this JSON against a schema" and "have a judge assess tone" in the same `match` without a type distinction leaking through — determinism is a property of *how* the verdict was produced, not a different type of verdict.

## Testing

`expect`, `assert`, `snapshot`, `benchmark`, `dataset`, and golden-output comparison are keywords, evaluated by the compiler/runtime the way a query is evaluated by a database engine — not parsed by an external test-runner reading a config file. A `dataset` is a first-class, versioned, injectable value; parametrizing a test over a dataset is the native looping construct, built into the grammar rather than bolted on as a decorator. See [Testing & Evaluation](./testing-and-evaluation.md).

## Runtime guarantees

The runtime provides these automatically, as language semantics rather than opt-in SDK features:

- **Retries** — declared as a policy attached to a step or conversation, not a `try`/`except` loop.
- **Traces** — every run produces a complete, replayable trace by default.
- **Caching** — model/tool calls are content-addressed and cached by default, so identical calls across runs or re-executions are free.
- **Provider routing** — resolved at runtime, against live policy, not fixed at compile time.
- **Checkpointing and replay** — a conversation's deterministic control flow can be checkpointed and replayed from any point, with non-deterministic model/tool calls memoized rather than re-invoked during replay.

## What's next

[Language Syntax](./language/syntax.md) shows exactly how to write each of these constructs, or jump to the [Examples gallery](./examples/index.md) to see them combined in complete programs.

---

This page adapts [§5 Language Overview](https://github.com/JGalego/ulexite/tree/main/docs/spec/05-language-overview.md) of the full spec (RFC-0001).
