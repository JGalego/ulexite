<div align="center">
<img src="assets/logo.svg" alt="Ulexite logo" width="140" height="140">
<h1>Ulexite</h1>
<p><strong>Stop scripting prompts. Start writing conversations.</strong></p>
<p>
<a href="https://github.com/JGalego/ulexite/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/JGalego/ulexite/actions/workflows/ci.yml/badge.svg"></a>
<a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
<a href="Cargo.toml"><img alt="Rust 2021" src="https://img.shields.io/badge/rust-2021-orange.svg"></a>
<a href="docs/spec/24-limitations.md"><img alt="Status: experimental" src="https://img.shields.io/badge/status-experimental-yellow.svg"></a>
</p>
</div>

Ulexite is a programming language for conversational AI interactions. Its primary abstraction is the `Conversation`, not the prompt, the model, or the agent. The runtime executes conversations involving humans, LLMs, tools, judges, datasets, and multimodal artifacts, with deterministic execution where possible, reproducible traces, and first-class testing.

This repository contains the language specification (RFC-0001) and a working reference implementation: a lexer, parser, semantic analyzer, IR lowering pass, and a tree-walking runtime with a content-addressed cache, a JSONL trace log, real `with`-block concurrency, and a suspend/resume flow for human-approval checkpoints. By default everything runs against a deterministic mock provider, so the whole thing runs and tests fully offline, no API key required; real HTTP-backed providers (OpenAI, Anthropic, Gemini, Groq, Cohere, Ollama, and any OpenAI-compatible server such as vLLM or LM Studio) are one `ulexite.toml` `[providers]` entry away — see [Configuring providers](#configuring-providers) below. See [§13 Compiler Architecture](docs/spec/13-compiler-architecture.md), [§12 Runtime Architecture](docs/spec/12-runtime-architecture.md), and [§24 Limitations](docs/spec/24-limitations.md) for exactly what is and isn't implemented yet.

> **Why "Ulexite"?** Ulexite is a real mineral, nicknamed the "TV rock" — it naturally grows as a bundle of parallel, fiber-optic-like crystal fibers that pipe an image straight through the stone, undistorted, from one face to the other. That felt like the right name for a language whose whole job is carrying a conversation — through models, tools, judges, and retries — faithfully from one end to the other.

## Install

**Prebuilt binary** — no Rust toolchain needed, detects your OS/architecture automatically:

Linux / macOS (x86_64 or arm64):

```sh
curl -fsSL https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.sh | sh
```

Windows (x86_64):

```powershell
irm https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.ps1 | iex
```

Both scripts ([`scripts/install.sh`](scripts/install.sh), [`scripts/install.ps1`](scripts/install.ps1)) just fetch the matching archive from the [latest release](https://github.com/JGalego/ulexite/releases/latest), so reading either one tells you exactly what it's going to do before you pipe it into a shell. Prefer to do it by hand? Grab the archive for your platform from the same releases page instead.

**From source** (any platform with Rust installed):

```sh
cargo install --git https://github.com/JGalego/ulexite ulx-cli --locked
```

## Try it

```sh
# scaffold a new package (writes ulexite.toml + main.ulx into the given directory)
ulx init my-first-package /tmp/my-first-package
ulx check /tmp/my-first-package/main.ulx
ulx run /tmp/my-first-package/main.ulx Hello --arg name=world --mock
```

Or drive one of the shipped examples directly, including a human-approval suspend/resume round trip:

```sh
ulx run examples/translate.ulx Translate --arg source=hello --arg target_lang=fr --mock
ulx run examples/translate.ulx Translate --arg source="MOCK_JUDGE_ESCALATE please" --arg target_lang=fr --run-id demo --mock
ulx approve demo --value "human said: ship it" --mock
ulx trace demo
```

`--mock` is required here on purpose: `ulx run`/`approve`/`deny` now error if no provider is configured at all, rather than silently defaulting to mock — see "Configuring providers" below.

## What's implemented

| Crate | What it does |
|---|---|
| [`crates/ulx-ast`](crates/ulx-ast) | AST node definitions with source spans (§13.4) |
| [`crates/ulx-syntax`](crates/ulx-syntax) | Lexer (`logos`) + parser (`chumsky`) implementing the grammar in [§8](docs/spec/08-grammar.md), including interpolated text blocks |
| [`crates/ulx-sema`](crates/ulx-sema) | Name/import resolution across files, artifact-type checking for `ask` calls (§9.2), `Verdict` match-exhaustiveness (§9.4), `with`-block independence checking (§9.7) |
| [`crates/ulx-ir`](crates/ulx-ir) | Lowers the AST to a pure/effect IR (§13.4), desugaring message-literal sugar into explicit `chat` effects, plus a dead-binding elimination pass (§13.5) |
| [`crates/ulx-runtime`](crates/ulx-runtime) | Tree-walking interpreter (§12.2) — a pluggable `Provider` trait with a deterministic `MockProvider` plus real HTTP-backed adapters (OpenAI/Groq/any OpenAI-compatible server, Anthropic, Gemini, Cohere, Ollama), a content-addressed cache and trace log (§10.3, §18), real concurrent `with`-block execution (`std::thread::scope`), and cache-backed suspend/resume for `escalate` (§10.7) |
| [`crates/ulx-cli`](crates/ulx-cli) | The `ulx` binary: `parse`, `check`, `run`, `approve`/`deny`, `replay`, `trace`, `init`, `manifest` |
| [`tooling/vscode-ulx`](tooling/vscode-ulx) | TextMate grammar + language config for `.ulx` syntax highlighting in VS Code (§20.10) |

Not implemented: PDF/video input to `vision` and a real content-addressed artifact store (a real vendor's `speak`/`generate_image` output is written to a temp file today, not a proper blob store), `benchmark`/`test` execution, `plan`'s cost estimation, a formatter, a language server, and package dependency resolution beyond parsing `ulexite.toml` — see [§24 Limitations](docs/spec/24-limitations.md) and [§25 Future Directions](docs/spec/25-future-directions.md) for the honest accounting and the plan.

## Configuring providers

`ulx run`/`approve`/`deny` require a configured provider and error otherwise — pass `--mock` to run against the deterministic, fully-offline mock provider explicitly, or configure a real vendor. This is deliberate: a silently-defaulted-to-mock run that you *thought* was hitting a real API is a worse failure mode than an upfront error.

To use a real vendor, add a `[providers.<name>]` table to `ulexite.toml` next to your `.ulx` file — one table per vendor account/deployment, `vendor` mandatory (never inferred from the table name, so two entries for the same vendor are unambiguous), and every other key a capability name mapped to a model:

```toml
[providers.anthropic]
vendor = "anthropic"                          # openai | azure_openai | anthropic | gemini | groq | cohere | ollama | openai_compatible | mock
api_key_env = "ANTHROPIC_API_KEY"              # name of an env var — never a literal key in this file
vision = "claude-3-5-sonnet-20241022"          # bare string = just the model name

[providers.anthropic.chat]                     # per-capability overrides need this longer table form instead:
model = "claude-3-5-sonnet-20241022"

[providers.anthropic.chat.params]
temperature = 0.2                              # defaults, overridable per call: ask chat(temperature: 0.7) { ... }

[providers.local_llm]
vendor = "openai_compatible"                   # any OpenAI-shaped /chat/completions server: vLLM, LM Studio, Groq, etc.
base_url = "http://localhost:8000/v1"
chat = "meta-llama/Llama-3-8b"

[providers.openai]
vendor = "openai"
api_key_env = "OPENAI_API_KEY"
transcribe = "whisper-1"                       # each vendor entry can list as many capabilities as it serves
speak = "tts-1"
generate_image = "dall-e-3"

[providers.azure]
vendor = "azure_openai"                        # same JSON shape as OpenAI, different URL/auth conventions
base_url = "https://my-resource.openai.azure.com"
api_key_env = "AZURE_OPENAI_API_KEY"
api_version = "2024-06-01"                     # optional; defaults to a recent stable version
chat = "my-gpt4o-deployment"                   # this is your *deployment name*, not a generic model id
```

`vendor = "ollama"` needs no API key and defaults to `http://localhost:11434`. `chat` is implemented for every vendor; `embed` for `openai_compatible`/`gemini`/`cohere`/`ollama`/`azure_openai`; `vision` for `openai_compatible`/`anthropic`/`gemini`/`ollama`/`azure_openai` (image files only — jpg/png/gif/webp, read straight off disk or passed through as an `http(s)://` URL where the vendor supports it; PDF/video are mock-only); `transcribe`/`speak`/`generate_image` for `openai_compatible` (covers OpenAI directly, and Groq for `transcribe`; not yet implemented for `azure_openai`, which does offer Whisper/TTS/DALL-E deployments of its own). Every real HTTP call goes through one retry-with-backoff policy plus a per-provider circuit breaker (`crates/ulx-runtime/src/provider/transport.rs`) — a handful of consecutive failures trips it open for a cooldown instead of hammering a downed vendor. A rate limit, timeout, or safety refusal surfaces as an unsettled `Draft<T>` (§9.3), not a crash. Adding a provider that isn't listed above needs no compiler/grammar/IR change (§12.4) — see `crates/ulx-runtime/src/provider/`.

If more than one registered provider serves the same capability (two `[providers.*]` entries both declaring `chat`, say) and nothing disambiguates it, `ask` fails with a clear `Ambiguous` error naming every candidate — it never silently picks one. Disambiguate either per call, with the reserved `provider:` arg (`ask chat(provider: "anthropic") { ... }`), or for the whole run, with `--provider name` on the CLI (repeatable; only the named provider(s) get registered at all).

### Declaring a provider in `.ulx` source

A `provider` block can also be declared directly in `.ulx` source — standalone (no `ulexite.toml` needed at all) or layered on top of a manifest entry:

```
provider MyAnthropic from "anthropic" {   // inherits vendor/api_key_env/etc. from [providers.anthropic]
  vision: "claude-3-5-sonnet-20241022"    // adds a capability the manifest entry didn't have
}

provider Local {                          // fully standalone — no ulexite.toml needed
  vendor: "openai_compatible"
  base_url: "http://localhost:8000/v1"
  chat: "meta-llama/Llama-3-8b"
}

conversation Greet(name: text) -> text {
  ask chat(provider: "Local") { user: """Say hello to {name}.""" } -> greeting: text
  greeting
}
```

See [`examples/custom_provider.ulx`](examples/custom_provider.ulx) for a runnable, fully-offline version (§21.12). `provider` decls can be imported across files too, the same way `judge`/`conversation`/`dataset` already are (`import provider Prod from "providers.ulx"`). This is the one place `.ulx` source can name an actual vendor — every `ask` call site still only ever names a capability (plus, optionally, a provider *name*, never a vendor kind directly); §12.4's provider-independence principle still holds for ordinary `ask` calls, this is an explicit, opt-in escape hatch layered on top of it, not a replacement for it.

**API keys via `.env`**: `ulx run` also loads a `.env` file next to the `.ulx` file being run, if one exists, before resolving providers — so `OPENAI_API_KEY=sk-...` can live in a local, gitignored `.env` instead of being `export`ed by hand every session. A real shell-exported variable always wins over the `.env` file's value. See [`examples/.env.example`](examples/.env.example).

## How it compares

A condensed slice of [§22's full comparison matrix](docs/spec/22-comparison-matrix.md) (11 systems, 19 capabilities) — ratings are **Yes** (native/structural), **Partial** (bolt-on/wrapper), or **No**, grounded in the specific findings of [§2's prior-art survey](docs/spec/02-prior-art-survey.md), not general reputation:

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

Where existing systems are honestly better and where Ulexite's design actually introduces something new (not just a recombination) are both spelled out in [§22.1](docs/spec/22-comparison-matrix.md#221-where-existing-systems-are-genuinely-superior-today) and [§22.2](docs/spec/22-comparison-matrix.md#222-where-ulexite-introduces-genuinely-new-abstractions) — and [§26 Self-Evaluation](docs/spec/26-self-evaluation.md) argues the skeptical case against Ulexite's own novelty before concluding.

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

The `.ulx` programs referenced by the spec live in [`examples/`](examples/), each with any fixture data it needs (`examples/tickets/`, `examples/kb/`, `examples/fixtures/`). All of them parse, type-check with zero diagnostics, and lower to IR (`cargo test` covers this end to end), and every conversation in every file runs to completion against the mock provider — `approval.ulx` deliberately suspends on `escalate` instead, demonstrating the human-approval flow (see [Try it](#try-it) above). `eval_translate.ulx` is the one exception: it's a `benchmark` declaration, which the CLI doesn't execute yet (§16, see Limitations).

Two examples specifically exercise the newer capabilities against a real provider: [`voice_memo.ulx`](examples/voice_memo.ulx) (`transcribe` + `speak`, §21.10) and [`generate_and_describe.ulx`](examples/generate_and_describe.ulx) (`generate_image` + `vision`, §21.11) — see [`examples/fixtures/`](examples/fixtures/) for real (if tiny) `sample.png`/`sample.wav` fixtures, and [`examples/ulexite.example.toml`](examples/ulexite.example.toml) for a `[providers]` block covering every real-vendor capability, ready to copy to `examples/ulexite.toml` once you've set the matching API key env var:

```sh
export OPENAI_API_KEY=sk-...
cp examples/ulexite.example.toml examples/ulexite.toml
ulx run examples/rag.ulx Caption --arg photo=examples/fixtures/sample.png
ulx run examples/voice_memo.ulx VoiceMemoReply --arg recording=examples/fixtures/sample.wav
ulx run examples/generate_and_describe.ulx GenerateAndDescribe --arg prompt="a lighthouse at sunset"
```

## Contributing / CI

```sh
just ci        # fmt-check + clippy (-D warnings) + build + test — the same gate CI runs
just           # list every other recipe (build, test, fmt, install, check-examples, clean, ...)
```

Every push and PR runs that same gate, validates the VS Code extension's JSON, and re-checks every example under `examples/` — see [`.github/workflows/ci.yml`](.github/workflows/ci.yml). Pushing a tag like `v0.1.0` triggers [`.github/workflows/release.yml`](.github/workflows/release.yml), which cross-builds `ulx` for Linux, macOS, and Windows (x86_64 + arm64) and publishes them to a GitHub Release.
