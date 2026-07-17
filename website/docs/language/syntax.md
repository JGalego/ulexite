---
title: Syntax
description: A declaration-by-declaration tour of Ulexite's recommended syntax, from top-level conversations to retries and escalation.
---

# Syntax

Ulexite source files use the `.ulx` extension, and a package is just a directory with an `ulexite.toml` manifest. Inside a file, identifiers, comments (`//` and `/* */`), and numeric/string literals follow the conventions you already know from Rust or Gleam. The one thing that's specific to Ulexite is the triple-quoted string: multi-line prompt and rubric text uses `"""..."""` blocks with `{expr}` interpolation, and that interpolation is resolved and type-checked at compile time against the enclosing scope. If you interpolate a variable that doesn't exist, or try to interpolate an `image` artifact into a spot that expects `text`, that's a compile error — not something that blows up at runtime after you've already paid for the API call.

`//` doc-comments immediately preceding a `conversation`, `step`, `judge`, or `dataset` declaration are structured documentation that tooling can extract, the same way you'd expect from a doc-comment in any modern language.

This page walks through the language's declarations one at a time, using the same shape of examples you'll find in `examples/*.ulx`.

## The declarations at a glance

Every `.ulx` file is built out of a small set of top-level declarations:

```ulexite
conversation Name(param: Type, ...) -> ReturnType {
  <body>
}

judge Name(subject: ArtifactType) -> Verdict {
  rubric: """..."""
  model: capability(chat)      // optional pin; defaults to runtime policy resolution
}

validator Name(subject: ArtifactType) -> Verdict {
  json_schema: SchemaName       // or regex:, ast:, python:, shell:
}

dataset Name: [ArtifactType] {
  from "path/to/data.jsonl"      // or inline literal rows
}

type Name = <artifact or record type definition>
```

- **`conversation`** is the unit of work: a named, typed function whose body is a sequence of messages, model calls, and control flow.
- **`judge`** is a model-graded check: you hand it a rubric, and it returns a `Verdict` (see [Type System](./type-system.md)).
- **`validator`** is a mechanical check — a JSON Schema, a regex, an AST shape, or a call out to Python/JS/shell — that also returns a `Verdict`.
- **`dataset`** is a typed, named collection of rows, usually loaded from a JSONL file, that you can feed into a `benchmark`.
- **`type`** declares a named artifact or record type you can reuse across declarations.

## Writing a conversation body

A conversation body reads like the transcript it produces. Here's a small translator with a quality gate:

```ulexite
conversation Translate(source: text, target_lang: text) -> text {
  system: """You are a professional translator."""
  user: """Translate to {target_lang}: {source}"""
  assistant -> draft: text

  verdict = judge Fluency(draft)

  match verdict {
    Pass                => draft
    Fail(reason)         => retry(2) {
                              user: """The previous translation was rejected: {reason}. Try again."""
                              assistant -> draft
                            } else escalate(human_approval, reason: reason)
    Escalate             => escalate(human_approval)
  }
}
```

A few things are happening here, each worth calling out on its own:

### Message literals

