# 16. Testing Framework

## 16.1 Keywords, not config

`expect`, `assert`, `snapshot`, `benchmark`, and `dataset` are grammar productions (§8), evaluated by the same compiler and runtime that executes ordinary conversations (§12.8) — not parsed by an external test-runner reading a YAML/JSON schema the way Promptfoo and OpenAI Evals both are (§2.2, §3.3). This is the single most direct payoff of §4.6: a test and the program it tests share one type system, so `expect result satisfies judge Fluency(result)` is checked against the exact same `Verdict` type (§9.4) the program's own `match` statements use — a test suite cannot silently drift from what the program actually returns the way an assertion library bolted onto a different type system can.

## 16.2 `dataset` as a fixture, not a file

A `dataset` (§7.2, §11.6) is a versioned, content-addressed, typed value — the native parametrize-over-data mechanism (pytest's `@parametrize`, §2.6, promoted to a first-class value rather than a decorator):

```
dataset TranslationPairs: [{source: text, target_lang: text, golden: text}] {
  from "fixtures/translations.jsonl"
}
```

A `benchmark` (§16.4) referencing a `dataset` runs its body once per row, each reported independently — the same "N cases, N reports" ergonomic pytest's parametrize provides, with the dataset itself content-addressed (§11.6) so a specific benchmark run can always be traced back to the exact dataset version it consumed.

## 16.3 `expect` — polling assertions over non-deterministic output

```
expect translation satisfies judge Fluency(translation) with threshold(0.8)
expect tool_call_result settles within duration(30s)
```

`expect` is Playwright's web-first, auto-waiting assertion (§2.6) adapted to LLM non-determinism: `expect ... satisfies <judge-or-validator>` resamples/re-evaluates up to a configurable retry budget until the verdict converges or the budget is exhausted, rather than grading a single sample once — closing the gap where every framework in §2.2 grades exactly one completion per test case with no native retry-until-converged semantics. `settles within` is the tool-call/async-effect analogue of Playwright's actionability polling, applied to a `tool_output` artifact reaching a terminal state (§5.2, §11.4) instead of a DOM element becoming visible.

## 16.4 `assert` and `benchmark`

```
benchmark TranslateQuality {
  dataset: TranslationPairs
  run: Translate(source: $.source, target_lang: $.target_lang) -> result
  expect result satisfies judge Fluency(result) with threshold(0.8)
  assert result != golden
  snapshot result as "translate/{source_lang}-{target_lang}"
}
```

`assert` is an ordinary boolean check (structural/deterministic comparisons, `assert.semantically_equal`, §15.15) for the subset of a test that *is* deterministic — mixed freely with `expect`'s judge-graded checks in the same `benchmark` body, because both ultimately produce the same `Verdict`-shaped pass/fail the compiler and reporter treat uniformly (§16.1).

## 16.5 `snapshot` — semantic golden-output testing

`snapshot expr as "<key>"` records (on first run, or with `ulx test --update-snapshots`) or compares (on subsequent runs) an artifact against a stored golden baseline — Playwright's visual-regression workflow (§2.6) adapted for text/structured content with one deliberate divergence: the comparison is a **semantic diff** (via `judge`/`assert.semantically_equal`, configurable per snapshot), not a byte- or line-diff, because exact-match text comparison is far too brittle for genuinely non-deterministic model output (§2.6's stated lesson from adapting Playwright's mechanism to this domain). A failing snapshot's report shows the semantic diff's specific claim of divergence (e.g., "golden mentions population data; new output omits it"), not merely "strings differ."

## 16.6 Reporting and aggregation

`ulx test` produces a structured report (JSON/JUnit-compatible for CI, per Promptfoo's genuinely good CI-native output, §2.2) aggregating `assert`/`expect`/`snapshot` results per dataset row, with `metrics` (§15.15) available for custom aggregation (`metrics.pass_rate`, `metrics.percentile`) inside a `benchmark`'s own reporting block. Every `benchmark` run produces a full trace (§12.8, §18) exactly as an ordinary conversation run would, so a failing test case is debuggable with the same trace viewer (§19, §20) used for production runs — not a separate, poorer-tooled test-failure format.

## 16.7 Fixtures and scoping

Reusable setup — a mock provider standing in for a real one during CI, a shared judge configuration — is declared as an ordinary importable value (§7.7) and scoped by where it's imported, mirroring pytest's `conftest.py`-based implicit scoping (§2.6) without inventing a separate fixture-injection mechanism: a package's `test/` directory conventionally holds `.ulx` files whose `judge`/`dataset`/`conversation` declarations are available to sibling test files the way a `conftest.py` fixture is available to sibling test modules, resolved by ordinary import rules (§7.7, §14) rather than a bespoke discovery protocol.

## 16.8 Determinism and CI reproducibility

Because every `ask`/`judge` call is content-addressed and cached by default (§10.3), and a `dataset` is versioned by content hash (§11.6), a `benchmark` run in CI against an unchanged program and dataset is a 100% cache hit — re-running the full suite costs nothing beyond IR interpretation, and a genuinely new run (changed program, changed dataset, or an explicit `--no-cache`) is the only case that spends tokens, directly answering the operational cost complaint implicit in §2.2's survey of frameworks with no first-class caching story for eval loops.
