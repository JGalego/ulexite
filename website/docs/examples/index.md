---
title: Examples
description: Twelve complete, runnable Ulexite programs, one concept at a time, with recorded terminal demos.
---

# Examples

Every program below ships in [`examples/`](https://github.com/JGalego/ulexite/tree/main/examples) in the repo, `ulx check`s cleanly with no configuration at all, and is replayed offline end-to-end by the project's own test suite. Try any of them in the [Playground](/playground) first — paste the code in and watch the diagnostics — then run the real thing once you have a provider configured (see [Providers](../providers.md)).

Several examples declare their own `provider { ... }` blocks directly in source and pin capabilities to specific vendors — no `ulexite.toml` needed for those, just the env var(s) named in each section. The rest resolve providers from `ulexite.toml`; if more than one configured vendor serves the same capability, pass `--provider <name>` to disambiguate.

The demos below were recorded with [VHS](https://github.com/charmbracelet/vhs) against real vendors — genuine output, not staged. Two recordings now predate a real fix and haven't been re-recorded yet: `pdf_qa.ulx`'s predates `pdf.extract_text` becoming real (it was a canned placeholder when recorded), and `eval_translate.ulx`'s predates `ulx bench` learning to suspend gracefully on a mid-run escalation instead of aborting the whole run. The source below is current for both; only the GIFs are stale.

## `translate.ulx` — judge-checked retry with human escalation

The canonical "hello world": translate, have a judge check fluency, retry once on failure, escalate to a human if the judge can't decide.

```ulexite
judge Fluency(subject: text) -> Verdict {
  rubric: """Is this an accurate, fluent translation of the source? Answer Pass, Fail(reason), or Escalate if you cannot tell."""
}

conversation Translate(source: text, target_lang: text) -> text {
  system: """You are a professional translator."""
  user: """Translate to {target_lang}: {source}"""
  assistant -> draft: text

  match judge Fluency(draft) {
    Pass          => draft
    Fail(reason)  => retry(2) {
                        user: """The previous translation was rejected: {reason}. Try again."""
                        assistant -> draft
                      } else escalate(human_approval, reason: reason)
    Escalate      => escalate(human_approval, reason: "judge could not decide")
    Score(_)      => draft   // Fluency never returns Score, but Verdict is closed, so it must be handled
  }
}
```

```bash
cd examples
ulx run translate.ulx Translate --arg source=hello --arg target_lang=fr --provider anthropic
```

![translate.ulx demo](/img/demos/translate.gif)

## `summarize.ulx` — parallel independent extraction with `with`

Two independent `vision` extractions run in parallel via a `with` block, then get combined by a `chat` step — vision on Anthropic, chat on Groq in this recording.

```ulexite
conversation Summarize(doc: pdf) -> text {
  with {
    outline  = ask vision(doc) { user: """Extract a section outline.""" }
    keyfacts = ask vision(doc) { user: """List the five most important facts.""" }
  }
  ask chat() {
    system: """You are a technical writer."""
    user: """Using this outline: {outline}\nAnd these facts: {keyfacts}\nWrite a one-page summary."""
  } -> summary: text
  summary
}
```

```bash
cd examples
export ANTHROPIC_API_KEY=sk-ant-... GROQ_API_KEY=gsk_...
ulx run summarize.ulx Summarize --arg doc=fixtures/sample.pdf
```

![summarize.ulx demo](/img/demos/summarize.gif)

## `pdf_qa.ulx` — OCR fallback for PDF question-answering

Deterministic text extraction first; falls back to `vision` on page images only when there's no text layer. `pdf.extract_text` is real (pure-Rust PDF text extraction), so for a PDF that has a text layer — like the shipped `fixtures/sample.pdf` — the `vision` fallback branch is never reached at all. `pdf.to_images` is honestly not implemented (real PDF rasterization needs a bundled rendering engine — see [Standard Library](../standard-library.md)), which is exactly why it's called only inside the `else` branch rather than unconditionally: a genuinely scanned, no-text-layer PDF would hit that branch's clear error instead of a silent fake result.

```ulexite
import "pdf" as pdf
import "vision" as vision

conversation PdfQA(doc: pdf, question: text) -> text {
  text_layer = pdf.extract_text(doc)
  ocr_text = if text_layer.length > 0 {
    text_layer
  } else {
    page_images = pdf.to_images(doc)
    ask vision(page_images) { user: """Transcribe all text in these pages.""" }
  }
  ask chat() {
    system: """Answer strictly using the provided document text."""
    user: """Document:\n{ocr_text}\n\nQuestion: {question}"""
  } -> answer: text
  answer
}
```

```bash
cd examples
ulx run pdf_qa.ulx PdfQA --arg doc=fixtures/sample.pdf --arg question="What is this about?" --provider anthropic
```

![pdf_qa.ulx demo](/img/demos/pdf_qa.gif)

## `rag.ulx` — image captioning + retrieval-augmented generation

Two entry points sharing one `dataset`: `Caption` describes a photo, `AnsweredByRAG` embeds a question, finds the nearest chunks in a toy knowledge base, and answers from context — three capabilities across three vendors (Anthropic vision, OpenAI embed, Groq chat) in this recording.

```ulexite
import "vector" as vector
import "embedding" as embedding

dataset KnowledgeBase: [{doc_id: text, chunk: text, embedding: embedding<1536>}] {
  from "kb/chunks.jsonl"
}

conversation Caption(photo: image) -> text {
  ask vision(photo) { user: """Describe this image in one sentence.""" } -> caption: text
  caption
}

conversation AnsweredByRAG(question: text) -> text {
  q_embedding = embedding.of(question, model: capability(embed))
  top_chunks  = vector.nearest(query: q_embedding, index: KnowledgeBase, k: 5)
  ask chat() {
    system: """Answer only from the provided context; say 'I don't know' if the context is insufficient."""
    user: """Context:\n{top_chunks}\n\nQuestion: {question}"""
  } -> answer: text
  answer
}
```

```bash
cd examples
export ANTHROPIC_API_KEY=sk-ant-... OPENAI_API_KEY=sk-... GROQ_API_KEY=gsk_...
ulx run rag.ulx Caption --arg photo=fixtures/sample.jpg
ulx run rag.ulx AnsweredByRAG --arg question="What is the PTO policy?"
```

![rag.ulx demo](/img/demos/rag.gif)

## `multi_agent.ulx` — nested conversations with handoff

`ResearchReport` calls `ResearchAgent`, then `WriteAgent`, then judges the result with `ReviewAgent` — each is its own nested conversation with its own trace, linked into the parent's, and a failing review retries the write step once before escalating.

```ulexite
judge Quality(subject: text) -> Verdict {
  rubric: """Is this report well-structured, accurate, and free of unsupported claims?"""
}

conversation ResearchAgent(topic: text) -> text {
  ask chat() { user: """Research key facts about {topic}.""" } -> notes: text
  notes
}

conversation WriteAgent(notes: text) -> text {
  ask chat() { user: """Write a two-paragraph report from these notes: {notes}""" } -> report: text
  report
}

conversation ReviewAgent(report: text) -> Verdict {
  judge Quality(report)
}

conversation ResearchReport(topic: text) -> text {
  notes  = ResearchAgent(topic)
  report = WriteAgent(notes)
  match ReviewAgent(report) {
    Pass          => report
    Fail(reason)  => retry(1) { report = WriteAgent(notes) } else escalate(human_approval, reason: reason)
    Escalate      => escalate(human_approval, reason: "review inconclusive")
    Score(_)      => report
  }
}
```

```bash
cd examples
ulx run multi_agent.ulx ResearchReport --arg topic="the history of lighthouses" --provider anthropic
```

![multi_agent.ulx demo](/img/demos/multi_agent.gif)

## `batch.ulx` — sequential loop over a dataset

A `for` loop iterates a `dataset` of support tickets, classifying each one's severity — sequential by default. (Parallelizing independent loop iterations is deliberately *not* expressible by relaxing `with`'s independence guarantee — see [dataset-driven benchmarks](../testing-and-evaluation.md) instead.)

```ulexite
dataset SupportTickets: [{ticket_id: text, body: text}] {
  from "tickets/backlog.jsonl"
}

conversation Triage(body: text) -> text {
  ask chat() { user: """Classify this support ticket's severity (low/medium/high): {body}""" } -> severity: text
  severity
}

conversation TriageBacklog() -> list<text> {
  results = list<text>()
  for ticket in SupportTickets {
    results.append(Triage(ticket.body))
  }
  results
}
```

```bash
cd examples
ulx run batch.ulx TriageBacklog --provider anthropic
```

![batch.ulx demo](/img/demos/batch.gif)

## `eval_translate.ulx` — benchmark with dataset, judge, and snapshot

Reuses `translate.ulx`'s `Translate` conversation and `Fluency` judge (imported, not copy-pasted) against a golden dataset, with a judge threshold and a snapshot assertion. A row whose `Translate` call escalates (the judge couldn't decide) now suspends that row gracefully instead of aborting the whole benchmark — the other rows still complete, and `ulx bench --run-id <id>` plus `ulx approve <id>`/`ulx deny <id>` resolves it.

