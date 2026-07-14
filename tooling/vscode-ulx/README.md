# Ulexite VS Code extension

TextMate-grammar syntax highlighting (§20.10) plus a `vscode-languageclient` client for `ulx-lsp` (§20.2's language server — see [`docs/spec/20-ide-integration.md`](../../docs/spec/20-ide-integration.md)): diagnostics, hover, go-to-definition, document symbols, and completion for `.ulx` files.

## Build the language server

The extension doesn't bundle `ulx-lsp` — build it once from the repo root and either put it on `PATH` or point the extension at it:

```sh
cargo build --release -p ulx-lsp
# binary lands at target/release/ulx-lsp
```

If it's not on `PATH`, set the `ulexite.serverPath` setting (in VS Code's settings UI, or `.vscode/settings.json`) to the built binary's full path.

## Try it locally

```sh
cd tooling/vscode-ulx
npm install
npm run compile
npx --yes @vscode/vsce package   # produces ulexite-0.0.1.vsix
code --install-extension ulexite-0.0.1.vsix
```

Or just symlink/copy this folder (after `npm install && npm run compile`, so `out/` exists) into your VS Code extensions directory (`~/.vscode/extensions/ulexite-dev`) and reload the window.

## What it highlights

- Line (`//`) and doc (`///`) comments, block comments (`/* */`)
- Declaration keywords (`conversation`, `judge`, `validator`, `dataset`, `type`, `benchmark`, `import`)
- Control keywords (`with`, `ask`, `match`, `retry`, `escalate`, `for`, `while`, `if`, `else`, ...)
- Message roles (`system`, `user`, `assistant`)
- The fourteen artifact-type keywords (`text`, `image`, `pdf`, ...)
- Triple-quoted text blocks, with `{interpolation}` spans highlighted as embedded code
- Capitalized identifiers as type/variant names (`Verdict`, `Pass`, `Fail`, ...)
- Function/capability calls, numbers, operators, punctuation
