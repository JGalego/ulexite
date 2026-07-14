# 15. Standard Library

The standard library is where technique that will keep evolving lives — per §3.4, this is deliberate: DSPy-style prompt optimization, RAG retrieval recipes, and provider-specific quirks are stdlib concerns, versioned and upgradable independently of the language grammar/IR (§13.4), so that the kind of improvement that forced LangChain/LlamaIndex/Semantic Kernel's repeated breaking rewrites (§2.8) lands as a library version bump against a stable compiler, never a language break.

## 15.1 `llm` — capability declarations and chat

Defines the capability trait (§9.6, §12.4) and the built-in `chat`, `vision`, `embed`, `transcribe`, `speak`, `generate_image` capability kinds referenced by `ask` (§7.5). Also provides `llm.pin(provider_id)`, `llm.cheapest()`, `llm.fastest()` — the policy expressions usable in `model:` (§7.5).

## 15.2 `judge` — reusable rubric-based verdicts

Beyond user-declared `judge`s (§7.2), this module ships common rubric patterns as parametrizable judges: `judge.factuality(subject, reference)`, `judge.toxicity(subject)`, `judge.rubric_match(subject, criteria: list<text>)`, and `judge.pairwise(a, b, criteria)` (A/B comparison, useful for benchmark-style regression checks, §16). Includes `judge.meta(judge_under_test, human_labels: dataset)`, adapting OpenAI Evals' meta-eval pattern (§2.2) as a standard library function rather than a bespoke harness.

## 15.3 `vision` / `image`

`vision` wraps the `vision` capability's common recipes: `vision.caption(image)`, `vision.ocr(image) -> text`, `vision.detect_objects(image) -> json`. `image` provides deterministic (non-model) image utilities: `image.resize`, `image.crop`, `image.to_mime(image, mime)` — a clean split between "calls a model" (`vision`) and "pure transformation" (`image`), reflecting §9.3's non-determinism-is-a-type principle at the module-boundary level too.

## 15.4 `audio` / `video`

`audio.transcribe(audio) -> text`, `audio.synthesize(text, voice: text) -> audio`. `video.extract_frames(video, every: duration) -> list<image>`, `video.caption(video) -> text` (a genuine multimodal capability call, not frame-by-frame image captioning glued together by hand — closing the video-handling gap documented as thin-to-absent across every framework in §2.3).

## 15.5 `pdf`

`pdf.extract_text(pdf) -> text`, `pdf.extract_tables(pdf) -> list<csv>`, `pdf.to_images(pdf) -> list<image>` (page rasterization, for vision-capability QA over scanned documents, §21.3) — extraction is always an explicit call (§11.4), never an implicit coercion.

## 15.6 `json` / `xml` / `html` / `csv`

Structural utilities plus schema-validation entry points consumed by `validator` declarations (§7.2, §9.2): `json.validate(subject, schema)`, `json.extract(subject, path)`, `html.to_markdown(html)`, `csv.parse(text, columns: record_type)`.

## 15.7 `http`

The one general-purpose networking primitive, deliberately narrow: `http.get`/`http.post` returning `json`/`text`/`raw` artifacts, used for tool implementations (§12.6) that call ordinary REST APIs rather than model providers — kept out of the language grammar entirely (there is no `http` keyword) precisely because it is not domain-specific to conversations, satisfying §1.3's non-goal of not becoming a general-purpose language.

## 15.8 `trace`

Query/inspect the trace log (§12.5, §18) from within a program or a script: `trace.of(conversation_run_id)`, `trace.diff(run_a, run_b)` (semantic diff of two runs against the same conversation, useful in `benchmark`s comparing a change's effect across a whole dataset, §16.6).

## 15.9 `dataset`

Loaders/writers beyond the `from "path"` sugar in §7.2: `dataset.from_csv`, `dataset.from_jsonl`, `dataset.sample(d, n)`, `dataset.split(d, ratio)` (train/eval-style splitting for optimizer workflows, §15.14).

## 15.10 `cache`

Explicit cache control beyond the default-on behavior in §10.3: `cache.invalidate(capability, ...)`, `cache.stats()` — mostly a debugging/ops surface, since caching itself needs no explicit stdlib call in ordinary programs.

## 15.11 `retry`

Policy constructors used by `retry(...)`/`supervise` (§7.3, §10.7): `retry.exponential(base: duration, max: int)`, `retry.fixed(delay: duration, max: int)`.

## 15.12 `python` / `javascript` / `shell` — deterministic FFI validators

The explicit, visible escape hatch referenced throughout (§4.6, §5.7, §9.1): `python.call(module_path, fn_name, args) -> T` (T declared at the call site and checked as a contract, not inferred), similarly for `javascript.call`/`shell.run`. These are the only places a non-Ulexite runtime executes, and they are always synchronous, deterministic-by-caller-contract calls — the runtime does not memoize them specially (§10.3's cache applies uniformly), but the type checker cannot verify their internal purity, so a validator built this way is trusted, not proven, and the language server flags it as such (§20.7).

## 15.13 (reserved — merged into §15.12 per FFI unification)

## 15.14 Optimization recipes (DSPy-adjacent)

`optimize.bootstrap_demos(conversation, dataset, metric)` and `optimize.mipro(conversation, dataset, metric)` wrap DSPy-style automatic few-shot/instruction optimization (§2.1) as ordinary stdlib functions operating over Ulexite's typed `conversation` values and `dataset`s — producing a new, versioned `conversation` value (§14.4) rather than mutating the original, so an optimized variant is reviewable, testable (§16), and independently publishable, unlike DSPy's own optimizer output, which practitioners report is hard to inspect/extract (§2.1, dspy#8042).

## 15.15 `metrics` / `assert`

`metrics` provides aggregation functions used inside `benchmark` reporting (§16.6): `metrics.mean`, `metrics.pass_rate`, `metrics.percentile`. `assert` is the grammar-level keyword (§7.6, §8); this module supplies additional matcher-style helpers callable from within an `assert` expression: `assert.semantically_equal(a, b, threshold: float)`, `assert.contains_claims(text, claims: list<text>)` — closing exactly the "no custom-matcher plugin API" gap documented against Promptfoo (§2.2).

## 15.16 `vector` / `embedding`

`embedding.of(artifact, model: capability) -> embedding<N>`, `vector.cosine_similarity`, `vector.nearest(query: embedding<N>, index: vector_index, k: int)` — the minimal RAG-retrieval primitives (§21.4's example), deliberately not a full vector-database client library; a production vector store is a provider plugin (§12.4) satisfying a `vector_index` capability, the same way a model provider is a plugin.

## 15.17 Module governance

Every stdlib module is versioned independently (§14.4) and any module may in principle be replaced by a community package satisfying the same capability/trait surface — the stdlib is privileged by being bundled and documented by default, not by being uneditable, avoiding the fate of frameworks in §2.3 whose "core" abstractions (SK's `Kernel`, LlamaIndex's `ServiceContext`) turned out not to be stable footing after all.