```ulexite
import conversation Translate from "translate.ulx"
import judge Fluency from "translate.ulx"

dataset TranslationPairs: [{source: text, target_lang: text, golden: text}] {
  from "fixtures/translations.jsonl"
}

benchmark TranslateQuality {
  dataset: TranslationPairs
  run: Translate(source: $.source, target_lang: $.target_lang) -> result
  expect result satisfies judge Fluency(result) with threshold(0.8)
  assert result != $.golden
  snapshot result as "translate/{$.target_lang}"
}
```

```bash
cd examples
ulx bench eval_translate.ulx TranslateQuality --provider anthropic
```

![eval_translate.ulx demo](/img/demos/eval_translate.gif)

## `approval.ulx` — suspend/resume as a human-approval checkpoint

`escalate` suspends the run and checkpoints it; a separate `ulx approve`/`ulx deny` invocation resumes execution exactly where it left off — the same mechanism ordinary retries use, not a separate webhook-driven subsystem.

```ulexite
conversation RefundRequest(order_id: text, amount: float) -> Verdict {
  ask chat() { user: """Summarize refund request for order {order_id}, amount {amount}.""" } -> summary: text
  escalate(human_approval, reason: summary)
  // when a human responds (approve/deny + optional note), execution resumes exactly here
}
```

