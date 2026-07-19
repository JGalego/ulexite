# The simple format: Markdown → `.ulx`

`.ulx` is a real programming language, and reading §7-10 of [the spec](spec/00-index.md) is worth it once you're hooked. But the first file you ever write shouldn't require reading a grammar. This is a small, deliberately restricted Markdown dialect that every `.ulx`-consuming command — `run`, `check`, `bench`, `plan`, `fmt`, `replay`, `approve`/`deny` — accepts in place of `.ulx` source, no braces, no keywords, no type annotations, unless you actually want to use them.

The smallest possible version is a title and a paragraph:

```markdown
# Greet

Say hello to {name} and ask how their day is going.
```

```sh
ulx run greet.md Greet --arg name=world --mock
```

`{name}` is automatically picked up as a parameter — you never declare it. That's it; that's a whole working conversation, no separate compile step required.

Want the generated `.ulx` itself — to read it, commit it, or hand-edit it from there — `ulx from-md` still compiles it to a file instead of running it:

```sh
ulx from-md greet.md -o greet.ulx
```

## Sections

A file is a title, followed by whatever of these you need, in any order:

| Heading | Required? | Becomes |
|---|---|---|
| `# Title` | yes, exactly one | the conversation's name (`My Conversation` → `MyConversation`) |
| *(a paragraph right after the title, before any `##`)* | yes, unless `## Ask` is used instead | the message sent to the model |
| `## System` | no | the system prompt |
| `## Ask` (or `## User`) | no — the untitled paragraph above already covers this | the message sent to the model, if you'd rather use a heading than a bare paragraph |
| `## Judge` | no | turns on grading: see below |

Anything in `{curly braces}` in the title paragraph or `## System` becomes a `text` parameter, in the order it first appears. Two placeholders with the same name are one parameter.

## Adding a judge

Add a `## Judge` section and describe, in plain language, what a good answer looks like:

```markdown
# Translate

## System
You are a professional translator.

## Ask
Translate to {target_lang}: {source}

## Judge
Is this an accurate, fluent translation? Answer Pass or Fail(reason).
```

This compiles to the same shape [`examples/translate.ulx`](../examples/translate.ulx) hand-writes: a `judge` block with your rubric, and a `match` that returns the answer on `Pass`, retries once with the judge's feedback on `Fail`, and hands off to a human (`escalate(human_approval, ...)`) if the judge itself can't decide or the retry also fails. You don't write any of that — it's generated.

## The escape hatch: a `ulx-meta` code block

The defaults are: return type `text`, every parameter type `text`, and a conversation name derived from the title. The most common reason to override anything is a title that doesn't make a clean identifier — add a fenced TOML block anywhere in the file:

````markdown
# turn this into a haiku

Write a haiku inspired by: {theme}

```ulx-meta
name = "WriteHaiku"
```
````

`name` and `returns` are optional strings; `params` is an optional list of `{name, type}` that, when given, fully replaces auto-detection (so list every parameter the text references).

Be careful with `returns`/`type` beyond `text`, though: this generator only ever writes a plain `user: """...{param}..."""` message and a plain `assistant -> answer` step — the same shape regardless of what you declare. A non-`text` param is still just string-interpolated (so a `pdf` param would paste its file path into the prompt as literal text, never actually read the file — real PDF handling needs `ask vision(...)`/`pdf.extract_text`, which this format doesn't generate), and a non-`text` `returns` doesn't get you a coerced or validated value, just a label. For that, see [`examples/summarize.ulx`](../examples/summarize.ulx)/[`examples/pdf_qa.ulx`](../examples/pdf_qa.ulx) and hand-write `.ulx`.

## A fuller worked example

[`examples/reply_to_review.md`](../examples/reply_to_review.md) uses all three sections — system, ask, and judge — for a customer-support reply generator, and is checked into CI the same way every `.ulx` example is (`just check-examples`).

## Limitations

This is a prototype covering one conversation per file, one `ask` + optional `judge`, nothing else — no imports, `provider` blocks, datasets, benchmarks, multi-step tool calls, or nested conversations. For any of that, drop into real `.ulx` — see [`docs/spec/07-recommended-syntax.md`](spec/07-recommended-syntax.md) and [`examples/`](../examples/). Literal `{`/`}` or `"""` in your prose will confuse the compiler; there's no escaping for them yet.

Whether you run a `.md` file directly or go through `ulx from-md`, the generated `.ulx` is always re-parsed before it's used, so a bug here fails loudly as an "internal error" rather than handing you `.ulx` that silently doesn't compile.
