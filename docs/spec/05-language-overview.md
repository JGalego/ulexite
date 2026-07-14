# 5. Language Overview

This section defines Ulexite's core concepts at a level independent of concrete syntax (see §6–7 for syntax, §8 for grammar). Every concept below is a first-class citizen of the type system (§9), not a library convention.

## 5.1 Conversation

A `conversation` is the unit of compilation and execution — the Ulexite analogue of a "program" or a "query." It is a named, typed value:

- It has a **participant set** (§5.2), each with a declared role and capability.
- It has an **artifact graph** (§5.3, §11) of everything produced or consumed during the conversation.
- It has an automatic, structural **history** — every message ever exchanged, in order, content-addressed (§18) — that the programmer never manually threads through calls the way LangGraph's `MessagesState`/`add_messages` reducer or DSPy's `dspy.History` require.
- It can be **nested**: a conversation can spawn a child conversation (a sub-agent negotiating with a tool, a judge conversing with itself to refine a rubric) whose history is scoped to the child but linked into the parent's trace.
- It can be **imported and parametrized** like a function or a Terraform module, and invoked repeatedly with different inputs — reuse is "call this conversation," not "subclass this planner."

## 5.2 Messages and Participants

Every message has a statically known **role**, drawn from a closed set the compiler can exhaustively match over:

- `user` — human input.
- `assistant` — model output.
- `system` — standing instructions/context.
- `tool` / `function` — a tool invocation and its result, as distinct message kinds (call vs. result).
- `judge` — an evaluative verdict over another message or artifact (§5.6).
- `human_approval` — a checkpoint requiring a human decision before the conversation proceeds (a first-class, awaitable message kind, not an out-of-band webhook).

Because roles are closed and exhaustively matchable, adding a new participant type to a conversation and forgetting to handle it in a downstream `match` is a compile error (§9.4), the way Gleam's actor mailboxes and Rust's enums make an unhandled variant a build failure rather than a runtime surprise.

## 5.3 Artifacts

An `artifact` is a typed value flowing through a conversation: `text`, `markdown`, `image`, `audio`, `video`, `pdf`, `json`, `xml`, `html`, `csv`, `embedding`, `vector`, or a `tool_output`. Artifacts are:

- **Typed and inspectable** — an `image` artifact carries width/height/mime metadata statically known to the compiler, so routing a `video` into a model whose capability declares `accepts: [text, image]` is a compile-time error (§4.2), not a runtime 400 from the provider.
- **Content-addressed and immutable** (§18) — like a git blob, identical content is deduplicated and hashable, which is what makes caching (§4.5, Bazel-style) and exact replay (§10) possible.
- **Composable in a graph** — an artifact can declare what it was derived from (`summary` derived from `source_pdf` via `ask`), giving the runtime a dependency graph it can use to skip recomputation when an upstream artifact hasn't changed (React-style memoization, applied to conversation steps instead of UI components).

## 5.4 Variables

A step's output is bound to a named variable with `->`, making data flow explicit and referenceable downstream, rather than implicit return-value threading:

```
assistant -> summary
assistant -> translation
```

`summary` and `translation` are ordinary typed artifact-valued bindings usable anywhere a value of that artifact type is expected — as input to another step, as the subject of a `judge`, or as the golden output in a `dataset` entry.

## 5.5 Multiple Models

A conversation step names *what capability it needs* (e.g. `chat`, `vision`, `embed`), not a vendor. Which concrete provider satisfies that capability is resolved by the runtime's provider registry (§12.4) according to policy (cost, latency, availability, explicit pin) — different steps in the same conversation may resolve to different providers transparently, and swapping providers never touches step syntax (§4.3).

## 5.6 Judges

LLM-as-judge is a language construct, `judge`, not a hand-written grading prompt glued to a string-matching function (contrast Promptfoo's `llm-rubric` assertion type and OpenAI Evals' `ModelBasedClassify` — both real prior art, both external to the language doing the orchestrating). A `judge` takes one or more artifacts/messages, a rubric, and returns a typed `Verdict` (§9.4) that downstream code pattern-matches over — `Pass`, `Fail(reason)`, `Score(value)`, `Escalate` — with the same exhaustiveness guarantee as §5.2's participant roles. Judges compose: a judge's own output can itself be judged (meta-evaluation, as in OpenAI Evals' meta-eval pattern), and a judge can be substituted with a deterministic validator (§5.7) wherever a `Verdict`-typed value is expected.

## 5.7 Deterministic Validators

Where a check *can* be deterministic, it must be expressible without invoking a model: `regex`, `json_schema`, `ast`, or an escape hatch to `python`, `javascript`, or `shell` (§15.11–15.13) for arbitrary custom logic. Validators return the same `Verdict` type as judges (§5.6), so a program can freely mix "check this JSON against a schema" and "have a judge assess tone" in the same `match` without a type distinction leaking through — determinism is a property of *how* the verdict was produced, not a different type of verdict.

## 5.8 Testing

`expect`, `assert`, `snapshot`, `benchmark`, `dataset`, and golden-output comparison are keywords (§16), evaluated by the compiler/runtime the way `SELECT` is evaluated by a SQL engine — not parsed by an external test-runner reading a config file (contrast Promptfoo's YAML and OpenAI Evals' registry YAML, both of which delegate anything non-trivial to embedded Python/JS). A `dataset` is a first-class, versioned, injectable value — parametrizing a test over a dataset is the native looping construct, the way pytest's `@parametrize` turns one function into many reported cases, except built into the grammar rather than a decorator.

## 5.9 Runtime Guarantees

The runtime automatically provides, as language semantics rather than opt-in SDK features:

- **Retries** — declared as a policy attached to a step or conversation (Elixir supervision-tree style), not a `try/except` loop.
- **Traces** — every run produces a complete, replayable trace by default (§18), the way `git commit` always produces a content-addressed object.
- **Caching** — model/tool calls are content-addressed and cached by default (Bazel-style), so identical calls across runs or re-executions are free.
- **Provider routing** — resolved per §5.5, at runtime, against live policy (cost/latency/availability), not fixed at compile time.
- **Checkpointing and replay** — a conversation's deterministic control flow can be checkpointed and replayed from any point (Temporal-style durable execution, §10.4), with non-deterministic model/tool calls memoized rather than re-invoked during replay.
