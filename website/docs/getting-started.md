---
title: Getting Started
description: Install the ulx CLI and language server, then run your first conversation.
---

# Getting Started

## Install

**📦 Prebuilt binaries** — detects your OS/architecture automatically and installs both `ulx` (the CLI) and `ulx-lsp` (the language server, so an editor extension works immediately with no separate step):

```bash
# 🐧 Linux / 🍎 macOS (x86_64 or arm64)
curl -fsSL https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.sh | sh
```

```bash
# 🪟 Windows (x86_64), in PowerShell
irm https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.ps1 | iex
```

**🦀 From source** (needs Rust):

```bash
cargo install --git https://github.com/JGalego/ulexite ulx-cli --locked
cargo install --git https://github.com/JGalego/ulexite ulx-lsp --locked   # only needed for editor support
```

**🧩 Editor extension** (VS Code / VSCodium / Cursor / Windsurf) — syntax highlighting plus hover, go-to-definition, document symbols, and completion via `ulx-lsp`:

- Search the Marketplace/Open VSX for **"Ulexite"** and install, or
- Grab the `.vsix` from the [latest release](https://github.com/JGalego/ulexite/releases/latest) and run `code --install-extension ulexite-*.vsix` (substitute `code` for `cursor`/`windsurf`/`codium` as needed).

See [Editor Support](./tooling/editor-support.md) for what the language server can currently do for you.

## Your first conversation

Scaffold a package and run it against a real provider — `ulx init` leaves `ulexite.toml`'s `[providers.*]` table empty, so add one:

```bash
ulx init my-first-package /tmp/my-first-package
cd /tmp/my-first-package
ulx check main.ulx
```

```bash
cat >> ulexite.toml <<'EOF'

[providers.anthropic]
vendor = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
chat = "claude-haiku-4-5-20251001"
EOF
export ANTHROPIC_API_KEY=sk-ant-...
ulx run main.ulx Hello --arg name=world
```

Or drive one of the shipped examples instead — `voice_memo.ulx` declares its own `provider` blocks right in the source, so no `ulexite.toml` is needed at all:

```bash
git clone https://github.com/JGalego/ulexite && cd ulexite/examples
export GROQ_API_KEY=gsk_...
export OPENAI_API_KEY=sk-...
ulx run voice_memo.ulx VoiceMemoReply --arg recording=fixtures/sample.wav
```

See the [Examples gallery](./examples/index.md) for the full set of shipped programs and what each one demonstrates.

## Try it fully offline

Every real provider call goes through `--mock` if you'd rather not spend API budget while you're learning the language — `--mock` swaps in a deterministic, offline provider that never makes a network call, so every example below runs with no API key at all.

This one demonstrates a full human-approval suspend/resume round trip: the conversation runs, a judge can't decide, it escalates to a human, and a separate command resumes it.

```bash
ulx run translate.ulx Translate --arg source="MOCK_JUDGE_ESCALATE please" --arg target_lang=fr --run-id demo --mock
ulx approve demo --value "human said: ship it"   # reuses the run's --mock automatically
ulx trace demo
```

`ulx run` suspends on the judge's `Escalate` verdict; `ulx approve` resumes and completes it. Both print a dialogue transcript followed by a small metadata footer:

```text
🧭 system: You are a professional translator.

🧑 user: Translate to fr: MOCK_JUDGE_ESCALATE please

🤖 assistant: [mock:chat] response to -> system: You are a professional translator. | user: Translate to fr: MOCK_JUDGE_ESCALATE please

⚖️  judge Fluency: Escalate

🙋 escalate human_approval: judge could not decide (suspended)

suspended: waiting on `human_approval` — judge could not decide
────────────────────────────────────────────
run id        demo
status        suspended
capabilities  chat, judge, escalate
provider      mock — chat, judge
resume with: ulx approve demo --value <text>   (or: ulx deny demo)
```

```text
$ ulx approve demo --value "human said: ship it"
⚖️  judge Fluency: Escalate

🙋 escalate human_approval: judge could not decide => human said: ship it

human said: ship it
────────────────────────────────────────────
run id        demo
status        ok
capabilities  chat, judge, escalate
provider      mock — chat, judge
```

`ulx trace demo` replays every record from the run's trace log instead of executing anything — one line per capability call, oldest first, `[miss]`/`[hit]`/`[err ]` marking cache status. Or skip the two-step approval flow entirely and answer at the terminal live, with `--interactive`:

```bash
ulx run translate.ulx Translate --arg source="MOCK_JUDGE_ESCALATE please" --arg target_lang=fr --mock --interactive
```

## What's next

- [Core Concepts](./core-concepts.md) — conversations, messages, artifacts, and judges, syntax-independent.
- [Language Syntax](./language/syntax.md) — every declaration kind, in detail.
- [Examples gallery](./examples/index.md) — twelve complete, runnable programs.
- [CLI Reference](./tooling/cli-reference.md) — every `ulx` subcommand.
- [Playground](/playground) — edit, check, and actually run a conversation against a real local model, live in your browser, no install needed.
