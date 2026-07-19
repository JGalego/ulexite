---
title: Examples
description: Twelve complete, runnable Ulexite programs, one concept at a time, with interactive terminal demos.
---

import MockConsole from '@site/src/components/MockConsole';

# Examples

Every program below ships in [`examples/`](https://github.com/JGalego/ulexite/tree/main/examples) in the repo, `ulx check`s cleanly with no configuration at all, and is replayed offline end-to-end by the project's own test suite. Try any of them in the [Playground](/playground) first — paste the code in and watch the diagnostics — then run the real thing once you have a provider configured (see [Providers](../providers.md)).

Several examples declare their own `provider { ... }` blocks directly in source and pin capabilities to specific vendors — no `ulexite.toml` needed for those, just the env var(s) named in each section. The rest resolve providers from `ulexite.toml`; if more than one configured vendor serves the same capability, pass `--provider <name>` to disambiguate.

The consoles below are hand-authored stand-ins for a real `ulx run`/`ulx bench` session — same role emojis and coloring the CLI's own `--output text` transcript uses (see `ulx-cli::output::role_style`) — illustrating one representative run per example rather than a literal recorded terminal session. `pdf_qa.ulx`'s demo takes the text-layer path, since `pdf.extract_text` is real (pure-Rust PDF text extraction) and the shipped `fixtures/sample.pdf` has a text layer, so the `vision` fallback is never reached. `eval_translate.ulx`'s demo shows `ulx bench` suspending one row gracefully on a mid-run judge escalation instead of aborting the whole benchmark.

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

<MockConsole blocks={[{
  command: 'ulx run translate.ulx Translate --arg source=hello --arg target_lang=fr --provider anthropic',
  lines: [
    {kind: 'turn', emoji: '🧭', role: 'system', tone: 'system', text: 'You are a professional translator.', delayMs: 350},
    {kind: 'turn', emoji: '🧑', role: 'user', tone: 'user', text: 'Translate to fr: hello', delayMs: 400},
    {kind: 'turn', emoji: '🤖', role: 'assistant', tone: 'assistant', text: 'Bonjour', delayMs: 1100},
    {kind: 'turn', emoji: '⚖️', role: 'judge Fluency', tone: 'judge', text: 'Pass', delayMs: 900},
    {kind: 'note', text: 'Bonjour', delayMs: 400},
    {kind: 'rule', delayMs: 250},
    {kind: 'summary', rows: [
      ['run id', '3e9a7c1d5f2b8064'],
      ['status', 'ok'],
      ['capabilities', 'chat, judge'],
      ['provider', 'anthropic — chat (claude-haiku-4-5), judge (claude-sonnet-4-5)'],
    ]},
  ],
}]} />

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

<MockConsole blocks={[{
  command: 'ulx run summarize.ulx Summarize --arg doc=fixtures/sample.pdf',
  lines: [
    {kind: 'turn', emoji: '👁️', role: 'vision (outline)', tone: 'assistant', text: '1. Introduction  2. Methodology  3. Results  4. Conclusion', delayMs: 1000},
    {kind: 'turn', emoji: '👁️', role: 'vision (keyfacts)', tone: 'assistant', text: 'Revenue +12%, churn down to 4%, NPS 61, two new markets launched, 92% renewal rate.', delayMs: 1000},
    {kind: 'turn', emoji: '🤖', role: 'chat', tone: 'assistant', text: 'A one-page summary combining the outline and key facts above.', delayMs: 1200},
    {kind: 'note', text: 'A one-page summary combining the outline and key facts above.', delayMs: 400},
    {kind: 'rule', delayMs: 250},
    {kind: 'summary', rows: [
      ['run id', '9d2f6a1b7c4e0358'],
      ['status', 'ok'],
      ['capabilities', 'vision, chat'],
      ['provider', 'anthropic — vision (claude-haiku-4-5), groq — chat (llama-3.3-70b-versatile)'],
    ]},
  ],
}]} />

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

<MockConsole blocks={[{
  command: 'ulx run pdf_qa.ulx PdfQA --arg doc=fixtures/sample.pdf --arg question="What is this about?" --provider anthropic',
  lines: [
    {kind: 'note', text: 'pdf.extract_text(doc) → 1,842 chars (text layer found; vision fallback skipped)', delayMs: 500},
    {kind: 'turn', emoji: '🧭', role: 'system', tone: 'system', text: 'Answer strictly using the provided document text.', delayMs: 300},
    {kind: 'turn', emoji: '🧑', role: 'user', tone: 'user', text: 'Document:\n(1,842 chars)…\n\nQuestion: What is this about?', delayMs: 400},
    {kind: 'turn', emoji: '🤖', role: 'assistant', tone: 'assistant', text: "It's a quarterly business review covering revenue, churn, and market expansion.", delayMs: 1100},
    {kind: 'note', text: "It's a quarterly business review covering revenue, churn, and market expansion.", delayMs: 400},
    {kind: 'rule', delayMs: 250},
    {kind: 'summary', rows: [
      ['run id', 'b6081ce4a72f9d53'],
      ['status', 'ok'],
      ['capabilities', 'chat'],
      ['provider', 'anthropic — chat (claude-haiku-4-5)'],
    ]},
  ],
}]} />

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

<MockConsole blocks={[
  {
    command: 'ulx run rag.ulx Caption --arg photo=fixtures/sample.jpg',
    lines: [
      {kind: 'turn', emoji: '👁️', role: 'vision', tone: 'assistant', text: 'A lighthouse standing on a rocky coastline at sunset.', delayMs: 1000},
      {kind: 'note', text: 'A lighthouse standing on a rocky coastline at sunset.', delayMs: 400},
      {kind: 'rule', delayMs: 250},
      {kind: 'summary', rows: [
        ['run id', '4a8e1f2c9b036d75'],
        ['status', 'ok'],
        ['capabilities', 'vision'],
        ['provider', 'anthropic — vision (claude-haiku-4-5)'],
      ]},
    ],
  },
  {
    command: 'ulx run rag.ulx AnsweredByRAG --arg question="What is the PTO policy?"',
    lines: [
      {kind: 'note', text: 'embedding.of(question) → 1536-d vector', delayMs: 500},
      {kind: 'note', text: 'vector.nearest(KnowledgeBase, k=5) → 5 chunks', delayMs: 500},
      {kind: 'turn', emoji: '🧭', role: 'system', tone: 'system', text: "Answer only from the provided context; say 'I don't know' if the context is insufficient.", delayMs: 300},
      {kind: 'turn', emoji: '🧑', role: 'user', tone: 'user', text: 'Context:\n(5 chunks)…\n\nQuestion: What is the PTO policy?', delayMs: 400},
      {kind: 'turn', emoji: '🤖', role: 'assistant', tone: 'assistant', text: 'Employees accrue 15 PTO days per year, with unused days carrying over up to a five-day cap.', delayMs: 1100},
      {kind: 'note', text: 'Employees accrue 15 PTO days per year, with unused days carrying over up to a five-day cap.', delayMs: 400},
      {kind: 'rule', delayMs: 250},
      {kind: 'summary', rows: [
        ['run id', '77c0a3d6e19f4b82'],
        ['status', 'ok'],
        ['capabilities', 'embed, chat'],
        ['provider', 'openai — embed (text-embedding-3-small), groq — chat (llama-3.3-70b-versatile)'],
      ]},
    ],
  },
]} />

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

<MockConsole blocks={[{
  command: 'ulx run multi_agent.ulx ResearchReport --arg topic="the history of lighthouses" --provider anthropic',
  lines: [
    {kind: 'turn', emoji: '🤖', role: 'ResearchAgent', tone: 'assistant', text: 'The Pharos of Alexandria (3rd century BC) is the earliest known lighthouse; the Eddystone and Fastnet lights pioneered modern wave-swept construction.', delayMs: 1200},
    {kind: 'turn', emoji: '🤖', role: 'WriteAgent', tone: 'assistant', text: 'Lighthouses trace back to the Pharos of Alexandria... (two-paragraph report)', delayMs: 1300},
    {kind: 'turn', emoji: '⚖️', role: 'judge Quality (ReviewAgent)', tone: 'judge', text: 'Pass', delayMs: 900},
    {kind: 'note', text: 'Lighthouses trace back to the Pharos of Alexandria... (two-paragraph report)', delayMs: 400},
    {kind: 'rule', delayMs: 250},
    {kind: 'summary', rows: [
      ['run id', 'e02b7f4d183a6c9e'],
      ['status', 'ok'],
      ['capabilities', 'chat, judge'],
      ['provider', 'anthropic — chat (claude-haiku-4-5), judge (claude-sonnet-4-5)'],
    ]},
  ],
}]} />

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

<MockConsole blocks={[{
  command: 'ulx run batch.ulx TriageBacklog --provider anthropic',
  lines: [
    {kind: 'turn', emoji: '🤖', role: 'Triage (1/3)', tone: 'assistant', text: "'App crashes on login' → high", delayMs: 800},
    {kind: 'turn', emoji: '🤖', role: 'Triage (2/3)', tone: 'assistant', text: "'Dashboard chart colors look off' → low", delayMs: 800},
    {kind: 'turn', emoji: '🤖', role: 'Triage (3/3)', tone: 'assistant', text: "'Export stalls above 10k rows' → medium", delayMs: 800},
    {kind: 'note', text: '[high, low, medium]', delayMs: 400},
    {kind: 'rule', delayMs: 250},
    {kind: 'summary', rows: [
      ['run id', 'c5d90a2f7e461b38'],
      ['status', 'ok'],
      ['capabilities', 'chat'],
      ['provider', 'anthropic — chat (claude-haiku-4-5)'],
    ]},
  ],
}]} />

## `eval_translate.ulx` — benchmark with dataset, judge, and snapshot

Reuses `translate.ulx`'s `Translate` conversation and `Fluency` judge (imported, not copy-pasted) against a golden dataset, with a judge threshold and a snapshot assertion. A row whose `Translate` call escalates (the judge couldn't decide) now suspends that row gracefully instead of aborting the whole benchmark — the other rows still complete, and `ulx bench --run-id <id>` plus `ulx approve <id>`/`ulx deny <id>` resolves it. The `snapshot` statement records a real golden baseline on first run and compares against it (exact value equality) on every later one; `ulx bench --update-snapshots` accepts a new baseline deliberately.

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
  snapshot result as """translate/{$.target_lang}"""
}
```

```bash
cd examples
ulx bench eval_translate.ulx TranslateQuality --provider anthropic
```

<MockConsole blocks={[{
  command: 'ulx bench eval_translate.ulx TranslateQuality --provider anthropic',
  lines: [
    {kind: 'note', text: 'row 1/3  en→fr   hello → Bonjour            judge Fluency: Pass (0.96)   snapshot: recorded', delayMs: 900},
    {kind: 'note', text: 'row 2/3  en→es   good morning → Buenos días  judge Fluency: Pass (0.91)   snapshot: recorded', delayMs: 900},
    {kind: 'note', text: 'row 3/3  en→de   thank you → …               judge Fluency: Escalate      suspended (row r3)', delayMs: 900},
    {kind: 'rule', delayMs: 250},
    {kind: 'summary', rows: [
      ['run id', 'f1a4d086c2937be5'],
      ['status', '2 passed, 1 suspended'],
      ['threshold', '0.8'],
      ['resume with', 'ulx bench --run-id f1a4d086c2937be5, then ulx approve r3 / ulx deny r3'],
    ]},
  ],
}]} />

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

<MockConsole blocks={[
  {
    command: 'ulx run approval.ulx RefundRequest --arg order_id=X123 --arg amount=42.50 --provider anthropic',
    lines: [
      {kind: 'turn', emoji: '🤖', role: 'chat', tone: 'assistant', text: 'Order X123: refund request for $42.50, reason unspecified — appears routine.', delayMs: 1000},
      {kind: 'turn', emoji: '🙋', role: 'escalate human_approval', tone: 'escalate', text: 'Order X123: refund request for $42.50, reason unspecified — appears routine. (suspended)', delayMs: 700},
      {kind: 'note', text: 'suspended: waiting on `human_approval`', delayMs: 400},
      {kind: 'rule', delayMs: 250},
      {kind: 'summary', rows: [
        ['run id', 'a17f2c9b3e5d1046'],
        ['status', 'suspended'],
        ['capabilities', 'chat, escalate'],
        ['provider', 'anthropic — chat (claude-haiku-4-5)'],
      ]},
    ],
  },
  {
    command: 'ulx approve a17f2c9b3e5d1046 --value "approved"',
    lines: [
      {kind: 'turn', emoji: '🙋', role: 'escalate human_approval', tone: 'escalateResolved', text: 'Order X123 refund request => approved', delayMs: 500},
      {kind: 'note', text: 'approved', delayMs: 400},
      {kind: 'rule', delayMs: 250},
      {kind: 'summary', rows: [
        ['run id', 'a17f2c9b3e5d1046'],
        ['status', 'ok'],
        ['capabilities', 'chat, escalate'],
        ['provider', 'anthropic — chat (claude-haiku-4-5)'],
      ]},
    ],
  },
]} />

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

<MockConsole blocks={[{
  command: 'ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav',
  lines: [
    {kind: 'turn', emoji: '🎙️', role: 'transcribe', tone: 'assistant', text: 'Hey, just checking if the quarterly numbers are ready for review.', delayMs: 900},
    {kind: 'turn', emoji: '🧭', role: 'system', tone: 'system', text: 'You write a one-sentence spoken reply to a voice memo.', delayMs: 300},
    {kind: 'turn', emoji: '🧑', role: 'user', tone: 'user', text: 'Voice memo transcript:\nHey, just checking if the quarterly numbers are ready for review.', delayMs: 400},
    {kind: 'turn', emoji: '🤖', role: 'assistant', tone: 'assistant', text: 'Yes, the quarterly numbers are finalized and ready for your review.', delayMs: 900},
    {kind: 'turn', emoji: '🔊', role: 'speak', tone: 'assistant', text: '[audio: reply.wav, 2.1s]', delayMs: 700},
    {kind: 'note', text: 'reply_audio → fixtures/out/reply.wav', delayMs: 400},
    {kind: 'rule', delayMs: 250},
    {kind: 'summary', rows: [
      ['run id', '6f3c9d1a08e4b275'],
      ['status', 'ok'],
      ['capabilities', 'transcribe, chat, speak'],
      ['provider', 'groq — transcribe, chat (whisper-large-v3, llama-3.3-70b-versatile), openai — speak (tts-1)'],
    ]},
  ],
}]} />

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

<MockConsole blocks={[{
  command: 'ulx run generate_and_describe.ulx GenerateAndDescribe --arg prompt="a lighthouse at sunset"',
  lines: [
    {kind: 'turn', emoji: '🖼️', role: 'generate_image', tone: 'assistant', text: '[image: picture.png, 1024x1024]', delayMs: 1400},
    {kind: 'turn', emoji: '👁️', role: 'vision', tone: 'assistant', text: 'A lighthouse silhouetted against an orange and purple sunset sky, waves breaking at its base.', delayMs: 1100},
    {kind: 'note', text: 'A lighthouse silhouetted against an orange and purple sunset sky, waves breaking at its base.', delayMs: 400},
    {kind: 'rule', delayMs: 250},
    {kind: 'summary', rows: [
      ['run id', '2b7e5a94f0c3d861'],
      ['status', 'ok'],
      ['capabilities', 'generate_image, vision'],
      ['provider', 'openai — generate_image (gpt-image-1), anthropic — vision (claude-haiku-4-5)'],
    ]},
  ],
}]} />

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

<MockConsole blocks={[{
  command: 'ulx run custom_provider.ulx Greet --arg name=world',
  lines: [
    {kind: 'turn', emoji: '🧑', role: 'user', tone: 'user', text: 'Say hello to world.', delayMs: 300},
    {kind: 'turn', emoji: '🤖', role: 'assistant', tone: 'assistant', text: '[mock:chat] response to -> user: Say hello to world.', delayMs: 600},
    {kind: 'note', text: '[mock:chat] response to -> user: Say hello to world.', delayMs: 400},
    {kind: 'rule', delayMs: 250},
    {kind: 'summary', rows: [
      ['run id', '0c4b8f21d6a3e957'],
      ['status', 'ok'],
      ['capabilities', 'chat'],
      ['provider', 'LocalAssistant (mock) — chat'],
    ]},
  ],
}]} />

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

<MockConsole blocks={[{
  command: 'ulx run prompt_from_file.ulx Greet --arg name=Ada --arg occasion=birthday --provider anthropic',
  lines: [
    {kind: 'turn', emoji: '🧭', role: 'system', tone: 'system', text: 'You are a warm, concise greeting writer. Reply with exactly one sentence.', delayMs: 350},
    {kind: 'turn', emoji: '🧑', role: 'user', tone: 'user', text: 'Write a one-sentence greeting for Ada for the occasion: birthday.', delayMs: 400},
    {kind: 'turn', emoji: '🤖', role: 'assistant', tone: 'assistant', text: 'Happy birthday, Ada — wishing you a day as wonderful as you are!', delayMs: 900},
    {kind: 'note', text: 'Happy birthday, Ada — wishing you a day as wonderful as you are!', delayMs: 400},
    {kind: 'rule', delayMs: 250},
    {kind: 'summary', rows: [
      ['run id', '5e9a03c7f184b62d'],
      ['status', 'ok'],
      ['capabilities', 'chat'],
      ['provider', 'anthropic — chat (claude-haiku-4-5)'],
    ]},
  ],
}]} />

## Supporting data

- `fixtures/` — sample inputs (`sample.pdf`, `sample.jpg`, `sample.wav`, `translations.jsonl`) used by the examples above and by `eval_translate.ulx`'s dataset.
- `kb/` — `chunks.jsonl`, the toy knowledge base `rag.ulx` queries.
- `tickets/` — `backlog.jsonl`, the dataset `batch.ulx` iterates over.
- `prompts/` — the on-disk prompt files `prompt_from_file.ulx` loads.

See the [full examples README](https://github.com/JGalego/ulexite/tree/main/examples) in the repo for exact per-example environment variable requirements and the raw `.tape` scripts behind the recordings this page's mock consoles are modeled on.
