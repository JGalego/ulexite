<div align="center">
<img src="assets/logo.svg" alt="Ulexite logo" width="140" height="140">
<h1>Ulexite</h1>
<p><strong>Stop scripting prompts. Start writing conversations.</strong></p>
<p>
<a href="https://github.com/JGalego/ulexite/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/JGalego/ulexite/actions/workflows/ci.yml/badge.svg"></a>
<a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
<a href="Cargo.toml"><img alt="Rust 2021" src="https://img.shields.io/badge/rust-2021-orange.svg"></a>
<img alt="Status: experimental" src="https://img.shields.io/badge/status-experimental-yellow.svg">
</p>
</div>

Ulexite is a programming language for conversational AI interactions. Its primary abstraction is the `Conversation`, not the prompt, the model, or the agent — with deterministic execution where possible, reproducible traces, and first-class testing.

> **Why "Ulexite"?** Ulexite is a real mineral, nicknamed the "TV rock" — it grows as a bundle of parallel fibers that pipe an image undistorted from one face of the stone to the other. Fitting for a language whose job is carrying a conversation faithfully from one end to the other.

## Install

**Prebuilt binary** — detects your OS/architecture automatically:

```sh
# Linux / macOS (x86_64 or arm64)
curl -fsSL https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.sh | sh

# Windows (x86_64), in PowerShell
irm https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.ps1 | iex
```

**From source** (needs Rust):

```sh
cargo install --git https://github.com/JGalego/ulexite ulx-cli --locked
```

## Try it

```sh
ulx init my-first-package /tmp/my-first-package
ulx check /tmp/my-first-package/main.ulx
ulx run /tmp/my-first-package/main.ulx Hello --arg name=world --mock
```

Or drive a shipped example against a real provider:

```sh
cd examples
export OPENAI_API_KEY=sk-...
cp ulexite.example.toml ulexite.toml
ulx run translate.ulx Translate --arg source=hello --arg target_lang=fr
```

Human-approval suspend/resume round trip (this one forces a judge escalation, so it stays on `--mock`):

```sh
ulx run translate.ulx Translate --arg source="MOCK_JUDGE_ESCALATE please" --arg target_lang=fr --run-id demo --mock
ulx approve demo --value "human said: ship it"   # reuses the run's --mock automatically
ulx trace demo
```

Or answer it live at the terminal instead, with `--interactive`:

```sh
ulx run translate.ulx Translate --arg source="MOCK_JUDGE_ESCALATE please" --arg target_lang=fr --mock --interactive
```

## What's implemented

| Crate | What it does |
|---|---|
| [`ulx-ast`](crates/ulx-ast) | AST node definitions with source spans |
| [`ulx-syntax`](crates/ulx-syntax) | Lexer + parser for the grammar, including interpolated text blocks |
| [`ulx-sema`](crates/ulx-sema) | Name/import resolution, artifact-type checking, `Verdict` exhaustiveness, `with`-block independence checking |
| [`ulx-ir`](crates/ulx-ir) | Lowers the AST to a pure/effect IR, desugars message literals, dead-binding elimination |
| [`ulx-runtime`](crates/ulx-runtime) | Interpreter: pluggable providers (mock + OpenAI/Groq/Anthropic/Gemini/Cohere/Ollama), content-addressed cache + trace log, real concurrent `with` execution, cache-backed suspend/resume for `escalate` |
| [`ulx-cli`](crates/ulx-cli) | The `ulx` binary: `parse`, `check`, `run`, `approve`/`deny`, `replay`, `trace`, `init`, `manifest` |
| [`vscode-ulx`](tooling/vscode-ulx) | TextMate grammar + language config for `.ulx` syntax highlighting in VS Code |

## Configuring providers

`ulx run`/`approve`/`deny` need a configured provider: pass `--mock` for the deterministic offline mock, or add a `[providers.<name>]` table to `ulexite.toml` next to your `.ulx` file:

```toml
[providers.anthropic]
vendor = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
chat = "claude-3-5-sonnet-20241022"

[providers.local_llm]
vendor = "openai_compatible"   # any OpenAI-shaped /chat/completions server: vLLM, LM Studio, Groq, ...
base_url = "http://localhost:8000/v1"
chat = "meta-llama/Llama-3-8b"
```

`vendor` is one of `openai | azure_openai | anthropic | gemini | groq | cohere | ollama | openai_compatible | mock` (never inferred from the table name, so two entries for the same vendor are unambiguous). Every other key names a capability; use `{ model = "...", params = { ... } }` instead of a bare string for per-capability overrides like `temperature`. `ollama` needs no API key and defaults to `localhost:11434`.