`system:`, `user:`, and `assistant -> name: Type` are message-literal statements — they read exactly as the transcript they produce. `assistant -> name` without a type annotation infers the type from context, defaulting to `text`. A bare model call (an `assistant ->` that follows `user`/`system` turns) implicitly invokes whatever capability the runtime's provider policy resolves — you don't have to name a vendor or model just to get a reply. When a step needs an explicit capability, a provider pin, or a multimodal input/output type, reach for the explicit `ask` form instead (see [Explicit `ask` calls](#explicit-ask-calls-multimodal-and-provider-capability) below).

### Judges and validators as expressions

`judge` and `validator` are expressions that return a `Verdict` — you can call them inline (`judge Fluency(draft)`) or declare them standalone and import/reuse them across conversations, exactly like any other declaration.

### Exhaustive `match`

`match` over a `Verdict` (or any closed union) must be exhaustive: the compiler rejects a `match` that's missing a variant. This closes off the classic "I forgot to handle the escalate case" bug at compile time, rather than letting an unhandled judge verdict fall through silently at runtime.

### `retry` and `escalate`

`retry(n) { ... } else <fallback-expr>` is sugar for a bounded loop with an explicit exhausted-retries branch. There's no silent infinite retry, and there's no way to declare a retry policy and then have it silently ignored — the `else` branch has to be there (unless the compiler can prove the retry body can't fail).

`escalate(human_approval, ...)` yields a `human_approval` message: the conversation suspends, checkpoints its state, and resumes when a human responds. This is a language-level suspend/resume, not something bolted on after the fact with a webhook. Here's the smallest possible escalation, straight out of `examples/approval.ulx`:

```ulexite
conversation RefundRequest(order_id: text, amount: float) -> Verdict {
  ask chat() { user: """Summarize refund request for order {order_id}, amount {amount}.""" } -> summary: text
  escalate(human_approval, reason: summary)
}
```

## Declarative `with` blocks

When a set of steps is genuinely independent of each other, you can name them as bindings inside a `with` block. The compiler is free to parallelize, cache, and reorder anything declared this way:

```ulexite
conversation Summarize(doc: pdf) -> text {
  with {
    outline  = ask vision(doc) { user: """Extract a section outline.""" }
    keyfacts = ask vision(doc) { user: """List the five most important facts.""" }
  }
  ask chat() {
    system: "You are a technical writer."
    user: """Using this outline: {outline}\nAnd these facts: {keyfacts}\nWrite a one-page summary."""
  } -> summary: text
  summary
}
```

Bindings inside one `with` block have no ordering dependency on each other by construction: referencing a sibling binding from within the same block is a compile error. That restriction is exactly what makes the parallelism and caching sound rather than a best-effort guess — the compiler doesn't need to infer independence from a dataflow analysis, because the grammar itself won't let you write a dependency between siblings. If you do need one binding to depend on another, write it as a second, sequential statement outside the block.

## Explicit `ask` calls, multimodal, and provider capability

The terse `system:`/`user:`/`assistant ->` form covers the common case: a single capability, text-first turn. The moment a step needs multimodal input, an explicit capability, or a provider policy override, use `ask`:

```ulexite
ask <capability>(<artifacts...>, model: <policy>?) {
  system: """..."""
  user: """..."""
} -> name: Type
```

`<capability>` is one of the standard library's capability kinds — `chat`, `vision`, `embed`, `transcribe`, `speak`, `generate_image`, and so on. The compiler checks every artifact you pass in against that capability's declared accepted types, and checks the return binding's type against the capability's declared output types. Concretely, that means this fails to compile:

```ulexite
conversation Caption(clip: video) -> text {
  ask vision(clip) { user: "Describe this clip." } -> caption: text   // OK: vision.accepts includes video
  ask chat(clip)    { user: "Summarize this clip." } -> bad: text     // compile error: chat.accepts = [text, image]
}
```

The second `ask` never reaches a provider — it fails to compile with a diagnostic naming the capability's declared `accepts` set and the artifact's actual type, instead of surfacing as a runtime 400 from whichever vendor you happened to be pointed at.

`model:` optionally pins cost/latency/provider policy — for example `model: cheapest` or `model: pinned("anthropic/claude-...")`. If you omit it, the runtime's provider registry resolves it for you, and no provider name needs to appear anywhere in your default path. You'll also see a `provider:` argument at real `ask` call sites in the examples (e.g. `ask vision(doc, provider: "Anthropic")`) — that's how you disambiguate between multiple providers registered for the same capability; see the provider-configuration docs for the full story.

## Testing and evaluation keywords

`dataset` and `benchmark` bring evaluation into the same language and the same compiler pass as the conversations they test:

```ulexite
dataset TranslationPairs: [{source: text, target_lang: text, golden: text}] {
  from "fixtures/translations.jsonl"
}

benchmark TranslateQuality {
  dataset: TranslationPairs
  run: Translate(source: $.source, target_lang: $.target_lang) -> result
  expect result satisfies judge Fluency(result) with threshold(0.8)
  assert result != golden           // structural inequality is fine; exact match is not required
  snapshot result as """translate/{source_lang}-{target_lang}"""
}
```

- `dataset` is a first-class, typed, versioned, injectable value — not a decorator bolted onto a host-language test function. Inside a `benchmark`, `$` refers to the current row.
- `expect ... satisfies <judge-or-validator> with threshold(...)` and `assert` share the same `Verdict` type that ordinary `match` control flow uses — a benchmark and the conversation it exercises are checked by one compiler pass, not two disconnected tools.
- `snapshot` records and compares an artifact against a golden baseline, today via exact value equality rather than the full design's semantic diff — note the key needs a triple-quoted string (`"""..."""`) to interpolate per row, since interpolation is a text-block feature, not a plain-string one.

See [Testing and evaluation](../testing-and-evaluation.md) for the full framework.

## Imports and reuse

```ulexite
import judge Fluency from "shared/judges.ulx"
import conversation Translate from "shared/translate.ulx"
```

A `conversation`, `judge`, `validator`, or `dataset` is imported and called like a function. Reuse in Ulexite is "import and call" — there's no subclassing, no overriding, no plugin registration ceremony to reason about.

## Putting it together

Two more patterns worth seeing before you write your own conversations. First, one conversation calling another, with a review step gating the result:

```ulexite
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

Second, an ordinary imperative loop over a dataset — loops are deliberately kept outside `with` blocks, since a loop body is sequential by nature:

```ulexite
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

For the full design rationale, see [§7 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/07-recommended-syntax.md).
