# Ulexite VS Code extension

TextMate-grammar syntax highlighting plus a `vscode-languageclient` client for `ulx-lsp` (the language server): hover, go-to-definition, document symbols, and completion for `.ulx` files.

## Get the language server

The extension doesn't bundle `ulx-lsp` — it execs the binary by name off `PATH`. The easiest way to get it is the same install as the CLI, which installs both together:

```sh
# Linux / macOS (x86_64 or arm64)
curl -fsSL https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.sh | sh

# Windows (x86_64), in PowerShell
irm https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.ps1 | iex
```

Building from source works too: `cargo build --release -p ulx-lsp` (binary lands at `target/release/ulx-lsp`).

If it's not on `PATH`, set the `ulexite.serverPath` setting (in VS Code's settings UI, or `.vscode/settings.json`) to the binary's full path.

## Try it locally

```sh
cd tooling/vscode-ulx
npm install
npm run compile
npx --yes @vscode/vsce package   # produces ulexite-<version>.vsix
code --install-extension ulexite-*.vsix
```

Or just symlink/copy this folder (after `npm install && npm run compile`, so `out/` exists) into your VS Code extensions directory (`~/.vscode/extensions/ulexite-dev`) and reload the window.

## What it highlights

- Line (`//`) and doc (`///`) comments, block comments (`/* */`)
- Declaration keywords (`conversation`, `judge`, `validator`, `dataset`, `type`, `provider`, `benchmark`, `import`)
- Control keywords (`with`, `ask`, `match`, `retry`, `escalate`, `for`, `while`, `if`, `else`, ...)
- Message roles (`system`, `user`, `assistant`)
- The fourteen artifact-type keywords (`text`, `image`, `pdf`, ...)
- Triple-quoted text blocks, with `{interpolation}` spans highlighted as embedded code
- Capitalized identifiers as type/variant names (`Verdict`, `Pass`, `Fail`, ...)
- Function/capability calls, numbers, operators, punctuation
- Field/property names and named-arg labels (`vendor:`, `rubric:`, `ask chat(temperature: 0.7)`) — anything shaped like `identifier:`
