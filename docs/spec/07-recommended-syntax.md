# 7. Recommended Syntax

**Recommendation: Alternative C (transcript-flavored, imperative core) as the base language, with Alternative A's `with` CTE block adopted verbatim as an optional declarative sub-mode.** Rationale is in §6.4; this section fixes the syntax in enough detail to write the grammar (§8) and the examples (§21).

## 7.1 Lexical basics

- Files have extension `.ulx`; a package is a directory with an `ulexite.toml` manifest (§14).
- Identifiers, comments (`//`, `/* */`), and numeric/string literals follow Rust/Gleam-family conventions.
- Multi-line prompt/rubric text uses triple-quoted strings (`"""..."""`) with `{expr}` interpolation, resolved and type-checked at compile time against the enclosing scope — an interpolation referencing an undefined variable or a type mismatch (e.g., interpolating an `image` artifact into a position requiring `text`) is a compile error (§9), not the runtime `KeyError` documented against LangChain in §3.2.
- `//` doc-comments immediately preceding a `conversation`, `step`, `judge`, or `dataset` declaration are structured documentation, extracted by `ulx doc` (§20.9).

## 7.2 Top-level declarations

```
conversation Name(param: Type, ...) -> ReturnType {
  <body>
}

judge Name(subject: ArtifactType) -> Verdict {
  rubric: """..."""
  model: capability(chat)      // optional pin; defaults to runtime policy resolution (§12.4)
}

validator Name(subject: ArtifactType) -> Verdict {
  json_schema: SchemaName       // or regex:, ast:, python:, shell: (§15.11-15.13)
}

dataset Name: [ArtifactType] {
  from "path/to/data.jsonl"      // or inline literal rows
}

type Name = <artifact or record type definition>   // §9
```

## 7.3 Conversation body: message literals and steps

```
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

Key elements:

- `system:` / `user:` / `assistant -> name: Type` are message-literal statements — they read as the transcript they produce (§4.1, §6.3). `assistant -> name` without a type annotation infers the type from context (default `text`).
- A bare model call (an `assistant ->` following user/system turns) implicitly invokes the capability-resolved model (§5.5, §12.4); an explicit `ask capability(...) { ... } -> name` form (§7.5) is available when a step needs an explicit capability, provider pin, or multimodal input/output type.
- `judge` and `validator` are expressions returning `Verdict` (§9.4), callable inline (`judge Fluency(draft)`) or declared standalone (§7.2) and imported/reused across conversations.
- `match` over a `Verdict` must be exhaustive (§9.4, §8.7); the compiler rejects a `match` missing a variant, closing the "unhandled judge verdict" gap in §2.7 and §3.4.
- `retry(n) { ... } else <fallback-expr>` is sugar for a bounded loop with an explicit exhausted-retries branch — there is no silent infinite retry and no silently-ignored retry policy (contrast langgraph#6027, §3.2).
- `escalate(human_approval, ...)` yields a `human_approval` message (§5.2) — the conversation suspends, checkpoints (§10.4), and resumes when a human responds; this is a language-level `await`, not an out-of-band webhook glued on afterward (contrast the OpenAI Agents SDK's HITL-as-afterthought history, §2.3).

## 7.4 Declarative sub-blocks (`with`)

Where a set of steps is genuinely independent — the common case §4.5 and §6.1 target — a `with` block names them as CTE-style bindings the compiler is free to parallelize, cache, and reorder (§10.2):

```
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

Bindings inside one `with` block have no ordering dependency on each other by construction — referencing a sibling binding from within the same block is a compile error (§9.7), which is what makes the parallelism/caching claim sound rather than aspirational (contrast Pulumi's non-pure preview, §2.4).

## 7.5 Explicit `ask`, multimodal, and provider capability

```
ask <capability>(<artifacts...>, model: <policy>?) {
  system: """..."""
  user: """..."""
} -> name: Type
```

- `<capability>` is one of the stdlib capability kinds (§15.1): `chat`, `vision`, `embed`, `transcribe`, `speak`, `generate_image`, etc. The compiler checks every artifact passed in against the capability's declared accepted types (§9.6) and the return binding's type against its declared output types — a `video` artifact passed to `chat()` is rejected before compilation succeeds, closing the multimodal-routing gap in §3.2.
- `model:` optionally pins cost/latency/provider policy (e.g. `model: cheapest`, `model: pinned("anthropic/claude-...")`); omitted, the runtime's provider registry resolves it per §12.4, and no provider name appears in the default path (§4.3).

## 7.6 Testing and evaluation keywords

```
dataset TranslationPairs: [{source: text, target_lang: text, golden: text}] {
  from "fixtures/translations.jsonl"
}

benchmark TranslateQuality {
  dataset: TranslationPairs
  run: Translate(source: $.source, target_lang: $.target_lang) -> result
  expect result satisfies judge Fluency(result) with threshold(0.8)
  assert result != golden           // structural inequality is fine; exact match is not required
  snapshot result as "translate/{source_lang}-{target_lang}"
}
```

- `dataset` is a first-class, typed, versioned, injectable value (§16.2) — the native parametrize-over-data mechanism, not a decorator bolted onto a host-language test function (contrast Promptfoo/OpenAI Evals, §2.2, §3.2).
- `expect ... satisfies <judge-or-validator> with threshold(...)` and `assert` share the same `Verdict` type as ordinary `match` control flow (§7.3) — a benchmark and the conversation it exercises are checked by one compiler pass (§3.4).
- `snapshot` records/compares a semantic (not textual) diff of an artifact against a golden baseline (§16.5, borrowing Playwright's visual-regression workflow, §2.6, adapted for text/structured semantics rather than pixels).

## 7.7 Imports and reuse

```
import judge Fluency from "shared/judges.ulx"
import conversation Translate from "shared/translate.ulx"
```

A `conversation`, `judge`, `validator`, or `dataset` is imported and called like a function — reuse is "import and call," never "subclass and override" (§4.8), directly resolving the subclassing/decorator-ordering complaints catalogued against DSPy's `Module`/`Signature` and Semantic Kernel's plugin registration in §2.1 and §2.3.

## 7.8 Why this resolves the tradeoffs in §6.4

- Reads like a transcript (Alternative C) for the sequential, stateful common case, while regaining Alternative A's visual/compiler-legible parallelism exactly where the author explicitly says a set of steps is independent (`with`), rather than forcing the compiler to infer independence from whole-program dataflow analysis alone.
- Never adopts Alternative B's tag-soup: natural-language prompt/rubric text always lives in a plain triple-quoted string, never embedded inside markup attributes.
- `match` exhaustiveness over `Verdict` (§7.3) is the syntax-level payoff of §4.7's principle and directly answers §2.7 point 4's "judge/eval mechanism, always bolted on, never able to see the orchestration language's own types."
