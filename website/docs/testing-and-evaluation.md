---
title: Testing and Evaluation
description: expect, assert, snapshot, benchmark, and dataset as grammar — plus the judge-calibration and production-evaluation methodology built on top of them.
---

# Testing and Evaluation

`expect`, `assert`, `snapshot`, `benchmark`, and `dataset` are grammar productions, evaluated by the same compiler and runtime that executes ordinary conversations — not parsed by an external test-runner reading a YAML/JSON config the way many eval frameworks are. That's the core payoff: a test and the program it tests share one type system, so `expect result satisfies judge Fluency(result)` is checked against the exact same `Verdict` type your program's own `match` statements use. A test suite can't silently drift from what the program actually returns the way an assertion library bolted onto a different type system can.

This page covers both halves of that story: the syntax you write (`expect`/`assert`/`snapshot`/`benchmark`/`dataset`), and the methodology behind judges, calibration, and production evaluation built on top of it. Read the callouts carefully — `ulx bench` executes a real, but deliberately narrower, slice of this than the full design describes, and the judge-calibration/production-shadow-evaluation methodology in the second half is design intent, not something you can run today.

## `dataset` — a fixture, not a file

A `dataset` is a versioned, typed value — the native parametrize-over-data mechanism, promoted to a first-class declaration rather than a decorator:

```ulexite
dataset TranslationPairs: [{source: text, target_lang: text, golden: text}] {
  from "fixtures/translations.jsonl"
}
```

A `benchmark` referencing a `dataset` runs its body once per row, each reported independently — "N cases, N reports," with the dataset content-addressed so a specific benchmark run can be traced back to the exact dataset version it consumed.

## `expect` — a judge-graded assertion

```ulexite
expect translation satisfies judge Fluency(translation) with threshold(0.8)
```

`expect ... satisfies <judge>` grades the subject against a judge's rubric — the design intends this to resample/re-evaluate up to a configurable retry budget until the verdict converges, an auto-waiting assertion adapted to LLM non-determinism rather than grading a single sample once. **Today, the judge call happens exactly once, with no polling or retry-until-converged behavior** — the same way an ordinary `match judge Fluency(x) {...}` inside a conversation body would call it once. `expect ... settles within duration(...)` (the tool-call/async-effect analogue) is described in the full design but has no working implementation.

