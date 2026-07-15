# Examples

Every `.ulx` file here doubles as the canonical version of the matching
example in [`docs/spec/21-examples.md`](../docs/spec/21-examples.md), and
`ulx check`s cleanly with no configuration at all (`just check-examples`
does this for all of them; CI runs the same command). To actually run one
against a real vendor: export the API key(s) it needs (see each row below,
or [README.md](../README.md)'s "Configuring providers"), `cd examples`, and
run the command shown.

Several examples (`voice_memo.ulx`, `rag.ulx`, `summarize.ulx`,
`pdf_qa.ulx`, `generate_and_describe.ulx`) declare their own `provider {
... }` blocks directly in source and pin each capability to a specific
vendor with `provider: "name"` (§12.4) — no `ulexite.toml` needed for
those at all, just the relevant env var(s) exported. The rest resolve
providers from `ulexite.toml`; if you copy `ulexite.example.toml`
verbatim (several vendors there declare `chat`), pass `--provider <name>`
to disambiguate, as shown below.

| File | Entry-point conversation | Demonstrates | Spec § |
|---|---|---|---|
| [`translate.ulx`](translate.ulx) | `Translate(source, target_lang)` | Judge-checked retry loop with human escalation on repeated failure | [21.1](../docs/spec/21-examples.md#211-translation-with-retry-and-human-escalation) |
| [`summarize.ulx`](summarize.ulx) | `Summarize(doc: pdf)` | Parallel independent extraction via `with`; vision on Anthropic, chat on Groq | [21.2](../docs/spec/21-examples.md#212-summarization-with-parallel-independent-extraction-with) |
| [`pdf_qa.ulx`](pdf_qa.ulx) | `PdfQA(doc: pdf, question)` | OCR fallback: deterministic text extraction, else `vision` on page images; same Anthropic/Groq split as `summarize.ulx` | [21.3](../docs/spec/21-examples.md#213-ocr-and-pdf-question-answering) |
| [`rag.ulx`](rag.ulx) | `Caption(photo)`, `AnsweredByRAG(question)` | Captioning + RAG over a `dataset`; three capabilities pinned to three vendors (Anthropic vision, OpenAI embed, Groq chat) | [21.4](../docs/spec/21-examples.md#214-image-captioning--rag-over-a-document-set) |
| [`multi_agent.ulx`](multi_agent.ulx) | `ResearchReport(topic)` | Nested conversations handing off to each other, with a judge-gated rewrite retry | [21.5](../docs/spec/21-examples.md#215-multi-agent-workflow-nested-conversations-handoff) |
| [`batch.ulx`](batch.ulx) | `TriageBacklog()` | Sequential `for` loop over a `dataset` | [21.6](../docs/spec/21-examples.md#216-batch-execution-over-a-dataset-loops) |
| [`eval_translate.ulx`](eval_translate.ulx) | `benchmark TranslateQuality` | Evaluation: reuses `translate.ulx` against a golden dataset with a judge threshold and snapshots | [21.7](../docs/spec/21-examples.md#217-evaluation-benchmark-with-dataset-judge-and-snapshot) |
| [`approval.ulx`](approval.ulx) | `RefundRequest(order_id, amount)` | Suspend/resume: `escalate` as a human-approval checkpoint | [21.8](../docs/spec/21-examples.md#218-human-approval-as-a-suspendresume-checkpoint) |
| [`voice_memo.ulx`](voice_memo.ulx) | `VoiceMemoReply(recording: audio)` | `transcribe` → `chat` → `speak`, each pinned to a different provider (Groq, then OpenAI — Groq has no `speak`) | [21.10](../docs/spec/21-examples.md#2110-voice-memo-reply-transcribe--speak) |
| [`generate_and_describe.ulx`](generate_and_describe.ulx) | `GenerateAndDescribe(prompt)` | `generate_image` (OpenAI) output fed straight into `vision` (Anthropic) — a genuinely cross-vendor pipeline | [21.11](../docs/spec/21-examples.md#2111-generate-then-describe-what-was-generated-generate_image--vision) |
| [`custom_provider.ulx`](custom_provider.ulx) | `Greet(name)` | Declaring a `provider` directly in `.ulx` source, no `ulexite.toml` needed (deliberately `vendor: "mock"`, to show the mechanism itself) | [21.12](../docs/spec/21-examples.md#2112-declaring-a-provider-directly-in-ulx-source) |
| [`prompt_from_file.ulx`](prompt_from_file.ulx) | `Greet(name, occasion)` | Loading prompt text from disk (`file(...)` / `@path`) instead of inline `"""..."""` blocks | [`08-grammar.md`](../docs/spec/08-grammar.md) `file_expr` |

## Running one

Self-contained (own `provider` decls — just export the env var(s) named in
their header comment):

```sh
cd examples
export ANTHROPIC_API_KEY=sk-ant-... GROQ_API_KEY=gsk_...
ulx run summarize.ulx Summarize --arg doc=fixtures/sample.pdf
```

```sh
export ANTHROPIC_API_KEY=sk-ant-... OPENAI_API_KEY=sk-... GROQ_API_KEY=gsk_...
ulx run rag.ulx Caption --arg photo=fixtures/sample.jpg
ulx run rag.ulx AnsweredByRAG --arg question="What is the PTO policy?"
```

```sh
export GROQ_API_KEY=gsk_... OPENAI_API_KEY=sk-...
ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav
```

```sh
export OPENAI_API_KEY=sk-... ANTHROPIC_API_KEY=sk-ant-...
ulx run generate_and_describe.ulx GenerateAndDescribe --arg prompt="a lighthouse at sunset"
```

Everything else resolves through `ulexite.toml` (copy
`ulexite.example.toml`, fill in the vendors you want, delete the rest —
its own header explains why). With more than one vendor declaring `chat`,
pass `--provider <name>` to disambiguate:

```sh
ulx run translate.ulx Translate --arg source=hello --arg target_lang=fr --provider anthropic
ulx run multi_agent.ulx ResearchReport --arg topic="the history of lighthouses" --provider anthropic
ulx run batch.ulx TriageBacklog --provider anthropic
ulx run approval.ulx RefundRequest --arg order_id=X123 --arg amount=42.50 --provider anthropic
ulx run prompt_from_file.ulx Greet --arg name=Ada --arg occasion=birthday --provider anthropic
ulx bench eval_translate.ulx TranslateQuality --provider anthropic
```

`translate.ulx`/`multi_agent.ulx` may print `suspended: waiting on
'human_approval'` if the judge escalates — resume with `ulx approve
<run_id> --value "..."` (printed in the output). `judge`/`validator`
calls can't be pinned per-call the way `ask` can (§12.4 — their args are
consumed entirely into rubric parameters), so pick one `--provider` that
supports both `chat` and `judge` rather than trying to pin them
separately. `ulx bench` also doesn't resume a mid-benchmark escalation
today — a row that escalates fails the whole run rather than suspending
it (§16, narrower-than-spec scope).

`custom_provider.ulx` needs no export at all — it deliberately runs
against its own inline mock-vendor provider to demonstrate the `provider {
... }` declaration mechanism itself, not a real vendor call.

## Demos

Recorded with [VHS](https://github.com/charmbracelet/vhs) (tape scripts in
[`demos/`](demos)) against real vendors — genuine output, not staged, so
exact wording will differ if you regenerate them (`vhs demos/<name>.tape`
from the repo root). Two are honest, not flattering: `pdf_qa.ulx` shows
today's `pdf.extract_text` placeholder limitation, and `eval_translate.ulx`
shows `ulx bench` failing outright on a mid-run judge escalation rather
than suspending — both noted above and left in on purpose.

<details>
<summary><code>translate.ulx</code> — judge-checked retry with human escalation</summary>

![translate.ulx demo](demos/translate.gif)
</details>

<details>
<summary><code>summarize.ulx</code> — parallel <code>with</code>-block extraction, vision on Anthropic + chat on Groq</summary>

![summarize.ulx demo](demos/summarize.gif)
</details>

<details>
<summary><code>pdf_qa.ulx</code> — OCR fallback (shows the current pdf.extract_text placeholder limitation)</summary>

![pdf_qa.ulx demo](demos/pdf_qa.gif)
</details>

<details>
<summary><code>rag.ulx</code> — captioning + RAG, three capabilities across three vendors</summary>

![rag.ulx demo](demos/rag.gif)
</details>

<details>
<summary><code>multi_agent.ulx</code> — nested conversations handing off, judge-gated</summary>

![multi_agent.ulx demo](demos/multi_agent.gif)
</details>

<details>
<summary><code>batch.ulx</code> — sequential <code>for</code> loop over a dataset</summary>

![batch.ulx demo](demos/batch.gif)
</details>

<details>
<summary><code>eval_translate.ulx</code> — benchmark (shows <code>ulx bench</code> failing on a mid-run judge escalation)</summary>

![eval_translate.ulx demo](demos/eval_translate.gif)
</details>

<details>
<summary><code>approval.ulx</code> — suspend/resume checkpoint</summary>

![approval.ulx demo](demos/approval.gif)
</details>

<details>
<summary><code>voice_memo.ulx</code> — transcribe → chat → speak across Groq + OpenAI</summary>

![voice_memo.ulx demo](demos/voice_memo.gif)
</details>

<details>
<summary><code>generate_and_describe.ulx</code> — generate_image (OpenAI) → vision (Anthropic), cross-vendor</summary>

![generate_and_describe.ulx demo](demos/generate_and_describe.gif)
</details>

<details>
<summary><code>custom_provider.ulx</code> — declaring a provider directly in source</summary>

![custom_provider.ulx demo](demos/custom_provider.gif)
</details>

<details>
<summary><code>prompt_from_file.ulx</code> — prompt text loaded from disk</summary>

![prompt_from_file.ulx demo](demos/prompt_from_file.gif)
</details>

## Supporting data

- [`fixtures/`](fixtures) — sample inputs (`sample.pdf`, `sample.jpg`, `sample.wav`, `translations.jsonl`) used by the examples and by `eval_translate.ulx`'s dataset.
- [`kb/`](kb) — `chunks.jsonl`, the knowledge base `rag.ulx` queries.
- [`tickets/`](tickets) — `backlog.jsonl`, the dataset `batch.ulx` iterates over.
- [`prompts/`](prompts) — the on-disk prompt files `prompt_from_file.ulx` loads.
- [`demos/`](demos) — the VHS `.tape` scripts that recorded the GIFs above.
