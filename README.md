# Ulexite

**CLI:** `ulx`

Ulexite is a programming language for conversational AI interactions. Its primary abstraction is the **conversation**, not the prompt, the model, or the agent. The runtime executes conversations involving humans, LLMs, tools, judges, datasets, and multimodal artifacts, with deterministic execution where possible, reproducible traces, and first-class testing.

This repository contains the language specification (RFC-0001) and an in-progress reference implementation. The front end (lexer + parser + AST, §13) exists and parses every example under `examples/`; semantic analysis, the IR, and the runtime (§9, §10, §12) are not implemented yet — see [§13 Compiler Architecture](docs/spec/13-compiler-architecture.md) and [§25 Future Directions](docs/spec/25-future-directions.md) for the plan.

## Building

```sh
cargo build
cargo test
cargo run --bin ulx -- parse examples/translate.ulx
```

- `crates/ulx-ast` — AST node definitions (§13.4)
- `crates/ulx-syntax` — lexer (`logos`) + parser (`chumsky`), implementing the grammar in [§8](docs/spec/08-grammar.md)
- `crates/ulx-cli` — the `ulx` binary (`parse`/`check` today; `run`/`test`/`plan`/`debug` depend on later compiler stages)
- `tooling/vscode-ulx` — TextMate grammar + language config for `.ulx` syntax highlighting in VS Code (§20.10)

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

The `.ulx` programs referenced by the spec live in [`examples/`](examples/) and all parse successfully (`cargo test` includes a golden-file test over this directory) — there's no runtime yet, so they don't *execute*.
