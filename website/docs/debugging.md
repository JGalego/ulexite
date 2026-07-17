---
title: Debugging
description: How to inspect a run today with ulx trace/replay/debug, and what the full time-travel/live-attach debugging model in the spec still needs built.
---

# Debugging

Because every run is checkpointed and traced by default, Ulexite's debugging story is designed around replaying a completed run's trace, not attaching to a live process. That's the right idea for a language where the interesting failures are usually a refused/rate-limited call or a judge that couldn't decide, rather than a segfault — but it's worth being direct about how much of that design is actually built.

**What you get today** is real: every `ulx run` produces a full trace, `ulx trace`/`ulx replay` let you inspect and reproduce it in detail (one line per capability call, in every output format from a plain listing to a Mermaid sequence diagram to a self-contained HTML page), and `ulx debug <run_id>` lets you interactively step through that same trace — forward and backward, with breakpoints, full artifact inspection, and a call-stack view for nested conversations. **What the spec describes beyond that** — `breakpoint()` as a language keyword suspending live interpretation, forking a run with modified inputs (`ulx fork`), and live-attaching to an in-flight conversation (`ulx attach`) — is a future direction. This page walks through both, clearly labeled.

## What's real: inspecting a run via its trace

The actual `ulx` command surface for inspecting a run is `ulx trace` and `ulx replay`, plus every run/approve/deny/replay command's `--output` flag:

```bash
ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav --output json
```

```bash
run_id=$(ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav --output json | jq -r .run_id)
ulx trace "$run_id" --output mermaid
ulx trace "$run_id" --output jsonl
ulx trace "$run_id" --output html > trace.html
```

`ulx trace` prints a completed run's trace log directly — one record per capability call, oldest first, marking cache status (`[miss]`/`[hit]`/`[err]`). `ulx replay` strictly replays a completed run from that same trace log: a cache miss during replay is an error, never a live provider call, so it's for reproducing a past run's exact dialogue and final value, not for continuing or retrying it.

A trace record carries the request (the actual `system`/`user` messages sent, or a judge's subject/rubric, or an escalation's reason), which provider and model actually served it, the resulting output or error, and a timestamp — everything you need to answer "what exactly did this call send, and what came back" after the fact. Here's a sample `jsonl` record:

```json
{"cache_hit":false,"capability":"chat","error":null,"input":[{"role":"system","text":"You write a one-sentence spoken reply to a voice memo."},{"role":"user","text":"Voice memo transcript:\n Hey! This is a quick voice memo about the quarterly report."}],"kind":"effect","output":"I've gone ahead and prepared a brief summary...","seq":1,"timestamp_ms":1784128850854}
```

And the same run as a Mermaid sequence diagram (`--output mermaid`), which you can paste straight into a Markdown/Mermaid renderer:

```mermaid
sequenceDiagram
    participant Program
    participant transcribe as transcribe
    participant chat as chat
    participant speak as speak
    Program->>+transcribe: #0 transcribe
    transcribe-->>-Program: [miss] Hey! This is a quick voice memo about the quarterly report...
    Program->>+chat: #1 chat
    chat-->>-Program: [miss] I've gone ahead and prepared a brief summary...
    Program->>+speak: #2 speak
    speak-->>-Program: [miss] .ulexite/artifacts/5e/d29d0f8cb4c24c.mp3
```

For a non-deterministic failure — a refused call, a rate limit, a judge that returned `Escalate` — the trace record's `output`/`error` field carries the exact typed value your program's own `match` would have seen, since these are ordinary typed values (a `Draft<T>`'s unsettled state, a `Verdict`), not exceptions with a stack to unwind. Reading the trace record for the failing step is, today, how you triage that failure — there's no dedicated inspector UI around it yet.

See the [CLI Reference](./tooling/cli-reference.md) for every `--output` format and the full flag list on `run`/`trace`/`replay`.

## `ulx debug <run_id>`: stepping through a trace interactively

`ulx debug <run_id>` loads a completed *or* suspended run's trace and drops you into an interactive session — a real debugger's mental model (step, breakpoints, inspect, call stack) applied to that recorded trace, rather than a live process:

```text
$ ulx debug demo
ulx debug: 2 record(s) for run `demo` — type `help` for commands
(ulx-debug) next
#0   [miss] Middle
(ulx-debug) stack
Middle (current)
(ulx-debug) next
#1   [miss] Leaf
(ulx-debug) stack
Middle > Leaf (current)
(ulx-debug) inspect
#1 [Leaf] kind=call cache=miss
input:
  x: hi
output: hi
timestamp_ms: 1784128850854
(ulx-debug) quit
```

Commands: `next`/`n` and `back`/`prev`/`p` step one record at a time (in either direction — `back` just re-displays an earlier record already held in memory, since this is a read-only stepper, not live re-execution); `break <seq>`/`bp <seq>` sets a breakpoint, and `continue`/`c` runs forward to the next one (or the end); `inspect`/`i`/`show` prints the current record's full, untruncated input/output/error (unlike `ulx trace`'s one-line-per-record table); `stack`/`where`/`w` prints the call-stack chain built from `parent_run_id` (see below); `list`/`l` lists every record; `help` and `quit` do what you'd expect.

