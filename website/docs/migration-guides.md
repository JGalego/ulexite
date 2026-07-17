---
title: Migration Guides
description: How LangChain/LangGraph, Semantic Kernel, DSPy, and Promptfoo/OpenAI Evals concepts map onto Ulexite.
---

# Migration Guides

No existing codebase needs to rewrite wholesale to adopt Ulexite. A `conversation` value satisfies a single, simple execution protocol and Ulexite has an FFI boundary designed in from the start, so the recommended path is always to **wrap one conversation at a time**, called from your existing host application — not a big-bang port.

The sections below map concepts from popular frameworks onto their Ulexite equivalent.

## From LangChain / LangGraph

- **LCEL chains** map most directly onto Ulexite's imperative statement sequence: a `prompt | model | parser` pipeline becomes a message-literal block ending in a typed binding. LangChain's `Runnable` uniform-interface idea is the same idea Ulexite generalizes, so this is largely a mechanical translation of pipe stages into sequential statements.
- **`RunnableWithMessageHistory` + session-id config** collapses entirely: Ulexite's automatic structural message history replaces it outright. Migrating this should *delete* the wrapper and session-store code, not port it.
- **LangGraph's `StateGraph`/reducers** map onto Ulexite's merge-function declarations plus `with` blocks for the parallelizable subset of the graph, and onto ordinary imperative control flow for genuinely sequential/conditional nodes. A graph with heavy conditional routing (`add_conditional_edges`) migrates most naturally to `match` statements, not a forced `with` block.
- **Checkpointers** are unnecessary to port — every Ulexite run is checkpointed by default. A team relying on LangGraph's `(thread_id, checkpoint_id)` model gets the equivalent for free.
- **LangSmith evals** migrate to `benchmark`/`judge` declarations; existing rubric text is largely reusable verbatim inside a `judge`'s rubric field.

## From Semantic Kernel

- **Plugins/`KernelFunction`s** map onto tool adapters; the `[KernelFunction]`-attribute ceremony collapses to a plain tool declaration with a checked input/output artifact schema.
- **`ChatHistory`** collapses the same way LangChain's history wrapper does — replaced, not ported, by automatic structural history.
- **Filters** are the one Semantic Kernel concept with a near-1:1 target: they map directly onto Ulexite's `next(context)`-style middleware extension point, and porting filter logic is close to mechanical.
- Given Semantic Kernel's own maintenance-mode status and its successor's abandonment of the `Kernel` object entirely, teams already mid-migration to Microsoft Agent Framework may find it lower-risk to evaluate Ulexite against Agent Framework directly rather than against legacy Semantic Kernel.

## From DSPy

- **Signatures** map onto a `conversation`'s typed parameter/return declaration — the cleanest of any migration on this page, since DSPy's `Signature` concept and Ulexite's typed conversation boundary solve the same problem the same way.
- **Modules** (`Predict`, `ChainOfThought`, `ReAct`) map onto conversation bodies with the corresponding control-flow shape written out explicitly. A `ChainOfThought`'s implicit `reasoning` field becomes an explicit intermediate binding; a `ReAct` loop becomes a `while`/`match` loop over tool-call results.
- **Optimizers** aren't reimplemented — Ulexite's standard library wraps the same technique (`optimize.mipro`/`optimize.bootstrap_demos`), and an existing DSPy-compiled prompt artifact can, if desired, seed the corresponding Ulexite conversation's initial few-shot examples rather than being discarded.
- **`dspy.Evaluate`** maps onto `benchmark`/`dataset`.

## From Promptfoo / OpenAI Evals

- A `promptfooconfig.yaml`'s `tests` array or an OpenAI Evals JSONL dataset maps directly onto a `dataset` declaration — close to a mechanical data-format conversion, not a redesign.
- `llm-rubric`/`ModelBasedClassify` assertions map onto `judge` declarations; deterministic assertions (`regex`, `json-schema`, `is-json`) map onto `validator` declarations.
- The one thing that does **not** migrate mechanically is control flow buried in Nunjucks templates or an escape-hatch Python/JS grader function — that logic has to be rewritten as ordinary Ulexite statements. That's more or less the point: it's exactly the class of logic that had nowhere legitimate to live in a config-file format.

## From LlamaIndex

- **`Workflow`'s typed-event routing** is the closest conceptual match to Ulexite's own step-to-step data flow — a `@step` function's `Event -> Event` signature translates fairly directly to a sequence of typed bindings and `match` branches.
- **`ChatEngine`/`ChatMemoryBuffer`** collapse the same way every other framework's bespoke history object does, into automatic structural history.
- **Retrieval/ingestion pipelines** (node parsers, retrievers, rerankers) are the one area where a migration should *keep* LlamaIndex, not replace it: register it as a `vector_index` capability provider behind Ulexite's `vector`/`embedding` standard-library calls, rather than reimplementing years of retrieval-specific engineering.

## From OpenAI Agents SDK

- **Agents/Handoffs** map onto nested conversations — a handoff's model-visible `transfer_to_<agent>` tool call becomes an ordinary nested `conversation` invocation, with routing expressed as `match`/`if` rather than a special handoff primitive.
- **Guardrails** map onto `validator`/`judge` declarations gating a `match`.
- **Sessions** collapse into automatic history, as in every other migration path above.
- **Tracing** — because Ulexite's trace format can export to OpenTelemetry, an existing investment in a tracing backend (Langfuse, Arize, etc.) connected to the Agents SDK's OTel-compatible export can often be pointed at Ulexite's export with minimal reconfiguration.

## What never needs to migrate

Retrieval engines, vector databases, and provider SDKs — LlamaIndex's ingestion stack, LiteLLM's provider matrix, an existing Pinecone/Qdrant deployment — aren't replaced. They become provider or tool plugins behind Ulexite's capability interfaces. Reinventing everything outside the conversation-orchestration domain is an explicit non-goal.

For the full design rationale, see [§23 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/23-migration-paths.md).