| Capability | Supported vendors |
|---|---|
| `chat`, `judge` | every vendor (`judge` routes through that vendor's own `chat`) |
| `embed` | openai_compatible, gemini, cohere, ollama, azure_openai |
| `vision` | openai_compatible, anthropic, gemini, ollama, azure_openai — image files (jpg/png/gif/webp); anthropic also accepts PDF |
| `transcribe` / `speak` / `generate_image` | openai_compatible only (OpenAI directly; Groq for `transcribe`) |

Every real HTTP call goes through retry-with-backoff plus a per-provider circuit breaker; a rate limit, timeout, or safety refusal surfaces as an unsettled `Draft<T>`, not a crash. Adding a new provider needs no compiler/grammar/IR change — see `crates/ulx-runtime/src/provider/`.

If two registered providers serve the same capability and nothing disambiguates it, `ask` fails with a clear `Ambiguous` error rather than silently picking one. Disambiguate per call with `ask chat(provider: "anthropic") { ... }`, or for the whole run with `--provider name` (repeatable).

### Declaring a provider in `.ulx` source

A `provider` block can also be declared directly in `.ulx` source — standalone, or layered on a `ulexite.toml` entry with `from`:

```
provider Local {
  vendor: "openai_compatible"
  base_url: "http://localhost:8000/v1"
  chat: "meta-llama/Llama-3-8b"
}

conversation Greet(name: text) -> text {
  ask chat(provider: "Local") { user: """Say hello to {name}.""" } -> greeting: text
  greeting
}
```

See [`examples/custom_provider.ulx`](examples/custom_provider.ulx) for a runnable version. `provider` decls can be imported across files too, the same way `judge`/`conversation`/`dataset` already are.

**API keys via `.env`**: `ulx run` loads a `.env` file next to the `.ulx` file, if one exists, before resolving providers — a real shell-exported variable always wins. See [`examples/.env.example`](examples/.env.example).

## Output formats

`ulx run`/`approve`/`deny`/`replay`/`trace` all take `--output <FORMAT>`, defaulting to `text`:

- `text` — a final value on stdout (`run id: ...` on stderr), a `suspended: ...`/resume hint, or `error: ...` on stderr.
- `json` — one JSON object, always on stdout, always carrying `run_id` (so a script can chain into `ulx trace` without passing `--run-id` up front).
- `jsonl` — one JSON line per trace record, newline-delimited — the whole run's trace, not just the final value.
- `mermaid` — a `sequenceDiagram` of the run's trace; paste into a Markdown/Mermaid renderer.
- `html` — a self-contained page rendering the trace as status-colored cards.

```sh
cd examples
run_id=$(ulx run translate.ulx Translate --arg source=hello --arg target_lang=fr --mock --output json | jq -r .run_id)
ulx trace "$run_id" --output mermaid
ulx trace "$run_id" --output html > trace.html
```

`jsonl`/`mermaid`/`html` always describe the whole trace, even via `run`/`approve`/`deny`/`replay`. Errors before a conversation starts running (unreadable file, ambiguous/unconfigured provider, bad `--arg`) are always plain text on stderr regardless of `--output`.

## How it compares

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

## Read the spec

Start at [docs/spec/00-index.md](docs/spec/00-index.md) for the full table of contents:

- **Why does this need to exist?** → [§1 Vision](docs/spec/01-vision.md), [§3 Gap Analysis](docs/spec/03-gap-analysis.md)
- **What did you learn from Guidance/LangGraph/DSPy/etc.?** → [§2 Prior Art Survey](docs/spec/02-prior-art-survey.md)
- **What does the language actually look like?** → [§7 Recommended Syntax](docs/spec/07-recommended-syntax.md), [§21 Complete Examples](docs/spec/21-examples.md)
- **What are the formal semantics?** → [§8 Grammar](docs/spec/08-grammar.md), [§9 Type System](docs/spec/09-type-system.md), [§10 Execution Semantics](docs/spec/10-execution-semantics.md)
- **How would this be built?** → [§12 Runtime Architecture](docs/spec/12-runtime-architecture.md), [§13 Compiler Architecture](docs/spec/13-compiler-architecture.md)
- **How does this compare to what I already use?** → [§22 Comparison Matrix](docs/spec/22-comparison-matrix.md), [§23 Migration Paths](docs/spec/23-migration-paths.md)
- **Is this actually novel, or just a remix?** → [§26 Self-Evaluation](docs/spec/26-self-evaluation.md)
- **What doesn't this solve?** → [§24 Limitations](docs/spec/24-limitations.md)

## Example programs

The `.ulx` programs referenced by the spec live in [`examples/`](examples/) and all run against the mock provider by default (`cargo test` covers this end to end). Three exercise real-vendor capabilities:

```sh
cd examples
export OPENAI_API_KEY=sk-...
cp ulexite.example.toml ulexite.toml

ulx run rag.ulx Caption --arg photo=fixtures/sample.png                                      # vision
ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav                     # transcribe + speak
ulx run generate_and_describe.ulx GenerateAndDescribe --arg prompt="a lighthouse at sunset"   # generate_image + vision
```

## Contributing / CI

```sh
just ci        # fmt-check + clippy (-D warnings) + build + test — the same gate CI runs
just           # list every other recipe (build, test, fmt, install, check-examples, clean, ...)
```

Every push and PR runs that same gate, validates the VS Code extension's JSON, and re-checks every example under `examples/` — see [`.github/workflows/ci.yml`](.github/workflows/ci.yml). Pushing a tag like `v0.1.0` triggers [`.github/workflows/release.yml`](.github/workflows/release.yml), which cross-builds `ulx` for Linux, macOS, and Windows (x86_64 + arm64) and publishes them to a GitHub Release.