If the run is suspended on a real `escalate(...)`, `ulx debug` opens with a banner naming the target and reason, plus the exact `ulx approve`/`ulx deny` command to resume it — it points you at that existing resume flow rather than reimplementing it.

## What the spec describes, not yet built

Section 19 of the spec lays out a considerably richer debugging model than the interactive stepper above. Here's each remaining piece, and what (if anything) stands in for it today.

### `breakpoint()` as a language construct

The spec describes a `breakpoint()` statement (and a conditional variant, `breakpoint(verdict is Fail)`) that would suspend interpretation at that IR node under `ulx run --debug` or during replay, exposing the current scope's bindings for inspection. **`breakpoint` isn't a keyword in the grammar at all** — there's no way to write one in a `.ulx` file today, and `ulx run` has no `--debug` flag.

### Time-travel and re-run-from-here (`ulx fork`)

Because every statement is meant to be a checkpoint, the spec describes jumping to any point in a trace and either inspecting state there or re-running from there with modified inputs, via `ulx fork`. **`ulx fork` doesn't exist.** `ulx debug`'s `back`/`next` do let you jump freely to any already-recorded point and inspect state there — the "jump to any point" half — but it's read-only: there's no way to re-run from that point with an edited input, since the stepper never re-invokes the interpreter. The closest thing to a real re-run capability is `ulx run --run-id <id>`, which lets you reuse a specific run id across separate invocations — useful for driving a suspend/resume flow deliberately, but not a way to branch a new variant off an arbitrary earlier point in an existing run.

### Root-cause navigation across nested conversations

The spec describes the debugger rendering nested conversations (one conversation calling another) as a navigable call stack, keyed off a `parent_run_id` carried by each trace record. **This part is real today.** Every `TraceRecord` carries `parent_run_id: Option<String>` — `None` at the top level, or the enclosing "call" record's own `{run_id}:{seq}` when produced while a nested conversation's body is executing (including across a `with` block's concurrently spawned branches). Both `ulx trace`'s default text output and `ulx debug`'s `stack` command render this as a call stack straight from the trace log:

```text
#0   [miss] Middle
#1   [miss]   Leaf
```

```text
(ulx-debug) stack
Middle > Leaf (current)
```

`--output json`/`jsonl` also include `parent_run_id` on every record, so a script or external tool can reconstruct the same tree. What's still missing from §19.4's full picture: nested calls still share one flat trace file/`run_id` rather than each getting its own genuinely separate, independently-replayable run.

### Live attach for in-flight conversations

The spec describes `ulx attach <run_id>` connecting to a live execution engine for a long-running or suspended conversation, showing the same view as replay debugging but against actual in-flight state — useful for inspecting a production conversation waiting on a human approval before deciding how to respond. **`ulx attach` doesn't exist.** `ulx debug` reads a suspended run's trace-so-far from disk, which is a real and useful view (including a startup banner naming what it's waiting on), but it isn't attached to anything actually running — there's no long-running execution engine to attach to in the first place. What's real for a suspended run beyond that is the asynchronous suspend/resume flow itself: `ulx run` prints a `suspended: ...` line with resume instructions, and a separate `ulx approve <run_id>`/`ulx deny <run_id>` invocation (possibly from a different terminal, possibly much later) resolves it.

### Debugger hooks for tool/provider authors

The spec describes tool and provider adapters registering debug-inspector callbacks exposed identically through `ulx debug`'s UI, so a third-party plugin (say, a vector-store provider) could expose its own "show me the retrieved candidates and their scores" panel without the core debugger needing to know anything about vector stores specifically. **There's no such extension point.** `ulx debug`'s command set (`next`/`back`/`break`/`inspect`/`stack`/`list`) is fixed in the binary; a plugin author has no way to register an additional panel or command into it today.

## What to actually reach for today

Given the above, the practical debugging loop looks like this:

1. Run with `--run-id` so you can find the run again: `ulx run main.ulx MyConv --arg x=1 --run-id debug-1 --mock`.
2. If it suspends, step through it with `ulx debug debug-1` (or `ulx trace debug-1` for the non-interactive listing), then resolve it with `ulx approve debug-1 --value ...` or `ulx deny debug-1`.
3. Otherwise, pull the full trace with `ulx trace debug-1 --output jsonl` (for grepping/`jq`) or `--output html` (for a readable, shareable page) — or step through it interactively with `ulx debug debug-1`.
4. Use `ulx replay debug-1` to reproduce the exact same dialogue and final value deterministically, from the cache — a cache miss there is itself a useful signal that something about the run's inputs or provider config changed since it was recorded.
5. Use `--output mermaid` when you want a shareable diagram of the call sequence rather than a line-by-line log.

For the full design rationale — including the parts not yet built — see [§19 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/19-debugging-model.md).
