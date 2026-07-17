---
title: How Ulexite Compares
description: A factual, even-handed comparison against Guidance, LMQL, DSPy, LangGraph, Promptfoo, and OpenAI Evals.
---

# How Ulexite Compares

This page compares Ulexite against six systems people commonly reach for today when building LLM-driven applications: [Guidance](https://github.com/guidance-ai/guidance), [LMQL](https://lmql.ai/), [DSPy](https://dspy.ai/), [LangGraph](https://www.langchain.com/langgraph), [Promptfoo](https://www.promptfoo.dev/), and [OpenAI Evals](https://github.com/openai/evals). These aren't competitors to dismiss — several of them are more mature, more production-proven, and better at their specific job than Ulexite is today. The goal here is to help you place Ulexite accurately relative to tools you may already use, not to claim uniform superiority.

Ratings are relative and qualitative: **Yes** means the capability is native/structural to the system's design; **Partial** means it's present via a bolt-on, wrapper, or separate companion product; **No** means it's absent.

| Capability | Ulexite | Guidance | LMQL | DSPy | LangGraph | Promptfoo | OpenAI Evals |
|---|---|---|---|---|---|---|---|
| Conversation-first (history automatic, structural) | **Yes** | No | No | Partial | **Yes** | No | No |
| Typed artifacts checked at compile time | **Yes** | No | No | No | No | No | No |
| Provider-independent by construction | **Yes** | No | Partial | Partial | No | **Yes** (matrix) | Partial |
| Built-in judges (LLM-as-judge) | **Yes** | No | No | Partial | No (separate product) | **Yes** | **Yes** |
| Reproducible traces/replay | **Yes** (native) | No | No | No | **Yes** (checkpointer) | Partial (cache) | No |
| Checkpointing / durable execution | **Yes** (unconditional) | No | No | No | **Yes** (best-in-class) | No | No |
| Testing (`expect`/`assert`/`snapshot`) as grammar | **Yes** | No | No | No | No | Partial (YAML) | Partial (YAML) |
| Production battle-testing / scale | Low (new) | Medium | Low | Medium | **Very high** | Medium | Medium (sunsetting) |

## Reading the table

- **Conversation-first.** Ulexite and LangGraph both treat multi-turn history as automatic and structural (Ulexite via the language's built-in message history, LangGraph via `MessagesState`). Guidance and LMQL operate closer to a single model/program object with no first-class conversation abstraction; DSPy has a `History` type but managing it is largely manual.
- **Typed artifacts at compile time.** This is the one row where Ulexite stands alone among these six: none of the others reject a type-mismatched multimodal call (e.g. handing a PDF to a capability that only accepts images) before a request is actually sent to a provider.
- **Provider independence.** Promptfoo's matrix-testing design makes it genuinely provider-independent for evaluation purposes. Ulexite aims for the same property at the language level — a capability like `chat` or `vision` resolves to whichever configured provider supports it, checked at compile time. LMQL and DSPy are "partial" here because provider abstraction leaks through backend-specific code paths or LiteLLM model strings.
- **Judges.** Promptfoo and OpenAI Evals both have mature, purpose-built LLM-as-judge grading — that's their core job, and they do it well. Ulexite's `judge` declarations bring the same idea into the language itself, sharing the program's own type system (a judge's `Verdict` is a value the compiler forces you to handle exhaustively). LangGraph has no built-in judge concept; it defers to a separate product (LangSmith).
- **Reproducible traces and checkpointing.** LangGraph's checkpointer is the most production-proven durable-execution story of anything in this table — it has years of real usage behind it, including edge cases Ulexite's design has not yet encountered at scale. Ulexite makes every run checkpointed and replayable unconditionally, by default, with no opt-in step, but that guarantee is new and comparatively untested against production failure modes.
- **Testing as grammar.** Promptfoo and OpenAI Evals both support test assertions, but as YAML/JSON-config schemas with no view into a program's actual types. Ulexite's testing primitives (`benchmark`, `dataset`, `expect`) are ordinary language constructs that type-check against the same declarations the rest of the program uses.
- **Production track record.** This is the row most worth taking seriously before adopting Ulexite. LangGraph in particular has "very high" real-world mileage; DSPy and Promptfoo have meaningful production usage; OpenAI Evals is a maintained but sunsetting product. Ulexite is a new project with no comparable track record yet — see [Known Limitations](./limitations.md) for what that means concretely.

## Where these systems are genuinely stronger today

- **LangGraph** has the most mature, most production-proven durable-execution/checkpointing story of any system surveyed. Ulexite's checkpoint/replay design borrows from the same ideas but starts with zero of LangGraph's accumulated production track record.
- **DSPy**'s optimizers (MIPROv2, GEPA) are genuine, empirically validated research that Ulexite doesn't reimplement — it wraps the same technique in its standard library. DSPy remains the better choice if your primary need is automatic prompt/few-shot optimization research rather than conversation orchestration.
- **Promptfoo and OpenAI Evals** are purpose-built, mature evaluation tools with battle-tested grading and matrix-testing features that a new language's built-in `judge`/`benchmark` constructs haven't yet accumulated the same operational mileage against.
- **Guidance and LMQL**, where a local backend with logit access is available, can offer a stronger structural-output guarantee than Ulexite's capability-negotiation model promises universally — because that guarantee is fundamentally dependent on what the backend actually supports, not something any orchestration layer above it can force.

## What's genuinely new here

A few things in Ulexite's design don't have a direct analog in any of the six systems above:

- A conversation as a compiler-checked value satisfying one execution protocol, rather than an object assembled ad hoc from framework primitives.
- Artifact types checked against a capability's accepted/produced types at compile time — catching a type-mismatched multimodal call before a request is ever sent.
- `Verdict`/`Draft<T>` as closed unions with compiler-enforced exhaustive matching, so a caller can't accidentally skip handling one non-deterministic outcome.
- `with`-block independence as a parser-enforced guarantee, not merely an inferred one.
- One trace format serving replay, debugging, and audit simultaneously, rather than three separate subsystems.

For the full design rationale, see [§22 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/22-comparison-matrix.md).
