---
title: Providers
description: Configure real vendors or the offline mock in ulexite.toml, or declare a provider directly in .ulx source.
---

# Providers

`ulx run`, `ulx approve`, and `ulx deny` all need a configured provider before they can do anything real. You have two ways to get one: pass `--mock` for the deterministic offline provider, or configure a real vendor. This page covers the second path — how you declare a provider, what capabilities each vendor actually supports today, and how ambiguity between multiple providers gets resolved.

## The two ways to declare a provider

**In `ulexite.toml`**, next to your `.ulx` file, add a `[providers.<name>]` table:

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

**Directly in `.ulx` source**, standalone or layered on a `ulexite.toml` entry with `from`:

```ulexite
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

A `.ulx`-declared `provider` can be imported across files, the same way `judge`/`conversation`/`dataset` already are. See [`examples/custom_provider.ulx`](https://github.com/JGalego/ulexite/tree/main/examples/custom_provider.ulx) for a runnable version, and [`examples/voice_memo.ulx`](https://github.com/JGalego/ulexite/tree/main/examples/voice_memo.ulx) for an example that declares two providers and pins a different vendor to each capability — no `ulexite.toml` needed at all.

Either form gives you a table with the same shape: a mandatory `vendor`, whatever connection details that vendor needs (`api_key_env`, `base_url`, `api_version`), and then one entry per capability you want that provider to serve.

## `vendor` is never inferred

`vendor` names which adapter to build, and it is **never** inferred from the table name — so two entries for the same vendor (say, two different `openai_compatible` servers) are unambiguous:

```toml
[providers.vllm_local]
vendor   = "openai_compatible"
base_url = "http://localhost:8000/v1"
chat     = "meta-llama/Llama-3-8b"

[providers.vllm_remote]
vendor   = "openai_compatible"
base_url = "http://gpu-box:8000/v1"
chat     = "meta-llama/Llama-3-70b"
```

The supported values are:

| `vendor` | What it connects to |
|---|---|
| `openai` | OpenAI's API directly |
| `azure_openai` | Azure OpenAI — a per-customer resource endpoint |
| `anthropic` | Anthropic's Messages API |
| `gemini` | Google Gemini |
| `groq` | Groq's hosted inference |
| `cohere` | Cohere's Chat v2 + embeddings |
| `ollama` | A local Ollama server |
| `openai_compatible` | Any other server speaking the OpenAI `/chat/completions` shape — vLLM, LM Studio, text-generation-webui, Groq, and so on |
| `mock` | The deterministic offline default |

## Capabilities, not models

Every key in a provider table besides `vendor`/`base_url`/`api_key_env`/`api_version` names a **capability** — `chat`, `judge`, `vision`, `embed`, `transcribe`, `speak`, `generate_image` — mapped to a model name:

```toml
[providers.openai]
vendor         = "openai"
api_key_env    = "OPENAI_API_KEY"
chat           = "gpt-4o-mini"
judge          = "gpt-4o-mini"
vision         = "gpt-4o-mini"
embed          = "text-embedding-3-small"
transcribe     = "whisper-1"
speak          = "tts-1"
generate_image = "gpt-image-1"
```

A conversation step never names a vendor directly — it names the capability it needs (`ask vision(...)`, `ask chat(...)`), and the runtime's provider registry resolves which configured provider actually serves that call.

Use a `{ model = "...", params = { ... } }` table instead of a bare string when you need per-capability overrides like `temperature`:

```toml
[providers.openai.chat]
model = "gpt-4o-mini"

[providers.openai.chat.params]
temperature = 0.2
max_tokens = 512
```

`ollama` needs no API key and defaults to `localhost:11434` if `base_url` is omitted. `azure_openai` is the one vendor with no fixed default `base_url` — since Azure's endpoint is per-customer (`https://<resource>.openai.azure.com`), `base_url` is mandatory, `api_version` is mandatory (or defaults to a recent stable release if omitted), and the value you give each capability is your *deployment name*, not a generic model id — Azure resolves the underlying model server-side from that deployment.

## What each vendor actually supports

Real HTTP adapters exist for every vendor in the table above, but coverage isn't uniform — a vendor's own API surface decides what it can serve:

