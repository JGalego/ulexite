<div align="center">
<img src="assets/logo.svg" alt="Ulexite logo" width="140" height="140">
<h1>Ulexite</h1>
<p><strong>Stop scripting prompts. Start writing conversations.</strong></p>
<p>
<a href="https://github.com/JGalego/ulexite/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/JGalego/ulexite/actions/workflows/ci.yml/badge.svg"></a>
<a href="https://github.com/JGalego/ulexite/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/JGalego/ulexite"></a>
<a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
<a href="Cargo.toml"><img alt="Rust 2021" src="https://img.shields.io/badge/rust-2021-orange.svg"></a>
<img alt="Status: experimental" src="https://img.shields.io/badge/status-experimental-yellow.svg">
</p>
</div>

Ulexite is a programming language for conversational AI interactions. Its primary abstraction is the `Conversation`, not the prompt, the model, or the agent — with deterministic execution where possible, reproducible traces, and first-class testing.

> **Why "Ulexite"?** Ulexite is a real mineral, nicknamed the "TV rock" — it grows as a bundle of parallel fibers that pipe an image undistorted from one face of the stone to the other. Fitting for a language whose job is carrying a conversation faithfully from one end to the other.

## Install

**📦 Prebuilt binaries** — detects your OS/architecture automatically and installs both `ulx` (the CLI) and `ulx-lsp` (the language server, so an editor extension works immediately with no separate step):

```sh
# 🐧 Linux / 🍎 macOS (x86_64 or arm64)
curl -fsSL https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.sh | sh

# 🪟 Windows (x86_64), in PowerShell
irm https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.ps1 | iex
```

**🦀 From source** (needs Rust):

```sh
cargo install --git https://github.com/JGalego/ulexite ulx-cli --locked
cargo install --git https://github.com/JGalego/ulexite ulx-lsp --locked   # only needed for editor support
```

**🧩 VS Code / VSCodium / Cursor / Windsurf extension** — syntax highlighting plus hover, go-to-definition, document symbols, and completion via `ulx-lsp` (installed above):