```bash
cd examples
ulx run approval.ulx RefundRequest --arg order_id=X123 --arg amount=42.50 --provider anthropic
# prints a run id and suspends; resume with:
ulx approve <run_id> --value "approved"   # or: ulx deny <run_id> --note "..."
```

![approval.ulx demo](/img/demos/approval.gif)

## `voice_memo.ulx` — transcribe, reply, speak

Three capabilities chained end to end: `transcribe` the recording, draft a one-sentence reply with `chat`, then `speak` it back out as audio — pinned to Groq for transcribe/chat and OpenAI for speak (Groq has no `speak` capability) via `provider` blocks declared right in the file.

```ulexite
conversation VoiceMemoReply(recording: audio) -> audio {
  ask transcribe(recording) { } -> transcript: text
  ask chat() {
    system: """You write a one-sentence spoken reply to a voice memo."""
    user: """Voice memo transcript:\n{transcript}"""
  } -> reply_text: text
  ask speak() { user: """{reply_text}""" } -> reply_audio: audio
  reply_audio
}
```

```bash
cd examples
export GROQ_API_KEY=gsk_... OPENAI_API_KEY=sk-...
ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav
```

![voice_memo.ulx demo](/img/demos/voice_memo.gif)

## `generate_and_describe.ulx` — generate, then describe what was generated

`generate_image`'s output feeds straight back into `vision` as an ordinary `image`-typed value — a genuinely cross-vendor pipeline (OpenAI generates, Anthropic describes) with no special-casing for the fact that the image was just synthesized rather than supplied by the caller.

```ulexite
conversation GenerateAndDescribe(prompt: text) -> text {
  ask generate_image() { user: """{prompt}""" } -> picture: image
  ask vision(picture) { user: """Describe what you generated in one sentence.""" } -> description: text
  description
}
```

```bash
cd examples
export OPENAI_API_KEY=sk-... ANTHROPIC_API_KEY=sk-ant-...
ulx run generate_and_describe.ulx GenerateAndDescribe --arg prompt="a lighthouse at sunset"
```

![generate_and_describe.ulx demo](/img/demos/generate_and_describe.gif)

## `custom_provider.ulx` — declaring a provider directly in source

No `ulexite.toml` needed at all: a `provider Name { ... }` block declares a fully self-contained provider directly in `.ulx` source, and `ask chat(provider: "LocalAssistant")`'s reserved `provider` argument selects it by name. This one deliberately uses `vendor: "mock"` to demonstrate the declaration mechanism itself, runnable with no API key at all — swap it for a real vendor and it works the same way.

```ulexite
provider LocalAssistant {
  vendor: "mock"
  chat: "unused-by-mock"
}

conversation Greet(name: text) -> text {
  ask chat(provider: "LocalAssistant") {
    user: """Say hello to {name}."""
  } -> greeting: text
  greeting
}
```

```bash
cd examples
ulx run custom_provider.ulx Greet --arg name=world
```

![custom_provider.ulx demo](/img/demos/custom_provider.gif)

## `prompt_from_file.ulx` — loading prompt text from disk

`file("path")` and the `@path` shorthand load prompt text from a file next to the `.ulx` source, instead of an inline `"""..."""` block — `{var}` interpolation inside the loaded file is statically checked exactly like an inline text block. Both forms are fully equivalent; this is purely a convenience for prompts long/reused enough to want their own file.

```ulexite
conversation Greet(name: text, occasion: text) -> text {
  system: file("prompts/greet_system.txt")
  user: @prompts/greet_user.txt
  assistant -> reply: text
  reply
}
```

```bash
cd examples
ulx run prompt_from_file.ulx Greet --arg name=Ada --arg occasion=birthday --provider anthropic
```

![prompt_from_file.ulx demo](/img/demos/prompt_from_file.gif)

## Supporting data

- `fixtures/` — sample inputs (`sample.pdf`, `sample.jpg`, `sample.wav`, `translations.jsonl`) used by the examples above and by `eval_translate.ulx`'s dataset.
- `kb/` — `chunks.jsonl`, the toy knowledge base `rag.ulx` queries.
- `tickets/` — `backlog.jsonl`, the dataset `batch.ulx` iterates over.
- `prompts/` — the on-disk prompt files `prompt_from_file.ulx` loads.

See the [full examples README](https://github.com/JGalego/ulexite/tree/main/examples) in the repo for exact per-example environment variable requirements and the raw `.tape` scripts behind every recording above.
