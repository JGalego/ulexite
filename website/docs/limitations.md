---
title: Known Limitations
description: What doesn't work yet, and what to watch out for, before adopting Ulexite.
---

# Known Limitations

Ulexite is a new language. This page lists what it doesn't do yet, where its design trades expressiveness for guarantees, and where you should watch out for rough edges — the facts a prospective user would want before adopting it, not a justification for why each trade-off was made.

## No production track record

Ulexite's checkpoint/replay design is unvalidated against the failure modes that only surface at scale in production — a partial write during a crash mid-checkpoint, clock skew across a distributed provider registry, an adversarial trace-log tampering attempt. Systems like LangGraph's checkpointer have years of production edge cases behind them; Ulexite has none yet. Treat it as a new project, not a battle-tested one.

## `with`-block parallelism has an expressiveness ceiling

The rule that a `with` block's branches can't reference each other's results is a parser-enforced, sound guarantee — but it's strictly less expressive than the inferred-dependency graphs of systems like Pulumi or Beam. A pattern like "three retrieval calls that could theoretically run in parallel, but the second wants to see the first's result to decide whether to bother" can't be expressed inside one `with` block at all; it has to be written sequentially, giving up the parallelism you might have wanted. This is a deliberate trade, but it's a real ceiling, not a solved problem. (A more permissive, effect-tracked parallelism model is on the [roadmap](./roadmap.md).)

## The generics system is thin

Ulexite's generic vocabulary (`Draft<T>`, `dataset<Row>`, `list<T>`) is small and closed. It can't express a user-defined generic container over artifacts with its own merge semantics, or higher-kinded abstractions like "any capability that produces a T." If you need that, you drop to the standard library's Rust implementation layer rather than expressing it in Ulexite itself.

## Capability negotiation can't catch everything

Ulexite's `structured_output: guaranteed | negotiated | unsupported` tiering is a real improvement over silent runtime failure, but it can only reflect a capability difference the provider plugin author *correctly declares*. It can't detect an undeclared gap — a provider that claims `guaranteed` but is subtly wrong under some input shape. This converts most of the "provider-agnostic, but leaky" problem into something compile-time-checkable, but it's a mitigation, not an elimination — it still depends on honest, well-tested plugin authors.

## Non-determinism is typed, not eliminated

`Draft<T>` makes non-determinism visible and forces you to handle it exhaustively — it does not make an LLM's output correct or stable. Two runs against the same prompt with caching off can still legitimately produce different `Settled(T)` values that both satisfy the same type. The type system disciplines how a program *reacts* to non-determinism; it doesn't and can't reduce the model's actual sampling variance. Judges mitigate this at the evaluation layer, not the type layer, and judges themselves are probabilistic instruments that need real calibration discipline to be trustworthy — a discipline the language encourages but can't force.

## The IR interpreter has a performance ceiling for compute-heavy programs

Ulexite interprets its IR rather than natively compiling it, on the bet that network-bound latency dominates interpretation overhead for typical conversation-orchestration workloads. For a program with a large, mostly-pure computational core — heavy client-side artifact post-processing, large-scale embedding math done in-language rather than via a provider capability — that bet doesn't hold, and interpretation overhead becomes a real, measurable cost. The workaround is architectural: push genuinely heavy computation into a `python`/`shell` FFI call or a provider/tool plugin written in Rust, rather than expressing it as Ulexite IR.

## The declarative/imperative split is a design bet

Ulexite splits programs into a provably-independent declarative region (`with` blocks) and an imperative region for everything else. It's possible real-world usage reveals a large class of programs that are "almost" parallelizable but don't fit `with`'s strict independence rule often enough that the ergonomic cost outweighs the soundness benefit. If so, a future revision may need a more permissive model instead of the current syntactic one.

## Tooling and ecosystem debt is real

