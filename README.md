# Ulexite

**CLI:** `ulx`

Ulexite is a programming language for conversational AI interactions. Its primary abstraction is the **conversation**, not the prompt, the model, or the agent. The runtime executes conversations involving humans, LLMs, tools, judges, datasets, and multimodal artifacts, with deterministic execution where possible, reproducible traces, and first-class testing.

This repository currently contains the language specification (RFC-0001) — no compiler or runtime has been implemented yet. See [§13 Compiler Architecture](docs/spec/13-compiler-architecture.md) and [§25 Future Directions](docs/spec/25-future-directions.md) for the implementation plan.

## Read the spec

Start at [docs/spec/00-index.md](docs/spec/00-index.md) for the full table of contents. Suggested entry points depending on what you're looking for:

- **Why does this need to exist?** → [§1 Vision](docs/spec/01-vision.md), [§3 Gap Analysis](docs/spec/03-gap-analysis.md)
- **What did you learn from Guidance/LangGraph/DSPy/etc.?** → [§2 Prior Art Survey](docs/spec/02-prior-art-survey.md)
- **What does the language actually look like?** → [§7 Recommended Syntax](docs/spec/07-recommended-syntax.md), [§21 Complete Examples](docs/spec/21-examples.md)
- **What are the formal semantics?** → [§8 Grammar](docs/spec/08-grammar.md), [§9 Type System](docs/spec/09-type-system.md), [§10 Execution Semantics](docs/spec/10-execution-semantics.md)
- **How would this be built?** → [§12 Runtime Architecture](docs/spec/12-runtime-architecture.md), [§13 Compiler Architecture](docs/spec/13-compiler-architecture.md)
- **How does this compare to what I already use?** → [§22 Comparison Matrix](docs/spec/22-comparison-matrix.md), [§23 Migration Paths](docs/spec/23-migration-paths.md)
- **Is this actually novel, or just a remix?** → [§26 Self-Evaluation](docs/spec/26-self-evaluation.md)
- **What doesn't this solve?** → [§24 Limitations](docs/spec/24-limitations.md)

## Example programs

Runnable-syntax (not yet executable — no runtime exists) example `.ulx` programs referenced by the spec live in [`examples/`](examples/).
