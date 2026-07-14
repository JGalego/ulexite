# 17. Evaluation Framework

§16 defines the *syntax* developers write (`expect`/`assert`/`snapshot`/`benchmark`); this section defines the *methodology and runtime machinery* behind judges, calibration, and production evaluation — the part of the ecosystem gap analysis (§3.2) that today lives entirely outside any orchestration language, in a separate product (LangSmith, Promptfoo, OpenAI Evals) with no view into the program's own types.

## 17.1 Judges as calibrated, versioned instruments

A `judge` (§7.2, §5.6) is not trusted by default — it is a versioned artifact (§14.4) that can itself be evaluated against human-labeled ground truth, adapting OpenAI Evals' meta-eval pattern (§2.2) and `judge.meta` (§15.2) as a standard, expected step rather than an optional afterthought:

```
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

A judge failing its own calibration benchmark is a signal the compiler/CLI surfaces (`ulx eval calibrate Fluency`) before that judge is trusted to gate a `retry`/`escalate` decision in production code — closing the gap where every surveyed framework's LLM-as-judge feature (Promptfoo's `llm-rubric`, OpenAI Evals' `ModelBasedClassify`) is used unvalidated by default (§2.2).

## 17.2 Pairwise and comparative evaluation

`judge.pairwise(a, b, criteria)` (§15.2) supports A/B regression testing across a full dataset: given a candidate conversation and a baseline (e.g., the previous released version, resolvable via `dataset.split`/package version pinning, §14.4), a `benchmark` can report win/loss/tie rates rather than only absolute pass/fail — the comparative-evaluation pattern every surveyed system leaves to bespoke scripting, made a native aggregation (`metrics.pass_rate` and win-rate variants, §15.15).

## 17.3 Production shadow evaluation

Because every ordinary conversation run already produces a full trace (§10.4, §12.5), the same evaluation engine (§12.8) that drives `benchmark` can be pointed at a sample of production traces instead of a static `dataset`: `ulx eval shadow --judge Fluency --sample 0.05 --window 24h` re-runs a judge against a random sample of real production runs, using the trace log as the dataset — closing the loop between "the tests I wrote" and "what actually happened in production," which none of the frameworks in §2.2/§2.3 connect natively (LangSmith's online evaluation is the closest analogue, and it is a separate paid product operating on traces LangChain/LangGraph export to it, not a feature of the orchestration language itself).

## 17.4 Regression tracking over time

`trace.diff` (§15.8) plus a benchmark's historical run records let `ulx eval trend <benchmark>` plot a metric's value across every past run of that benchmark (one point per commit/CI run, since every run's trace is content-addressed and retained per §12.5/§18) — a first-class regression dashboard input rather than a bespoke internal tool every team building on the frameworks in §2 ends up assembling by hand from CSV exports.

## 17.5 Human evaluation as a first-class dataset source

Because `human_approval` is an ordinary message kind (§5.2) and an escalation's resolution is recorded in the trace (§10.4), a stream of real human approval/rejection decisions can itself be exported as a `dataset` (`dataset.from_traces(filter: ...)`, §15.9) and fed back into `CalibrateFluencyJudge`-style benchmarks (§17.1) — human-in-the-loop feedback and judge calibration share one data path instead of being two disconnected systems (a support/ops tool for approvals, a separate eval harness for judges) the way they are in every framework surveyed.

## 17.6 Cost-quality tradeoff evaluation

`ulx plan`'s cost estimation (§10.5) and a `benchmark`'s quality metrics (§16.6) can be swept jointly: `ulx eval sweep --benchmark TranslateQuality --provider-policy cheapest,balanced,best` runs the same benchmark under different provider-resolution policies (§12.4) and reports a quality-vs-cost curve — directly operationalizing §5.5/§12.4's provider-independence guarantee as an evaluation tool, not just a runtime convenience: switching providers is not only *possible* without a rewrite, its quality/cost consequence is *measurable* without a rewrite either.

## 17.7 Evaluation engine reuse, not duplication

Per §12.8, none of the above requires a second execution model: shadow evaluation, calibration, sweeps, and ordinary `ulx test` all lower to the same evaluation engine driving the same execution engine (§12.2) against different input sources (a static `dataset`, a sampled trace stream, a policy sweep). This is the structural answer to §2.2 and §2.3's shared failure: every surveyed system's eval tooling is architecturally a second product bolted beside the orchestration runtime, not a mode of the same one.
