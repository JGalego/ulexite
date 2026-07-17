---
title: CLI Reference
description: Every ulx subcommand, its flags, and a realistic invocation.
---

# CLI Reference

`ulx` is the Ulexite command-line tool: it parses and checks `.ulx` source, runs conversations against a real provider or the deterministic mock, drives the human-approval suspend/resume flow, and inspects a run's trace log. This page documents every subcommand in the `ulx` binary, in the order they're most useful to a newcomer.

Every subcommand returns a non-zero exit code on failure (a parse error, a failed check, a run that errors or suspends, an `ulx fmt --check` that would reformat, and so on), so `ulx <subcommand> ... && echo ok` composes the way you'd expect in a script or CI job.

## `ulx parse`

Parses a `.ulx` file and reports success or syntax errors. This is the fastest sanity check available — it only runs the lexer and parser, with no semantic analysis (no name resolution, no type checking).

```bash
ulx parse main.ulx
```

On success it prints a one-line summary (`OK: N import(s), M declaration(s)`); on a syntax error it prints one diagnostic per error, with the offending span underlined in the source.

## `ulx check`

Parses the file and then runs full semantic analysis across it and everything it imports: name/import resolution, artifact-type checking, `Verdict` exhaustiveness, and `with`-block independence checking. This is what an editor's "Problems" panel is built on (the same analysis is exposed live through `ulx-lsp`, the language server).

```bash
ulx check main.ulx
```

Run this before `ulx run` while iterating — it catches type and structural errors that would otherwise only surface once the runtime is actually executing (and possibly after a real provider call has already been made).

## `ulx run`

Runs a conversation to completion, or until it suspends on an `escalate(...)`. This is the main way you execute a `.ulx` program.

```
ulx run <file> <conversation> [--arg NAME=VALUE]... [--run-id ID] [--provider NAME]... [--mock]
         [--output text|plain|json|jsonl|mermaid|html] [--interactive] [--no-cache]
```

Key flags:

- **`--arg NAME=VALUE`** — repeatable, binds one conversation parameter per flag, e.g. `--arg name=world`.
- **`--mock`** — forces the deterministic, offline mock provider, ignoring any configured real providers. `ulx run` errors if no provider is configured at all, so pass `--mock` while learning the language or writing tests that shouldn't spend API budget. Conflicts with `--provider`.
- **`--provider NAME`** — repeatable; selects a specific configured provider by name (a `.ulx` `provider` decl or a `ulexite.toml` `[providers.<name>]` entry). Only the named provider(s) are registered, which resolves an otherwise-ambiguous capability (e.g. two configured vendors both serving `chat`) unambiguously.
- **`--run-id ID`** — reuses a specific run id instead of generating a fresh one. This is the only way to get a *stable*, reproducible run id across separate invocations — needed if you intend to `ulx approve`/`ulx deny`/`ulx trace` a suspended run later, since without it every invocation (even with identical file/conversation/args) gets its own unique id.
- **`--output FORMAT`** — `text` (default; a colorized/emoji dialogue transcript when stdout is a terminal), `plain` (the same transcript with no color/emoji/spacing), `json`, `jsonl`, `mermaid`, or `html`. The last four render the run's full trace, not just its final value. Ignored when `--interactive` is set.
- **`--interactive`** — prompts you at the terminal for each `escalate(...)` suspension instead of stopping and printing `ulx approve`/`ulx deny` instructions. This is the synchronous alternative to the suspend/resume flow, for a human sitting at the terminal right now. Always reports in plain text.
- **`--no-cache`** — skips the cache for `ask`/`judge` calls, forcing a fresh live call every time — useful when iterating on a prompt or rubric under the same `--run-id`/args, where a stale cache hit would otherwise hide the change. Never bypasses `escalate`'s own cache entry, since that's the persistence mechanism `ulx approve`/`ulx deny`/`--interactive` rely on.

```bash
ulx run main.ulx Hello --arg name=world
```

```bash
# Force the offline mock provider and pin a stable run id, so a later
# `ulx approve`/`ulx trace` can find this exact run again.
ulx run translate.ulx Translate --arg source="MOCK_JUDGE_ESCALATE please" \
  --arg target_lang=fr --run-id demo --mock
```

