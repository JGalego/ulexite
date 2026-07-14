# 21. Complete Example Programs

Each example is complete and compiles against the grammar in §8. Comments call out which §-numbered concept each example is exercising.

## 21.1 Translation with retry and human escalation

Full version of §7.3, shown once here as the canonical "hello world":

```
// examples/translate.ulx
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
    Score(_)      => draft   // Fluency never returns Score, but Verdict is closed (§9.4) so it must be handled
  }
}
```

## 21.2 Summarization with parallel independent extraction (`with`)

Full version of §7.4 — demonstrates parallel execution over independent steps.

```
// examples/summarize.ulx
conversation Summarize(doc: pdf) -> text {
  with {
    outline  = ask vision(doc) { user: "Extract a section outline." } -> text
    keyfacts = ask vision(doc) { user: "List the five most important facts." } -> text
  }
  ask chat() {
    system: "You are a technical writer."
    user: """Using this outline: {outline}\nAnd these facts: {keyfacts}\nWrite a one-page summary."""
  } -> summary: text
  summary
}
```

## 21.3 OCR and PDF question-answering

```
// examples/pdf_qa.ulx
import "pdf" as pdf
import "vision" as vision

conversation PdfQA(doc: pdf, question: text) -> text {
  with {
    text_layer   = pdf.extract_text(doc)                       // deterministic extraction, §15.5
    page_images  = pdf.to_images(doc)                          // for scanned/no-text-layer pages
  }
  ocr_text = if text_layer.length > 0 { text_layer } else {
    ask vision(page_images) { user: "Transcribe all text in these pages." } -> text
  }
  ask chat() {
    system: "Answer strictly using the provided document text."
    user: """Document:\n{ocr_text}\n\nQuestion: {question}"""
  } -> answer: text
  answer
}
```

## 21.4 Image captioning + RAG over a document set

```
// examples/rag.ulx
import "vector" as vector
import "embedding" as embedding

dataset KnowledgeBase: [{doc_id: text, chunk: text, embedding: embedding<1536>}] {
  from "kb/chunks.jsonl"
}

conversation Caption(photo: image) -> text {
  ask vision(photo) { user: "Describe this image in one sentence." } -> caption: text
  caption
}

conversation AnsweredByRAG(question: text) -> text {
  q_embedding = embedding.of(question, model: capability(embed))
  top_chunks  = vector.nearest(query: q_embedding, index: KnowledgeBase, k: 5)
  ask chat() {
    system: "Answer only from the provided context; say 'I don't know' if the context is insufficient."
    user: """Context:\n{top_chunks}\n\nQuestion: {question}"""
  } -> answer: text
  answer
}
```

## 21.5 Multi-agent workflow (nested conversations, handoff)

```
// examples/multi_agent.ulx
conversation ResearchAgent(topic: text) -> text {
  ask chat() { user: "Research key facts about {topic}." } -> notes: text
  notes
}

conversation WriteAgent(notes: text) -> text {
  ask chat() { user: "Write a two-paragraph report from these notes: {notes}" } -> report: text
  report
}

conversation ReviewAgent(report: text) -> Verdict {
  judge Quality(report)   // reuses the pattern from §21.1
}

conversation ResearchReport(topic: text) -> text {
  notes  = ResearchAgent(topic)          // nested conversation, §5.1 — its own trace, linked to this parent's
  report = WriteAgent(notes)
  match ReviewAgent(report) {
    Pass          => report
    Fail(reason)  => retry(1) { report = WriteAgent(notes) } else escalate(human_approval, reason: reason)
    Escalate      => escalate(human_approval, reason: "review inconclusive")
    Score(_)      => report
  }
}
```

## 21.6 Batch execution over a dataset (loops)

```
// examples/batch.ulx
dataset SupportTickets: [{ticket_id: text, body: text}] {
  from "tickets/backlog.jsonl"
}

conversation Triage(body: text) -> text {
  ask chat() { user: "Classify this support ticket's severity (low/medium/high): {body}" } -> severity: text
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

`for` iterates the imperative region (§10.2, §8) — each `Triage` call is sequential by default; see §21.2 for the `with`-block alternative when the author wants the compiler to parallelize independent iterations explicitly (`with { for ticket in SupportTickets { ... } }` is intentionally *not* legal grammar, §9.7 — parallel batch execution is expressed via `dataset`-driven `benchmark`s, §21.7, or an explicit `parallel_map` stdlib helper over provably pure steps, not by relaxing `with`'s independence guarantee for loops).

## 21.7 Evaluation: benchmark with dataset, judge, and snapshot

```
// examples/eval_translate.ulx
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

Run with `ulx test examples/eval_translate.ulx` (§16.6); every row is cached per §10.3/§16.8, so re-running after an unrelated code change costs nothing.

## 21.8 Human approval as a suspend/resume checkpoint

```
// examples/approval.ulx
conversation RefundRequest(order_id: text, amount: float) -> Verdict {
  ask chat() { user: "Summarize refund request for order {order_id}, amount {amount}." } -> summary: text
  escalate(human_approval, reason: summary)   // suspends here; checkpointed per §10.4
  // when a human responds (approve/deny + optional note), execution resumes exactly here
}
```

`ulx run examples/approval.ulx --order_id X123 --amount 42.50` suspends and prints the run id; `ulx approve <run_id>` or `ulx deny <run_id> --note "..."` resumes it (§7.3, §10.7) — the same checkpoint mechanism used for ordinary retries, not a separate webhook-driven subsystem.

## 21.9 Reusable workflow as an importable, parametrized value

`Translate` (§21.1) is already this: any package can `import conversation Translate from "translate.ulx"` and call it with different arguments, or wrap it (§21.7) in a `benchmark`, without subclassing or decorator ordering (§4.8, §7.7) — no additional syntax is needed beyond what §21.1 and §21.7 already show, which is itself the point: reuse in Ulexite is exactly "import and call," never a distinct mechanism from ordinary composition.