A `Pass` verdict always passes the check; `Fail(reason)` always fails with that reason; `Score(s)` passes iff `s` meets the `threshold` you gave (or is greater than `0.0` if you didn't give one); `Escalate` — a judge declining to decide — fails, since there's no human-in-the-loop resolution available inside a benchmark row.

## `assert` and `benchmark`

```ulexite
benchmark TranslateQuality {
  dataset: TranslationPairs
  run: Translate(source: $.source, target_lang: $.target_lang) -> result
  expect result satisfies judge Fluency(result) with threshold(0.8)
  assert result != $.golden
  snapshot result as """translate/{$.target_lang}"""
}
```

Inside a `benchmark` body, `$` refers to the current dataset row. `run: <call> -> name` invokes a conversation once per row and binds its result. `assert` is an ordinary boolean check — for the part of a test that genuinely is deterministic — evaluated the same as any other expression, with a clear "assertion failed (evaluated to ...)" message on failure. `assert` and `expect` mix freely in the same `benchmark` body, because both ultimately produce the same pass/fail check the reporter treats uniformly.

Run a benchmark with `ulx bench`:

```bash
ulx bench eval_translate.ulx TranslateQuality --provider anthropic
```

```bash
# Or fully offline, no provider configured:
ulx bench eval_translate.ulx TranslateQuality --mock
```

`ulx bench` prints a plain-text `PASS`/`FAIL` line per row plus a summary count, and exits non-zero if any row failed:

```text
row 0: PASS
row 1: FAIL
  - expect failed: score 0.62 below threshold 0.8
TranslateQuality: 1/2 row(s) passed
```

## `snapshot` — a real golden baseline, exact-equality today

```ulexite
snapshot result as """translate/{$.target_lang}"""
```

The full design describes `snapshot expr as "<key>"` as Playwright-style visual-regression testing adapted for text: recording a golden baseline on first run (or with `ulx bench --update-snapshots`), then comparing against it on subsequent runs via a **semantic** diff rather than a byte- or line-diff, since exact-match text comparison is far too brittle for genuinely non-deterministic model output.

**The baseline storage and comparison are real today; the diff is exact, not semantic.** The first time a `snapshot` statement runs for a given key, it writes the evaluated value to a JSON file under `<package-dir>/snapshots/<benchmark>/` (meant to be committed alongside your source, the same role a `.snap` file plays for `insta` or `__snapshots__/` plays for Jest) and passes as "recorded (new baseline)." Every later run compares the freshly-evaluated value against that stored baseline with plain `Value` equality — an identical value passes ("matches baseline"), anything else fails with both values printed. `ulx bench --update-snapshots` skips the comparison and unconditionally overwrites the baseline, for accepting an intentional change.

Because the comparison is exact rather than semantic, `snapshot` today suits a deterministic subexpression (a computed key, a structural transform) far better than raw `ask`/`judge` output, which will almost never come back byte-identical across a real provider call — reserve it for the parts of a benchmark row that genuinely shouldn't change, not the whole non-deterministic response.

The key must be a triple-quoted string (`"""..."""`) if you want it interpolated per row — a plain `"..."` string is a literal with no `{...}` substitution in Ulexite (interpolation is a text-block feature, §7.1), so every row would collide on the same literal key.

## Reporting and aggregation

The full design calls for `ulx test` to produce a structured JSON/JUnit-compatible report, with `metrics.pass_rate`/`metrics.percentile`-style aggregation available inside a `benchmark`'s own reporting block. **There is no `ulx test` command** — the real entry point is `ulx bench`, and its report is a plain in-memory pass/fail-per-row structure with no `metrics.*` aggregation and no JUnit/JSON output format. A row that hits a real `escalate(...)` mid-run does suspend gracefully rather than failing the whole benchmark — see [`ulx bench`](./tooling/cli-reference.md#ulx-bench) — but that's a different thing from `expect`'s own retry-until-converged polling (§16.3), which still isn't implemented.

Every ordinary conversation run still produces a full trace by default, and that's true of a run invoked via `ulx bench` too — so a failing row is debuggable with `ulx trace` the same way any other run is; see the [CLI Reference](./tooling/cli-reference.md).

## Fixtures and scoping

Reusable setup — a mock provider standing in for a real one during CI, a shared judge configuration — is meant to be declared as an ordinary importable value and scoped by where it's imported, the same way any other `judge`/`dataset`/`conversation` import works. There's no bespoke fixture-discovery mechanism (no `conftest.py`-equivalent auto-discovery) in the language; you get reuse purely through ordinary `import` statements, which already works today for `judge`/`conversation`/`dataset`/`provider` declarations.

## Determinism and CI reproducibility

Because every `ask`/`judge` call is content-addressed and cached by default, and a `dataset` is loaded and hashed consistently, re-running `ulx bench` against an unchanged program and dataset should be close to a full cache hit — a genuinely new run (changed program, changed dataset, or an explicit `--no-cache`-equivalent) is the case that actually spends tokens. This caching behavior is real and comes from the same runtime machinery every `ulx run` uses; it isn't something you have to opt into separately for benchmarks.

---

## Judges as calibrated, versioned instruments

Everything above is what you write and run today. Everything below this line — judge calibration, pairwise/comparative evaluation, and production shadow evaluation — describes the intended methodology on top of that grammar, and **none of it has a working CLI command yet**. There is no `ulx eval` subcommand at all in the current CLI (only `ulx bench`, alongside `parse`/`check`/`run`/`plan`/`approve`/`deny`/`replay`/`trace`/`init`/`manifest`/`fmt`).

The design's premise: a `judge` shouldn't be trusted by default. It's meant to be a versioned artifact you can evaluate against human-labeled ground truth, the same idea as a meta-eval pattern, made a standard, expected step rather than an optional afterthought:

```ulexite
dataset HumanLabeled: [{subject: text, human_verdict: Verdict}] {
  from "fixtures/human_labels.jsonl"
}

benchmark CalibrateFluencyJudge {
  dataset: HumanLabeled
  run: judge Fluency($.subject) -> model_verdict
  assert model_verdict == $.human_verdict
  expect metrics.agreement(model_verdict, $.human_verdict) satisfies threshold(0.85)
}
```

You can write and run the `benchmark` half of this today with `ulx bench` (modulo the `metrics.agreement` call, which isn't implemented — see [Standard Library](./standard-library.md)). The part that doesn't exist is `ulx eval calibrate Fluency`, the dedicated command the design describes for surfacing "this judge failed its own calibration benchmark" as a signal before that judge gates a `retry`/`escalate` decision in production code.

## Pairwise and comparative evaluation

`judge.pairwise(a, b, criteria)` is meant to support A/B regression testing across a full dataset: given a candidate conversation and a baseline (the previous released version, say), a `benchmark` could report win/loss/tie rates rather than only absolute pass/fail. `judge.pairwise` isn't implemented (see [Standard Library](./standard-library.md)), and there's no win-rate aggregation built into `ulx bench`'s report today.

## Production shadow evaluation

Because every ordinary conversation run already produces a full trace, the design's evaluation engine is meant to be pointable at a sample of production traces instead of a static `dataset` — `ulx eval shadow --judge Fluency --sample 0.05 --window 24h` re-running a judge against a random sample of real production runs, using the trace log itself as the dataset. This closes the loop between "the tests I wrote" and "what actually happened in production." **This is a future direction, not a real command** — there's no `ulx eval` subcommand, and no trace-sampling machinery behind it.

## Regression tracking over time

The design describes `ulx eval trend <benchmark>` plotting a metric's value across every past run of a benchmark, one point per commit/CI run, using each run's retained, content-addressed trace as the data source. Not implemented — there's no trend/history command, and no `trace.diff` stdlib function to semantically diff two runs against each other either.

## Human evaluation as a first-class dataset source

Because `human_approval` is an ordinary message kind and an `escalate`'s resolution is recorded in the trace (recorded today via `ulx approve`/`ulx deny`), the design imagines exporting a stream of real human approval/rejection decisions as a `dataset` (`dataset.from_traces(filter: ...)`) and feeding it back into calibration benchmarks like `CalibrateFluencyJudge` above. `dataset.from_traces` isn't implemented; today, an approval/denial's outcome is real and inspectable via `ulx trace`, but there's no automated path from a batch of past runs into a new `dataset` value.

## Cost-quality tradeoff evaluation

The design pairs `ulx plan`'s cost estimation (real and working today, for a single conversation/provider combination) with a benchmark's quality metrics, swept jointly across provider policies: `ulx eval sweep --benchmark TranslateQuality --provider-policy cheapest,balanced,best`. Not implemented — `ulx plan` estimates cost for one resolved set of providers at a time; there's no sweep command and no `cheapest`/`balanced`/`best` policy vocabulary wired up yet (see the `llm.pin`/`cheapest`/`fastest` gap in [Standard Library](./standard-library.md)).

## Evaluation engine reuse, not duplication

The design's structural claim is that shadow evaluation, calibration, sweeps, and ordinary benchmark testing should all lower to one evaluation engine driving the same execution engine against different input sources, rather than a second product bolted beside the orchestration runtime. Today, `ulx bench` genuinely does reuse the same interpreter and provider registry as `ulx run` — that part of the claim holds for the one real entry point that exists. The rest (`ulx eval calibrate`/`shadow`/`trend`/`sweep`) simply hasn't been built against that same engine yet.

For the full design rationale, see [§16 Testing Framework](https://github.com/JGalego/ulexite/tree/main/docs/spec/16-testing-framework.md) and [§17 Evaluation Framework](https://github.com/JGalego/ulexite/tree/main/docs/spec/17-evaluation-framework.md) of the spec.