```bash
# Disambiguate between multiple configured vendors, and render the whole
# trace as a Mermaid sequence diagram instead of the dialogue transcript.
ulx run rag.ulx Caption --arg photo=fixtures/sample.jpg --output mermaid
```

## `ulx bench`

Runs a `benchmark` declaration to completion: loads its `dataset:`, runs the benchmark body once per row, and prints a plain-text pass/fail report. Like `ulx run`, it errors if no provider is configured — pass `--mock` to use the offline mock instead.

```
ulx bench <file> <benchmark> [--run-id ID] [--provider NAME]... [--mock] [--update-snapshots]
```

```bash
ulx bench eval_translate.ulx TranslateQuality --provider anthropic
```

- **`--run-id ID`** — reuses a specific run id, the same way `ulx run --run-id` does. Needed to resume a row that suspends on a human-approval `escalate(...)` — without it, the report still shows which rows are pending, but there's no stable id for `ulx approve`/`ulx deny` to resume by.
- **`--update-snapshots`** — unconditionally overwrites every `snapshot` statement's stored golden baseline with this run's freshly-evaluated value, instead of comparing against it. Use it once, deliberately, when a `snapshot` failure reflects an intentional change to the benchmark's expected output.

A row that hits a real `escalate(...)` (not a judge's own `Escalate` verdict, which is just an ordinary failed `expect` check) suspends gracefully rather than aborting the whole benchmark — the other rows still run and report normally. `ulx approve <run-id> --value ...`/`ulx deny <run-id>` resolves the first still-suspended row and re-runs; call it again with the same id if more than one row is pending.

`snapshot expr as "<key>"` records a golden baseline on first run (one JSON file per key, under `<package-dir>/snapshots/<benchmark>/`, meant to be committed alongside the source) and compares against it on every later run — a real regression gate, not just a recorded artifact. Note the comparison is exact `Value` equality, not §16.5's semantic diff, so it suits deterministic subexpressions better than raw non-deterministic model output. Also note the scope is still narrower than the full spec's `benchmark` design in other ways: there's no `expect`-polling/retry-until-converged, and no `metrics.*` aggregation or JUnit/JSON report — just a plain-text per-row result.

## `ulx plan`

Statically estimates which providers/models a conversation will call and a rough token/cost range, without executing anything — no provider is ever called for real, live or mocked (there's deliberately no `--mock` flag here, since forcing the mock provider would only hide what a real `ulx run` will actually resolve to).

```
ulx plan <file> <conversation> [--arg NAME=VALUE]... [--provider NAME]...
```

```bash
ulx plan translate.ulx Translate --arg source=hello --arg target_lang=fr --provider anthropic
```

`--arg` here only sharpens the token-length estimate for a parameter you pin (e.g. a long `source` string changes the estimate), it never causes anything to actually run. Treat the cost figures as a rough, illustrative estimate, not a billing-accurate forecast.

## `ulx approve`

Records a human decision for a suspended run and resumes it. This is one half of the suspend/resume flow that `escalate(...)` triggers.

```
ulx approve <run_id> [--value TEXT] [--provider NAME]... [--mock] [--output FORMAT]
```

- **`--value TEXT`** — the value to resolve the `escalate(...)` expression to. Defaults to `"approved"`.
- Provider selection defaults to whatever the original `ulx run` used (persisted in the run's manifest), so in the common case you don't need to repeat `--provider`/`--mock` — an explicit flag here still overrides it.

```bash
ulx approve demo --value "human said: ship it"
```

## `ulx deny`

Records a denial for a suspended run. Unlike `ulx approve`, this does not let execution continue successfully — the denial is recorded as the `escalate`'s resolved value (as a failing `Verdict`), the same mechanism an approval uses, just with a failure outcome.

```
ulx deny <run_id> [--note TEXT] [--provider NAME]... [--mock] [--output FORMAT]
```

```bash
ulx deny demo --note "insufficient justification for the refund"
```

`--note` is optional and defaults to `"denied by human reviewer"`. Note that v0.1 does not distinguish "deny and abort" from "deny with a value" at the type level — the denial is recorded as the resolved value, the same way an approval would be.

## `ulx replay`

Strictly replays a completed run from its trace log. A cache miss during replay is an error, never a live provider call — this is for reproducing a past run's exact dialogue and final value, not for continuing or retrying it.

```
ulx replay <run_id> [--output FORMAT]
```

```bash
ulx replay demo --output json
```

## `ulx trace`

Prints a run's trace log — one line per capability call, oldest first, marking cache status (`[miss]`/`[hit ]`/`[err ]`) in the default text format.

```
ulx trace <run_id> [--output text|json|jsonl|mermaid|html]
```

```bash
ulx trace demo
```

```bash
# Render the trace as a shareable Mermaid sequence diagram or a
# self-contained HTML page instead of the plain per-call listing.
ulx trace demo --output mermaid
ulx trace demo --output html > trace.html
```

A common pattern is chaining a `--output json` run straight into `ulx trace`, since the JSON output always carries the `run_id`:

```bash
run_id=$(ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav --output json | jq -r .run_id)
ulx trace "$run_id" --output mermaid
```

## `ulx init`

Scaffolds a new package: a `ulexite.toml` manifest plus a starter `main.ulx` conversation, in the given directory (created if it doesn't exist).

```
ulx init <name> [dir]
```

`dir` defaults to `.` (the current directory) if omitted.

```bash
ulx init my-first-package /tmp/my-first-package
```

The scaffolded `ulexite.toml` has an empty `[providers.*]` table — you'll need to add a provider (or pass `--mock` on `ulx run`) before the generated `Hello` conversation can actually run against a real vendor.

## `ulx manifest`

Parses, validates, and prints a `ulexite.toml` package manifest: the package name/version/required-`ulexite`-version, dependencies, configured providers and their capabilities, and runtime settings (concurrency, cache backend).

```
ulx manifest [file]
```

`file` defaults to `ulexite.toml` in the current directory if omitted.

```bash
ulx manifest
```

```bash
ulx manifest path/to/other/ulexite.toml
```

## `ulx fmt`

Reformats a `.ulx` file to canonical style. This is an AST-based pretty-printer, **not** a lossless/comment-preserving formatter — running it drops comments from the file. The formatter guarantees its own output always re-parses; if it doesn't, that's treated as an internal formatter bug and nothing is written.

```
ulx fmt <file> [--check]
```

- **`--check`** — don't write anything; exit non-zero if the file isn't already in canonical form. Mirrors `cargo fmt --check`/`gofmt -l`, so it composes well as a CI gate.

```bash
ulx fmt main.ulx
```

```bash
# CI-friendly: fail the build if any file needs reformatting.
ulx fmt main.ulx --check
```

## Output formats, summarized

`run`, `approve`, `deny`, `replay`, and `trace` all accept `--output`, defaulting to `text`:

| Format | What it renders |
|---|---|
| `text` | The dialogue transcript (role emoji, color when stdout is a terminal) plus a final value/`suspended`/`error` line and a metadata footer. |
| `plain` | The same transcript and final-value/suspended/error lines as `text`, with no color, emoji, or blank-line spacing — for scripts and logs. |
| `json` | One JSON object, always on stdout, always carrying `run_id` — so a script can chain straight into `ulx trace` without tracking the id separately. |
| `jsonl` | One JSON line per trace record, newline-delimited — the whole run's trace, not just the final value. |
| `mermaid` | A `sequenceDiagram` of the run's trace, ready to paste into a Markdown/Mermaid renderer. |
| `html` | A self-contained page rendering the trace as status-colored cards. |

`jsonl`/`mermaid`/`html` always describe the *whole* trace, even when produced via `run`/`approve`/`deny`/`replay` rather than `trace` directly. Errors that occur before a conversation starts running at all (an unreadable file, an ambiguous or unconfigured provider, a malformed `--arg`) are always plain text on stderr, regardless of `--output`.

`ulx bench` and `ulx plan` don't take `--output` — they always print their own plain-text report.
