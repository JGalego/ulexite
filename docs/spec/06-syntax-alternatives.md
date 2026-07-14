# 6. Syntax Alternatives

Three concrete syntaxes were prototyped against the same example — a two-step translate-then-judge conversation — to evaluate ergonomics before committing to one (§7).

## 6.1 Alternative A — "SQL-flavored" (declarative, clause-based)

```sql
conversation Translate(source: text, target_lang: text) -> Verdict {
  WITH draft AS (
    ASK chat(model: any)
      SYSTEM "You are a professional translator."
      USER "Translate to {target_lang}: {source}"
      -> translation: text
  ),
  graded AS (
    JUDGE draft.translation
      RUBRIC "Is this an accurate, fluent translation of the source?"
      -> verdict: Verdict
  )
  SELECT graded.verdict
  WHERE graded.verdict IS Pass
  RETRY 2 OTHERWISE ESCALATE TO human_approval
}
```

**Tradeoffs.** Reads naturally to anyone who knows SQL's CTE pattern, and makes the "declare what, let the runtime plan it" principle (§4.5) visually obvious — independent `WITH` clauses are legibly parallelizable. But multi-turn, stateful, deeply imperative logic (loop until a judge passes, branch five ways on a tool result) turns into awkward, deeply nested CTEs or procedural escape hatches, the same failure mode that makes real application logic painful in T-SQL/PL-pgSQL. Keyword-heavy, ALL-CAPS clauses read as shouty in a language meant to be read turn-by-turn like a transcript.

## 6.2 Alternative B — "React/JSX-flavored" (component-based, declarative tree)

```jsx
conversation Translate({ source, targetLang }) {
  const draft = <Ask model="chat" system="You are a professional translator.">
    Translate to {targetLang}: {source}
  </Ask> -> translation

  const verdict = <Judge subject={draft.translation} rubric="Is this an accurate, fluent translation?" />

  return (
    <Match value={verdict}>
      <Pass /> -> draft.translation
      <Fail(reason)> -> <Retry limit={2}><Escalate to="human_approval" /></Retry> </Fail>
    </Match>
  )
}
```

**Tradeoffs.** Leans hard on the React analogy from §4.9 (steps as components, re-render on changed inputs) and reads well to the huge population of frontend-adjacent engineers now writing LLM apps. But JSX's angle-bracket tag soup is a poor fit for a domain that is fundamentally about *prose* (system prompts, rubrics, user turns) — embedding multi-line natural-language strings inside JSX attributes and children is exactly the ergonomic pain LangChain's prompt-template-as-Python-string already suffers from, just with more punctuation. It also imports a whole component-lifecycle mental model (props, children, reconciliation) that has no clean mapping for genuinely sequential, side-effecting turns like a tool call or a human approval gate.

## 6.3 Alternative C — "Transcript-flavored" (imperative, message-literal-first)

```
conversation Translate(source: text, target_lang: text) -> Verdict {
  system: "You are a professional translator."
  user: "Translate to {target_lang}: {source}"
  assistant -> translation: text

  judge translation against "Is this an accurate, fluent translation of the source?" -> verdict

  match verdict {
    Pass          => return translation
    Fail(reason)  => retry(2) else escalate(human_approval)
  }
}
```

**Tradeoffs.** Reads like the transcript it produces — a `system:`/`user:`/`assistant ->` block *looks like* the conversation it will run, which is the single biggest ergonomic win for a language whose primary abstraction is the conversation itself (§4.1): a reader can follow the turn-by-turn shape without mentally simulating a call graph or a component tree. Imperative control flow (`match`, `retry`, loops) is ordinary and familiar to anyone coming from Rust/Gleam/Swift, satisfying §4.5's "imperative where beneficial." The risk is the opposite of Alternative A's: because it reads sequentially, it's less visually obvious which steps are independent and therefore parallelizable/cacheable by the compiler — that has to be recovered from the dependency graph the compiler infers from variable references (§10.2), not from lexical adjacency.

## 6.4 Evaluation

| Criterion | A (SQL) | B (JSX) | C (Transcript) |
|---|---|---|---|
| Reads like a conversation | Poor | Poor | **Strong** |
| Natural-language string ergonomics (prompts/rubrics) | Medium | Poor | **Strong** |
| Imperative control flow (loops, branches, retries) | Poor | Medium | **Strong** |
| Visual parallelism hint | **Strong** | Medium | Weak (recovered by compiler, §10.2) |
| Familiarity to target audience (LLM app authors) | Medium | Strong (frontend devs) | **Strong** (general-purpose-language habits) |
| Exhaustiveness / static analysis fit (§9.4) | Medium | Weak | **Strong** (native `match`) |

See [§7 Recommended Syntax](07-recommended-syntax.md) for the resolution: Alternative C as the base language, with an optional `with`-style declarative block (borrowing A's CTE ergonomics, not its keyword casing) for the subset of a conversation the author explicitly marks as independent and plannable.
