---
title: Standard Library
description: The stdlib modules Ulexite ships or plans — capability kinds, judges, vision/audio/pdf helpers, structural utilities, and what's actually implemented today.
---

# Standard Library

The standard library is where technique that keeps evolving lives — separately from the language grammar. DSPy-style prompt optimization, RAG retrieval recipes, and provider-specific quirks are stdlib concerns, versioned and upgradable independently of the compiler, so an improvement to *how* you retrieve or optimize lands as a library version bump, not a language break.

This page surveys every module the design calls for. Read the callouts carefully: **a small, real slice of this is implemented today** — `pdf`, `embedding`, and `vector` have working code behind them (with one important caveat on `pdf`), and everything else described below is the intended design, not yet wired up to the runtime. Where a module isn't implemented, the underlying capability is usually still reachable a different way — through an explicit `ask <capability>(...)` call rather than a stdlib helper function — and this page says so each time.

## What's real right now

Import a module with `import "name" as name`, then call `module.function(...)`. As of today, the runtime's stdlib dispatcher implements exactly five functions:

| Call | What it does |
|---|---|
| `pdf.extract_text(doc)` | Returns a **canned placeholder string**, not real text extraction — see the caveat below |
| `pdf.to_images(doc)` | Returns a **canned placeholder image**, not real page rasterization — same caveat |
| `embedding.of(text, model: capability(embed), provider: "...")` | A real call: resolves an `embed`-capable provider and returns a live embedding vector |
| `vector.cosine_similarity(a, b)` | A real, deterministic similarity computation over two embedding lists |
| `vector.nearest(query: ..., index: ..., k: ...)` | A real, deterministic top-k search over a dataset of `{..., embedding: ...}` rows |

Everything else named in the sections below — every `judge.*` helper, `vision.*`/`image.*`, `audio.*`/`video.*`, `json`/`xml`/`html`/`csv`, `http`, `trace.*`, most of `dataset.*`, `cache.*`, `retry.*`, the `python`/`javascript`/`shell` FFI, `optimize.*`, `metrics.*`/`assert.*` helpers, and `llm.pin`/`cheapest`/`fastest` — calls into the stdlib dispatcher and gets a clear "not implemented" error naming the exact call, rather than silently doing nothing. None of this is hidden: the runtime's own source comments point at this gap directly.