| Capability | Supported vendors |
|---|---|
| `chat`, `judge` | every vendor (`judge` routes through that vendor's own `chat` — it builds a rubric-evaluation prompt, sends it through `chat`, and parses the reply into a `Verdict`) |
| `embed` | `openai_compatible`, `gemini`, `cohere`, `ollama`, `azure_openai` |
| `vision` | `openai_compatible`, `anthropic`, `gemini`, `ollama`, `azure_openai` — image files (jpg/png/gif/webp); Anthropic also accepts a PDF, routed to a document content block instead of an image one |
| `transcribe` / `speak` / `generate_image` | `openai_compatible` only (OpenAI directly, or Groq for `transcribe`) |

A few sharp edges worth knowing before you rely on one:

- **Anthropic and Cohere have no `transcribe`/`speak`/`generate_image` API at all.** If you want those capabilities from a vendor other than OpenAI/Groq, you're out of luck until a future adapter covers it.
- **Gemini and Ollama only read local vision files**, never a remote URL — Gemini's URL-fetching path needs a separate File API upload flow that isn't wired up yet, and Ollama's native API has no URL-fetch concept at all. `openai_compatible`, Azure OpenAI, and Anthropic can fetch an `http(s)://`/`data:` reference directly.
- **`ArtifactType::Video` isn't implemented by any vendor** — passing a video artifact where a capability's declared `accepts` set doesn't cover it fails at the HTTP boundary with a plain error, not a compile-time rejection.
- **Refusal detection is vendor-specific.** A vendor's finish/stop-reason field maps to a `Draft<T>`'s `Refused` state; Cohere's Chat v2 API exposes no such signal, so a Cohere-backed call never produces `Refused`.

None of this is hidden behind a "full parity across vendors" claim — a capability a vendor genuinely can't serve is a `Draft<T>` you'll never see resolve, and an unsupported artifact/vendor combination is a clear error at the HTTP boundary rather than a silent no-op.

## Retries, circuit breaking, and unsettled drafts

Every real HTTP call goes through retry-with-backoff (exponential, with jitter, honoring a `Retry-After` header on 429) plus a per-provider circuit breaker that trips after repeated 5xx/transport failures. A rate limit, timeout, or safety refusal surfaces as an unsettled `Draft<T>` for your program's own `match` to handle — never a crash.

One deliberate asymmetry: `generate_image`/`speak` never retry on a client-side timeout specifically, unlike every other capability. The vendor may have already completed and billed for the image/audio even though the response didn't arrive back in time, so retrying risks paying for it twice.

Adding a new provider adapter needs no compiler, grammar, or IR change — see [`crates/ulx-runtime/src/provider/`](https://github.com/JGalego/ulexite/tree/main/crates/ulx-runtime/src/provider) if you're building one.

## Disambiguating multiple providers

If two registered providers serve the same capability and nothing at the call site disambiguates it, `ask` fails with a clear `Ambiguous` error rather than silently picking one. You disambiguate two ways:

- **Per call**, with a `provider:` argument naming a `.ulx` provider decl or a `ulexite.toml` table name:

  ```ulexite
  ask chat(provider: "anthropic") { user: """..." """ } -> reply: text
  ```

- **For the whole run**, with `--provider name` on the CLI (repeatable — only the named provider(s) get registered, so an otherwise-ambiguous capability resolves unambiguously):

  ```bash
  ulx run rag.ulx AnsweredByRAG --arg question="..." --provider openai --provider groq
  ```

## API keys via `.env`

`ulx run` loads a `.env` file next to the `.ulx` file, if one exists, before resolving providers — a real shell-exported variable always wins over one loaded from `.env`. See [`examples/.env.example`](https://github.com/JGalego/ulexite/tree/main/examples/.env.example):

```bash
export OPENAI_API_KEY=sk-...
export AZURE_OPENAI_API_KEY=...
export ANTHROPIC_API_KEY=sk-ant-...
export GEMINI_API_KEY=...
export GROQ_API_KEY=gsk_...
export COHERE_API_KEY=...
```

`api_key_env` only ever names an environment variable — never put a literal API key in `ulexite.toml` or a `.ulx` file.

## A complete reference manifest

[`examples/ulexite.example.toml`](https://github.com/JGalego/ulexite/tree/main/examples/ulexite.example.toml) lists every supported vendor with real capability mappings, meant as a reference to copy individual entries from rather than to use verbatim:

```toml
[package]
name    = "ulexite-examples"
version = "0.1.0"
ulexite = "^0.1"

[providers.openai]
vendor         = "openai"
api_key_env    = "OPENAI_API_KEY"
chat           = "gpt-4o-mini"
judge          = "gpt-4o-mini"
vision         = "gpt-4o-mini"
embed          = "text-embedding-3-small"
transcribe     = "whisper-1"
speak          = "tts-1"
generate_image = "gpt-image-1"

[providers.azure]
vendor      = "azure_openai"
base_url    = "https://my-resource.openai.azure.com"
api_key_env = "AZURE_OPENAI_API_KEY"
api_version = "2024-06-01"
chat        = "my-gpt4o-deployment"
judge       = "my-gpt4o-deployment"
vision      = "my-gpt4o-deployment"
embed       = "my-embedding-deployment"

[providers.anthropic]
vendor      = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
chat        = "claude-3-5-sonnet-20241022"
judge       = "claude-3-5-sonnet-20241022"
vision      = "claude-3-5-sonnet-20241022"

[providers.gemini]
vendor      = "gemini"
api_key_env = "GEMINI_API_KEY"
chat        = "gemini-1.5-flash"
judge       = "gemini-1.5-flash"
vision      = "gemini-1.5-flash"
embed       = "text-embedding-004"

[providers.groq]
vendor      = "groq"
api_key_env = "GROQ_API_KEY"
chat        = "llama-3.3-70b-versatile"
judge       = "llama-3.3-70b-versatile"
transcribe  = "whisper-large-v3"

[providers.cohere]
vendor      = "cohere"
api_key_env = "COHERE_API_KEY"
chat        = "command-r"
judge       = "command-r"
embed       = "embed-english-v3.0"

[providers.ollama]
vendor = "ollama"
chat   = "llama3"
judge  = "llama3"
vision = "llava"     # needs a multimodal model pulled locally
embed  = "nomic-embed-text"

[providers.local_llm]
vendor   = "openai_compatible"
base_url = "http://localhost:8000/v1"
chat     = "meta-llama/Llama-3-8b"
```

If more than one entry in a file like this declares the same capability, provider resolution just picks the first one registered (alphabetical by table name) and the rest sit unused — delete the entries you don't want before copying a reference file like this one into your own `ulexite.toml`.

## Trying it without spending anything

`ulx run`/`bench`/`plan`/`approve`/`deny` all accept `--mock`, which forces the deterministic, offline mock provider regardless of what's configured, and never makes a network call. It's the right default while you're learning the language or writing tests you don't want to spend API budget on:

```bash
ulx run translate.ulx Translate --arg source=hello --arg target_lang=fr --mock
```

For the full picture on inspecting what a run actually did — including which provider served which capability — see the [CLI Reference](./tooling/cli-reference.md)'s `--output` formats and `ulx trace`.