Ulexite's package ecosystem is rated "Low (new)" against LangChain's or LlamaIndex's very large integration catalogues, honestly. Every provider, vector store, and tool integration those ecosystems have accumulated over years has to be either reimplemented as an Ulexite plugin or wrapped via FFI before parity is reached. That's real work, not a solved problem, and some long tail of niche integrations may never justify a native port.

## Adopting a new language has a real cost

Every team adopting Ulexite has to learn a new grammar, a new type system's vocabulary (`Draft<T>`, `Verdict`, capability negotiation), and a new toolchain. Even with migration paths designed to be incremental, this is a strictly higher up-front cost than adding one more Python import to an existing codebase — one that's only worth paying for teams whose conversation-orchestration surface is large or critical enough for the static guarantees to pay for themselves.

## Provider coverage has real gaps

The shipped provider adapters cover `chat`/`judge` across every supported vendor (OpenAI-compatible servers, Azure OpenAI, Anthropic, Gemini, Cohere, Ollama), and `embed`/`vision` across most of them — but:

- **There's no general artifact/blob store yet.** A file path passed via `--arg` is read directly off disk by the provider adapter at the HTTP-call boundary, not managed by a pluggable content-addressed store. `speak`/`generate_image` output is written to a hash-named temp file, which is close to but not the same as the pluggable store the design eventually calls for.
- **`vision` is images-plus-Anthropic-PDF, not full PDF/video support.** jpg/png/gif/webp work broadly; Anthropic additionally accepts PDFs, routed to a document content block. Every other vendor rejects `.pdf` outright, and video artifacts aren't implemented anywhere.
- **`transcribe`/`speak`/`generate_image` are OpenAI-compatible-only** (OpenAI itself, or Groq for `transcribe`). Anthropic and Cohere don't expose these APIs at all; adapters for Gemini's, Azure's, or a native Ollama server's equivalents don't exist yet.
- **Retry/circuit-breaking is real but simple** — exponential backoff with jitter and a per-provider circuit breaker, but no per-error-class tuning and no shared breaker state across processes.
- **Refusal detection is vendor-specific and not exhaustive** — Cohere's Chat v2 API exposes no refusal signal at all, so it never produces a `Refused` draft.

## Some provider-declaration edge cases are unpolished

- A `judge`/`validator` call can't carry a per-call provider override the way an `ask` call can — an ambiguous `judge` capability can only be disambiguated globally, via `--provider` on the CLI.
- `.ulx` provider capability params can't express a boolean literal at all (the grammar has none); the manifest's TOML `params` table can, since TOML has real booleans.
- `ulx check`'s validation of `from`/`provider:` references is best-effort — it depends on a `ulexite.toml` being discoverable next to the file. A clean `ulx check` is not a guarantee that a later `ulx run` won't fail on an unresolvable provider reference.
- Every declared provider is constructed eagerly, whether or not the conversation you're actually running ever asks for it — a broken provider declaration can fail a run even if that specific conversation never references it.

## The compiler front end has known gaps (pre-v0.1)

A pre-v0.1 review surfaced several compiler-quality issues worth knowing about:

- Text-block interpolation splitting isn't brace/string-aware, so a record or string literal containing `}` inside an interpolation can produce a spurious parse error.
- `///` doc comments are discarded identically to `//` — there's a grammar production for them, but nothing in the compiler recovers doc text yet.
- `Verdict` exhaustiveness checking matches the literal type name `"Verdict"`, not a resolved type; general user-declared union exhaustiveness checking isn't implemented at all.

## IDE support is early

`ulx-lsp` implements four LSP capabilities today: hover, go-to-definition, document symbols, and completion. It resyncs the whole document on every edit rather than doing incremental analysis, and it has no code lens, no artifact-content hover previews, no live-run attachment, no trace-viewer webview, and no lint warnings beyond hard compile errors. `ulx doc` and a REPL don't exist yet either. The VS Code extension does correctly launch `ulx-lsp`, so what's implemented is real and usable — it's just a small subset of where the tooling is eventually headed. See the [roadmap](./roadmap.md) for what's planned.

For the full design rationale, see [§24 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/24-limitations.md).