**The `pdf` caveat, concretely**: [`examples/pdf_qa.ulx`](https://github.com/JGalego/ulexite/tree/main/examples/pdf_qa.ulx) calls `pdf.extract_text(doc)` and `pdf.to_images(doc)`, and both always return the same placeholder string/image regardless of what `doc` actually contains. That means a branch like `if text_layer.length > 0 { text_layer } else { ask vision(page_images) {...} }` always takes the "real text layer" branch in practice — the vision-OCR fallback is declared and pinned in the example for when real extraction lands, but never actually executes today.

## `llm` — capability declarations and chat

Defines the capability kinds `ask` uses: `chat`, `vision`, `embed`, `transcribe`, `speak`, `generate_image`. These are real and working — you reach them with `ask chat(...)`, `ask vision(...)`, and so on, resolved against your configured providers (see [Providers](./providers.md)). The module is also meant to expose policy helpers usable in a `model:` argument — `llm.pin(provider_id)`, `llm.cheapest()`, `llm.fastest()` — describing which provider to route a call to; these policy expressions aren't implemented yet, so `model:` today accepts a pinned model reference but not a cost/latency policy function.

## `judge` — reusable rubric-based verdicts

Beyond a `judge` you declare yourself (see [Testing and Evaluation](./testing-and-evaluation.md)), this module is meant to ship common rubric patterns as parametrizable judges: `judge.factuality(subject, reference)`, `judge.toxicity(subject)`, `judge.rubric_match(subject, criteria: list<text>)`, and `judge.pairwise(a, b, criteria)` for A/B comparison. It also plans `judge.meta(judge_under_test, human_labels: dataset)` for calibrating a judge against human-labeled ground truth. None of these are implemented today — every real example declares its own `judge` with an explicit rubric (see `examples/translate.ulx`'s `judge Fluency`) rather than calling a stdlib helper.

## `vision` / `image`

The design splits "calls a model" from "pure transformation": `vision.caption(image)`, `vision.ocr(image) -> text`, `vision.detect_objects(image) -> json` would wrap the `vision` capability's common recipes, while `image.resize`/`image.crop`/`image.to_mime` would be deterministic, non-model utilities. Neither half is implemented — today you get vision by calling `ask vision(image) { user: """..." """ }` directly, as `examples/rag.ulx`'s `Caption` conversation does, and there's no deterministic image-manipulation utility available at all.

## `audio` / `video`

Planned: `audio.transcribe(audio) -> text`, `audio.synthesize(text, voice: text) -> audio`, `video.extract_frames(video, every: duration) -> list<image>`, `video.caption(video) -> text` (a genuine multimodal call, not frame-by-frame image captioning glued together by hand). None of this is implemented. What's real today is the underlying capability calls: `ask transcribe(...)` and `ask speak(...)` work end to end against `openai_compatible` providers (OpenAI directly, or Groq for `transcribe`) — see `examples/voice_memo.ulx`. There's no video capability or adapter at all yet; `ArtifactType::Video` isn't implemented by any provider.

## `pdf`

Planned: `pdf.extract_text(pdf) -> text`, `pdf.extract_tables(pdf) -> list<csv>`, `pdf.to_images(pdf) -> list<image>` for page rasterization ahead of a vision-capability QA pass over scanned documents. `extract_text` and `to_images` exist as stdlib calls today, but — as described above — both return a fixed placeholder rather than real extraction/rasterization. `extract_tables` isn't implemented at all.

## `json` / `xml` / `html` / `csv`

Planned structural utilities and schema-validation entry points for `validator` declarations: `json.validate(subject, schema)`, `json.extract(subject, path)`, `html.to_markdown(html)`, `csv.parse(text, columns: record_type)`. None of these are implemented yet. A `validator` declaration's `json_schema:`/`regex:`/`ast:` forms are grammar-level constructs independent of this module; consult the [Syntax](./language/syntax.md) page for what a `validator` can express today without leaning on stdlib helpers that don't exist.

## `http`

The design calls for one deliberately narrow general-purpose networking primitive — `http.get`/`http.post` returning `json`/`text`/`raw` artifacts — for tool implementations that call ordinary REST APIs rather than model providers. There's no `http` keyword in the grammar by design (it isn't domain-specific to conversations), and the stdlib functions themselves aren't implemented yet either.

## `trace`

Planned: query/inspect the trace log from within a program or a script — `trace.of(conversation_run_id)`, `trace.diff(run_a, run_b)` for a semantic diff of two runs against the same conversation. Not implemented as a stdlib call. The CLI equivalent that *is* real today is `ulx trace <run_id>`, which prints a completed run's trace log directly — see the [CLI Reference](./tooling/cli-reference.md).

## `dataset`

A `dataset` declaration and its `from "path"` loader sugar are real and load JSONL rows today (see `examples/eval_translate.ulx`'s `TranslationPairs`). Beyond that sugar, the module also plans `dataset.from_csv`, `dataset.from_jsonl`, `dataset.sample(d, n)`, and `dataset.split(d, ratio)` for train/eval-style splitting — none of these extra loader/sampling functions are implemented yet.

## `cache`

Caching itself needs no explicit stdlib call — every `ask`/`judge` call is content-addressed and cached by default (`ulx run --no-cache` skips the cache read for a single invocation). The module additionally plans explicit cache-control functions, `cache.invalidate(capability, ...)` and `cache.stats()`, mostly as a debugging/ops surface. Neither is implemented.

## `retry`

`retry(n) { ... } else <fallback>` is a real grammar construct (see [Syntax](./language/syntax.md#retry-and-escalate)) that works today. The module additionally plans policy constructors — `retry.exponential(base: duration, max: int)`, `retry.fixed(delay: duration, max: int)` — for more elaborate retry policies than a bare `retry(n)`. These constructors aren't implemented; `retry(n)` today just takes a bounded count.

## `python` / `javascript` / `shell` — deterministic FFI

The design's explicit, visible escape hatch: `python.call(module_path, fn_name, args) -> T`, similarly for `javascript.call`/`shell.run` — the only places a non-Ulexite runtime would execute, always synchronous and deterministic by caller contract. None of these are implemented yet. A `validator` declaration's grammar has room for a `python:`/`shell:` form conceptually, but there's no working FFI bridge behind it today.

## Optimization recipes (DSPy-adjacent)

Planned: `optimize.bootstrap_demos(conversation, dataset, metric)` and `optimize.mipro(conversation, dataset, metric)`, wrapping DSPy-style automatic few-shot/instruction optimization as ordinary stdlib functions over Ulexite's typed `conversation` values and `dataset`s — producing a new, versioned `conversation` rather than mutating the original. Not implemented.

## `metrics` / `assert`

`assert` itself is a real grammar-level keyword inside a `benchmark` body (see [Testing and Evaluation](./testing-and-evaluation.md)) — an ordinary boolean check, working today. The `metrics` module (`metrics.mean`, `metrics.pass_rate`, `metrics.percentile`) and additional `assert.*` matcher helpers (`assert.semantically_equal(a, b, threshold: float)`, `assert.contains_claims(text, claims: list<text>)`) are planned but not implemented — `ulx bench`'s report today is a plain per-row pass/fail with no aggregation helpers available inside the benchmark body itself.

## `vector` / `embedding`

The one module with the most real coverage: `embedding.of` and `vector.cosine_similarity`/`vector.nearest` all work today and are exercised end to end by `examples/rag.ulx`. The design additionally frames this as deliberately minimal — not a full vector-database client library — with a production vector store meant to arrive as a provider plugin satisfying a `vector_index` capability the same way a model provider does. That plugin surface doesn't exist yet; `vector.nearest` today does an in-memory linear scan over a `dataset`'s rows, not a call to an external index.

## Module governance

Every stdlib module is meant to be versioned independently of the compiler, and any module may in principle be replaced by a community package satisfying the same capability/trait surface — the stdlib is privileged by being bundled and documented by default, not by being uneditable. That governance model is aspirational alongside the package registry described in [Package System](./package-system.md); today, the stdlib is exactly the fixed dispatcher shipped inside `ulx-runtime` described above, not a swappable/publishable set of packages.

For the full design rationale, see [§15 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/15-standard-library.md).
