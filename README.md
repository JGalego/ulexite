<div align="center">
  <img src="assets/logo.svg" alt="Ulexite logo" width="160" height="160">

  # Ulexite

 > The conversation is the program.

  [![CI](https://github.com/JGalego/ulexite/actions/workflows/ci.yml/badge.svg)](https://github.com/JGalego/ulexite/actions/workflows/ci.yml) [![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE) [![Rust 2021](https://img.shields.io/badge/rust-2021-orange.svg)](Cargo.toml) [![Status: experimental](https://img.shields.io/badge/status-experimental-yellow.svg)](docs/spec/24-limitations.md)
</div>

Ulexite is a programming language for conversational AI interactions. Its primary abstraction is the `Conversation`, not the prompt, the model, or the agent. The runtime executes conversations involving humans, LLMs, tools, judges, datasets, and multimodal artifacts, with deterministic execution where possible, reproducible traces, and first-class testing.

This repository contains the language specification (RFC-0001) and a working reference implementation: a lexer, parser, semantic analyzer, IR lowering pass, and a tree-walking runtime with a content-addressed cache, a JSONL trace log, real `with`-block concurrency, and a suspend/resume flow for human-approval checkpoints — all exercised against a deterministic mock provider so the whole thing runs and tests fully offline, no API key required. See [§13 Compiler Architecture](docs/spec/13-compiler-architecture.md), [§12 Runtime Architecture](docs/spec/12-runtime-architecture.md), and [§24 Limitations](docs/spec/24-limitations.md) for exactly what is and isn't implemented yet.

## Try it

```sh
cargo build
cargo test

# put a real `ulx` binary on your PATH (rerun after pulling changes to update it)
cargo install --path crates/ulx-cli --locked

# scaffold a new package (writes ulexite.toml + main.ulx into the given directory)
ulx init my-first-package /tmp/my-first-package
ulx check /tmp/my-first-package/main.ulx
ulx run /tmp/my-first-package/main.ulx Hello --arg name=world
```

Or drive one of the shipped examples directly, including a human-approval suspend/resume round trip:

```sh
ulx run examples/translate.ulx Translate --arg source=hello --arg target_lang=fr
ulx run examples/translate.ulx Translate --arg source="MOCK_JUDGE_ESCALATE please" --arg target_lang=fr --run-id demo
ulx approve demo --value "human said: ship it"
ulx trace demo
```

Don't want to install it? Skip `cargo install` and use `./target/debug/ulx` (after `cargo build`) or `cargo run --bin ulx --` in place of `ulx` everywhere above.

## What's implemented

| Crate | What it does |
|---|---|
| [`crates/ulx-ast`](crates/ulx-ast) | AST node definitions with source spans (§13.4) |
| [`crates/ulx-syntax`](crates/ulx-syntax) | Lexer (`logos`) + parser (`chumsky`) implementing the grammar in [§8](docs/spec/08-grammar.md), including interpolated text blocks |
| [`crates/ulx-sema`](crates/ulx-sema) | Name/import resolution across files, artifact-type checking for `ask` calls (§9.2), `Verdict` match-exhaustiveness (§9.4), `with`-block independence checking (§9.7) |
| [`crates/ulx-ir`](crates/ulx-ir) | Lowers the AST to a pure/effect IR (§13.4), desugaring message-literal sugar into explicit `chat` effects, plus a dead-binding elimination pass (§13.5) |
| [`crates/ulx-runtime`](crates/ulx-runtime) | Tree-walking interpreter (§12.2) — a pluggable `Provider` trait with a deterministic `MockProvider`, a content-addressed cache and trace log (§10.3, §18), real concurrent `with`-block execution (`std::thread::scope`), and cache-backed suspend/resume for `escalate` (§10.7) |
| [`crates/ulx-cli`](crates/ulx-cli) | The `ulx` binary: `parse`, `check`, `run`, `approve`/`deny`, `replay`, `trace`, `init`, `manifest` |
| [`tooling/vscode-ulx`](tooling/vscode-ulx) | TextMate grammar + language config for `.ulx` syntax highlighting in VS Code (§20.10) |

Not implemented: a real HTTP-backed provider, `benchmark`/`test` execution, `plan`'s cost estimation, a formatter, a language server, and package dependency resolution beyond parsing `ulexite.toml` — see [§24 Limitations](docs/spec/24-limitations.md) and [§25 Future Directions](docs/spec/25-future-directions.md) for the honest accounting and the plan.

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

The `.ulx` programs referenced by the spec live in [`examples/`](examples/). All of them parse, type-check with zero diagnostics, and lower to IR (`cargo test` covers this end to end). Most run to completion against the mock provider (`translate.ulx`, `summarize.ulx`, `pdf_qa.ulx`, `batch.ulx`, `multi_agent.ulx`); `approval.ulx` deliberately suspends on `escalate`, demonstrating the human-approval flow; `rag.ulx` references an illustrative dataset file (`kb/chunks.jsonl`) that doesn't exist in this repo, so it fails cleanly with an I/O error rather than silently; `eval_translate.ulx` is a `benchmark` declaration, which the CLI doesn't execute yet (§16, see Limitations).

## Contributing / CI

Every push and PR runs `cargo fmt --check`, `cargo clippy -D warnings`, `cargo build`, and `cargo test` across the whole workspace, validates the VS Code extension's JSON, and re-checks every example under `examples/` — see [`.github/workflows/ci.yml`](.github/workflows/ci.yml).