- Search the Marketplace/Open VSX for **"Ulexite"** and install, or
- Grab the `.vsix` from the [latest release](https://github.com/JGalego/ulexite/releases/latest) and run `code --install-extension ulexite-*.vsix` (substitute `code` for `cursor`/`windsurf`/`codium` as needed).

## Try it

Scaffold a package and run it against a real provider — `ulx init` leaves `ulexite.toml`'s `[providers.*]` empty, so add one:

```sh
ulx init my-first-package /tmp/my-first-package
cd /tmp/my-first-package
ulx check main.ulx
cat >> ulexite.toml <<'EOF'

[providers.anthropic]
vendor = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
chat = "claude-haiku-4-5-20251001"
EOF
export ANTHROPIC_API_KEY=sk-ant-...
ulx run main.ulx Hello --arg name=world
```

Or drive a shipped example — `voice_memo.ulx` declares its own `provider` blocks right in the source (§21.10), so no `ulexite.toml` is needed:

```sh
cd examples
export GROQ_API_KEY=gsk_...
export OPENAI_API_KEY=sk-...
ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav
```

Human-approval suspend/resume round trip. Forcing a real judge to escalate isn't reliable on demand, so this one uses the deterministic offline provider (`--mock`) instead:

```sh
ulx run translate.ulx Translate --arg source="MOCK_JUDGE_ESCALATE please" --arg target_lang=fr --run-id demo --mock
ulx approve demo --value "human said: ship it"   # reuses the run's --mock automatically
ulx trace demo
```

Sample output — `ulx run` suspends on the judge's `Escalate`, `ulx approve` resumes and completes it, both as a dialogue transcript followed by a metadata footer:

```text
🧭 system: You are a professional translator.

🧑 user: Translate to fr: MOCK_JUDGE_ESCALATE please

🤖 assistant: [mock:chat] response to -> system: You are a professional translator. | user: Translate to fr: MOCK_JUDGE_ESCALATE please

⚖️  judge Fluency: Escalate

🙋 escalate human_approval: judge could not decide (suspended)

suspended: waiting on `human_approval` — judge could not decide
────────────────────────────────────────────
run id        demo
status        suspended
capabilities  chat, judge, escalate
provider      mock — chat, judge
resume with: ulx approve demo --value <text>   (or: ulx deny demo)
```

```text
$ ulx approve demo --value "human said: ship it"
🧭 system: You are a professional translator.

🧑 user: Translate to fr: MOCK_JUDGE_ESCALATE please

🤖 assistant: [mock:chat] response to -> system: You are a professional translator. | user: Translate to fr: MOCK_JUDGE_ESCALATE please

⚖️  judge Fluency: Escalate

🙋 escalate human_approval: judge could not decide => human said: ship it

human said: ship it
────────────────────────────────────────────
run id        demo
status        ok
capabilities  chat, judge, escalate
provider      mock — chat, judge
```

Colors show in a real terminal (disable with `NO_COLOR=1`); `ulx trace demo` replays every record from the log instead — one line per capability call, oldest first, `[miss]`/`[hit]`/`[err ]` marking cache status:

```text
#0   [miss] chat       [mock:chat] response to -> system: You are a professional translator. | user: Translate to fr: MOCK_...
#1   [miss] judge      Escalate
#2   [err ] escalate   suspended
#0   [hit ] chat       [mock:chat] response to -> system: You are a professional translator. | user: Translate to fr: MOCK_...
#1   [hit ] judge      Escalate
#2   [err ] escalate   suspended
#0   [hit ] chat       [mock:chat] response to -> system: You are a professional translator. | user: Translate to fr: MOCK_...
#1   [hit ] judge      Escalate
#2   [hit ] escalate   human said: ship it
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
| [`ulx-cli`](crates/ulx-cli) | The `ulx` binary: `parse`, `check`, `run`, `bench`, `plan`, `approve`/`deny`, `replay`, `trace`, `init`, `manifest`, `fmt` |
| [`ulx-lsp`](crates/ulx-lsp) | Language server: hover, go-to-definition, document symbols, completion |
| [`vscode-ulx`](tooling/vscode-ulx) | TextMate grammar + language config for `.ulx` syntax highlighting in VS Code, plus a client that launches `ulx-lsp` |

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

Every real HTTP call goes through retry-with-backoff plus a per-provider circuit breaker; a rate limit, timeout, or safety refusal surfaces as an unsettled `Draft<T>`, not a crash. `generate_image`/`speak` never retry on a client-side timeout specifically (unlike every other capability) — the vendor may have already completed and billed for the image/audio even though the response didn't arrive in time, so retrying risks paying for it twice. Adding a new provider needs no compiler/grammar/IR change — see `crates/ulx-runtime/src/provider/`.

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
export GROQ_API_KEY=gsk_...
export OPENAI_API_KEY=sk-...
run_id=$(ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav --output json | jq -r .run_id)
ulx trace "$run_id" --output mermaid
ulx trace "$run_id" --output jsonl
ulx trace "$run_id" --output html > trace.html
```

`voice_memo.ulx` declares its own `provider` blocks (§21.10), so no `ulexite.toml` is needed — it pins `transcribe`/`chat` to Groq and `speak` to OpenAI right in the source, one real vendor call per capability.

Sample `mermaid` output:

```mermaid
sequenceDiagram
    participant Program
    participant transcribe as transcribe
    participant chat as chat
    participant speak as speak
    Program->>+transcribe: #0 transcribe
    transcribe-->>-Program: [miss] Hey! This is a quick voice memo about the quarterly report. Can you send me a o...
    Program->>+chat: #1 chat
    chat-->>-Program: [miss] I've gone ahead and prepared a brief summary, which I'll send over to you shortl...
    Program->>+speak: #2 speak
    speak-->>-Program: [miss] .ulexite/artifacts/5e/d29d0f8cb4c24c.mp3
```

Sample `jsonl` output — one record per line, oldest first:

```json
{"cache_hit":false,"capability":"transcribe","error":null,"input":[],"kind":"effect","output":" Hey! This is a quick voice memo about the quarterly report. Can you send me a one-line summary before the meeting?","seq":0,"timestamp_ms":1784128850492}
{"cache_hit":false,"capability":"chat","error":null,"input":[{"role":"system","text":"You write a one-sentence spoken reply to a voice memo."},{"role":"user","text":"Voice memo transcript:\n Hey! This is a quick voice memo about the quarterly report. Can you send me a one-line summary before the meeting?"}],"kind":"effect","output":"I've gone ahead and prepared a brief summary, which I'll send over to you shortly, outlining our key quarterly metrics and performance highlights.","seq":1,"timestamp_ms":1784128850854}
{"cache_hit":false,"capability":"speak","error":null,"input":[{"role":"user","text":"I've gone ahead and prepared a brief summary, which I'll send over to you shortly, outlining our key quarterly metrics and performance highlights."}],"kind":"effect","output":".ulexite/artifacts/5e/d29d0f8cb4c24c.mp3","seq":2,"timestamp_ms":1784128853144}
```

`ulx run` also takes `--no-cache`, which skips the cache *read* for `ask`/`judge` calls (forcing a fresh live call every time) without touching `escalate`'s own cache entry — useful when iterating on a prompt/rubric under the same `--run-id`/args, where a stale cache hit would otherwise hide the change.

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

The `.ulx` programs referenced by the spec live in [`examples/`](examples/) — every one `ulx check`s with no configuration at all, and `cargo test` replays them offline end to end. Several (`voice_memo.ulx`, `rag.ulx`, `summarize.ulx`, `pdf_qa.ulx`, `generate_and_describe.ulx`) declare their own `provider` blocks and mix real vendors per capability out of the box — just export the API key(s) they need. See [`examples/README.md`](examples/README.md) for the full index and exact commands.

## Contributing / CI

```sh
just ci        # fmt-check + clippy (-D warnings) + build + test — the same gate CI runs
just           # list every other recipe (build, test, fmt, install, check-examples, clean, ...)
```

Every push and PR runs that same gate, validates the VS Code extension's JSON, and re-checks every example under `examples/` — see [`.github/workflows/ci.yml`](.github/workflows/ci.yml). Pushing a tag like `v0.1.0` triggers [`.github/workflows/release.yml`](.github/workflows/release.yml), which cross-builds `ulx` for Linux, macOS, and Windows (x86_64 + arm64) and publishes them to a GitHub Release.
