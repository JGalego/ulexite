# Ulexite VS Code extension (syntax highlighting)

Minimal TextMate-grammar-only extension for `.ulx` files (§20.10 of the language spec). No language server yet — see [`docs/spec/20-ide-integration.md`](../../docs/spec/20-ide-integration.md) for the planned `ulx-lsp`.

## Try it locally

```sh
cd tooling/vscode-ulx
npx --yes @vscode/vsce package   # produces ulexite-0.0.1.vsix
code --install-extension ulexite-0.0.1.vsix
```

Or just symlink/copy this folder into your VS Code extensions directory (`~/.vscode/extensions/ulexite-dev`) and reload the window.

## What it highlights

- Line (`//`) and doc (`///`) comments, block comments (`/* */`)
- Declaration keywords (`conversation`, `judge`, `validator`, `dataset`, `type`, `benchmark`, `import`)
- Control keywords (`with`, `ask`, `match`, `retry`, `escalate`, `for`, `while`, `if`, `else`, ...)
- Message roles (`system`, `user`, `assistant`)
- The fourteen artifact-type keywords (`text`, `image`, `pdf`, ...)
- Triple-quoted text blocks, with `{interpolation}` spans highlighted as embedded code
- Capitalized identifiers as type/variant names (`Verdict`, `Pass`, `Fail`, ...)
- Function/capability calls, numbers, operators, punctuation
