# Examples

Every `.ulx` file here runs standalone against the built-in mock provider (no
API key needed) and doubles as the canonical version of the matching example
in [`docs/spec/21-examples.md`](../docs/spec/21-examples.md). `just
check-examples` type-checks all of them; CI runs the same command.

To try one against a real vendor instead of the mock, see the root
[README.md](../README.md)'s "Configuring providers": copy
`ulexite.example.toml` to `ulexite.toml` and `.env.example` to `.env` in this
directory, fill in the key(s) for whichever vendor you enabled, then drop
`--mock` from the command.

| File | Entry-point conversation | Demonstrates | Spec § |
|---|---|---|---|
| [`translate.ulx`](translate.ulx) | `Translate(source, target_lang)` | Judge-checked retry loop with human escalation on repeated failure | [21.1](../docs/spec/21-examples.md#211-translation-with-retry-and-human-escalation) |
| [`summarize.ulx`](summarize.ulx) | `Summarize(doc: pdf)` | Parallel independent extraction via `with` | [21.2](../docs/spec/21-examples.md#212-summarization-with-parallel-independent-extraction-with) |
| [`pdf_qa.ulx`](pdf_qa.ulx) | `PdfQA(doc: pdf, question)` | OCR fallback: deterministic text extraction, else `vision` on page images | [21.3](../docs/spec/21-examples.md#213-ocr-and-pdf-question-answering) |
| [`rag.ulx`](rag.ulx) | `Caption(photo)`, `AnsweredByRAG(question)` | Image captioning + retrieval-augmented generation over a `dataset` | [21.4](../docs/spec/21-examples.md#214-image-captioning--rag-over-a-document-set) |
| [`multi_agent.ulx`](multi_agent.ulx) | `ResearchReport(topic)` | Nested conversations handing off to each other, with a judge-gated rewrite retry | [21.5](../docs/spec/21-examples.md#215-multi-agent-workflow-nested-conversations-handoff) |
| [`batch.ulx`](batch.ulx) | `TriageBacklog()` | Sequential `for` loop over a `dataset` | [21.6](../docs/spec/21-examples.md#216-batch-execution-over-a-dataset-loops) |
| [`eval_translate.ulx`](eval_translate.ulx) | `benchmark TranslateQuality` | Evaluation: reuses `translate.ulx` against a golden dataset with a judge threshold and snapshots | [21.7](../docs/spec/21-examples.md#217-evaluation-benchmark-with-dataset-judge-and-snapshot) |
| [`approval.ulx`](approval.ulx) | `RefundRequest(order_id, amount)` | Suspend/resume: `escalate` as a human-approval checkpoint | [21.8](../docs/spec/21-examples.md#218-human-approval-as-a-suspendresume-checkpoint) |
| [`voice_memo.ulx`](voice_memo.ulx) | `VoiceMemoReply(recording: audio)` | `transcribe` → `chat` → `speak` pipeline, each pinned to a different provider (Groq, then OpenAI) via `provider: "name"` (§12.4) | [21.10](../docs/spec/21-examples.md#2110-voice-memo-reply-transcribe--speak) |
| [`generate_and_describe.ulx`](generate_and_describe.ulx) | `GenerateAndDescribe(prompt)` | `generate_image` output fed straight back in as `vision` input | [21.11](../docs/spec/21-examples.md#2111-generate-then-describe-what-was-generated-generate_image--vision) |
| [`custom_provider.ulx`](custom_provider.ulx) | `Greet(name)` | Declaring a `provider` directly in `.ulx` source — no `ulexite.toml` needed | [21.12](../docs/spec/21-examples.md#2112-declaring-a-provider-directly-in-ulx-source) |
| [`prompt_from_file.ulx`](prompt_from_file.ulx) | `Greet(name, occasion)` | Loading prompt text from disk (`file(...)` / `@path`) instead of inline `"""..."""` blocks | [`08-grammar.md`](../docs/spec/08-grammar.md) `file_expr` |

## Running one

```sh
cd examples
ulx run summarize.ulx Summarize --arg doc=fixtures/sample.pdf --mock
```

Swap the conversation name and `--arg`s per the table above. A few examples
have their own real-vendor walkthrough in a header comment (`voice_memo.ulx`,
`generate_and_describe.ulx`); the rest just need `--mock` dropped once a
provider is configured.

## Supporting data

- [`fixtures/`](fixtures) — sample inputs (`sample.pdf`, `sample.jpg`, `sample.wav`, `translations.jsonl`) used by the examples and by `eval_translate.ulx`'s dataset.
- [`kb/`](kb) — `chunks.jsonl`, the knowledge base `rag.ulx` queries.
- [`tickets/`](tickets) — `backlog.jsonl`, the dataset `batch.ulx` iterates over.
- [`prompts/`](prompts) — the on-disk prompt files `prompt_from_file.ulx` loads.
