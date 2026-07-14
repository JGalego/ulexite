# 1. Vision

## 1.1 The claim

Every mature computing domain eventually gets a language, not just a library. Relational data got SQL. Infrastructure got Terraform/HCL. Browser automation got Playwright's test runner. Build graphs got Bazel/Starlark. Concurrent fault-tolerant systems got Erlang/Elixir. In each case the shift from "a library in a general-purpose language" to "a language with its own grammar, type system, and runtime" happened because the domain had a *recurring shape* that libraries could approximate but not enforce, and because the domain needed guarantees — determinism, reproducibility, static checking, a canonical execution model — that a library bolted onto a host language's control flow could never fully deliver.

LLM-driven conversational AI has reached that point. It has a recurring shape: a sequence of turns between typed participants (humans, models, tools, judges), producing typed multimodal artifacts, threaded through automatic history, subject to retries and non-determinism, and — increasingly — required to be testable, reproducible, and auditable in production. Today that shape is approximated by a dozen incompatible Python/TypeScript libraries, each reinventing conversation state, retries, tracing, and evaluation as ad hoc application code, because none of them is a language: none has a grammar that can statically reject an unhandled judge verdict, none has a compiler that can prove a multimodal artifact is being routed to a model that accepts it, none has a runtime whose replay guarantee is a language-level contract rather than a best-effort SDK feature.

## 1.2 What Ulexite is

Ulexite is a language, with a lexer, parser, static semantic analysis, an intermediate representation, and a runtime — independent of any single LLM provider — whose central value proposition is:

> **The conversation is the unit of compilation and execution.** Everything else — models, tools, judges, artifacts, retries, traces — is a typed participant, message, or effect inside that conversation, checked and scheduled by the compiler and runtime, not hand-assembled by application code.

Ulexite is not a prompt templating engine, not an agent framework, not a YAML config format, and not an embedded Python DSL. It is closer in spirit to Terraform (declare a graph, preview it, apply it, replay it), Playwright (auto-waiting, tracing, and assertions as language primitives, not test-runner conventions), and Gleam/Elixir (sound typing and supervision as defaults, not opt-in patterns) — applied to the domain of conversations with and between intelligent, non-deterministic participants.

## 1.3 Non-goals

- Ulexite is not trying to be a general-purpose language. It has no ambition to write web servers or device drivers. It calls out to a host ecosystem (via FFI to Python/JS/shell) for everything outside its domain, the way SQL calls out to application code for anything outside relational queries.
- Ulexite does not try to make LLMs deterministic. It makes the *scaffolding* around them — retries, validation, routing, tracing, replay of the deterministic parts of a run — deterministic and typed, while treating the model call itself as an explicitly effectful, explicitly non-deterministic primitive (see [§10 Execution Semantics](10-execution-semantics.md)).
- Ulexite does not try to out-optimize DSPy at automatic prompt optimization, or out-orchestrate LangGraph at arbitrary graph topologies. Both are legitimate techniques that Ulexite's standard library can express (see [§15](15-standard-library.md)); they are not the language's reason for existing.

## 1.4 Why now

Three things have changed since the current generation of LLM libraries were designed (2022–2024):

1. **Structured/constrained output is now a solved backend problem.** Grammar-constrained decoding (pioneered by Guidance and LMQL) proved token-level constraint enforcement works, but tied it to a Python embedding and a specific backend tier. The technique is now provider-supported (JSON mode, structured outputs, tool-calling schemas) widely enough that a language can assume typed structured output as a baseline capability rather than a research feature.
2. **Multi-turn, multi-agent, multi-provider is now the default case, not the exception.** LangGraph's checkpoint/thread model and OpenAI's Agents SDK sessions both independently converged on "conversation as a persisted, replayable object" — evidence that the industry already wants what §1.2 describes; nobody has made it a language-level guarantee instead of an SDK feature.
3. **Evaluation and testing have become the operational bottleneck, not the model call itself.** Promptfoo and OpenAI Evals both prove that teams want matrix testing, LLM-as-judge grading, and golden datasets — but both remain config-file test *runners* bolted onto the side of a separate orchestration codebase, rather than a language where `expect`, `judge`, and `dataset` are as native as `if` and `for`.

Ulexite's bet is that these three trends are ready to be unified as language primitives, the way relational access patterns were ready to be unified as SQL once enough systems had independently reinvented cursors and join loops.
