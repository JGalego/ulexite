---
title: Editor Support
description: What the ulx-lsp language server actually does today, and how to wire it into your editor.
---

# Editor Support

`ulx-lsp` is a standard [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) implementation for Ulexite, built directly on the same compiler stages (`ulx-syntax`, `ulx-sema`) the `ulx` CLI uses. It speaks LSP over stdio, so it works with any LSP-capable editor, not just VS Code.

## What's implemented today

- **Diagnostics** — live as you type (a fast, single-file parse + semantic check on every keystroke), plus a fuller cross-file check (with imports resolved) on file open/save, mirroring what `ulx check` reports from the command line.
- **Hover** — shows a declaration's signature and doc comment for conversations, judges, validators, datasets, types, and providers; shows a capability's `accepts`/`produces` types when hovering an `ask <capability>` call.
- **Go to definition** — works for references within a file and across `import`s into other files.
- **Document symbols** — an outline view listing every top-level declaration in the current file.
- **Completion** — declared names in scope, stdlib capability and module names, artifact-type keywords, and grammar keywords.

Go to definition and the outline view both land precisely on a declaration's name token — `conversation Foo(...) { ... }` jumps the cursor to just `Foo`, not the whole multi-line body — the same way a reference *inside* a body (a call, a type reference) already resolved precisely. Every top-level declaration's name (and every parameter's) carries its own span, separate from the whole declaration's span.

## What's not implemented yet

The full IDE-integration vision (see [§20 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/20-ide-integration.md)) describes more than what's built today:

- **Trace-viewer code lens** — a "jump to trace" link above every `conversation`/`judge`/`benchmark` declaration, filtered to recent runs. Not built; today you'd separately run `ulx trace <run-id>`.
- **Live-run attach** — hovering a binding showing the actual runtime artifact from a recent or in-progress `ulx run --debug` session. Not built.
- **Incremental recompilation** — the spec envisions keystroke-level edits re-checking only the affected AST/IR subtree. Today, `ulx-lsp` re-parses and re-checks the whole file on every change — acceptable for how small these scripts are in practice, but not true incrementality.
- **Extra lints** beyond the hard compile errors — unused-output detection, unreachable-branch detection beyond `Verdict` exhaustiveness, untrusted-FFI-validator flags, uncalibrated-judge warnings. None of these exist yet.

## Installing the server

```bash
cargo install --git https://github.com/JGalego/ulexite ulx-lsp --locked
```

Or build it from a local clone:

```bash
cargo build --release -p ulx-lsp
# binary lands at target/release/ulx-lsp
```

Either way, make sure `ulx-lsp` ends up on your `PATH` — most editor integrations resolve the server binary by name.

## VS Code / VSCodium / Cursor / Windsurf

Install the "Ulexite" extension from the Marketplace/Open VSX, or install the `.vsix` from a [release](https://github.com/JGalego/ulexite/releases/latest) directly. It bundles syntax highlighting and a `vscode-languageclient` client that spawns `ulx-lsp`.

If the binary isn't on `PATH`, set the `ulexite.serverPath` setting to its full path (VS Code settings UI, or `.vscode/settings.json`):

```json
{
  "ulexite.serverPath": "/full/path/to/ulx-lsp"
}
```

## Other editors

Since `ulx-lsp` is a plain stdio LSP server, any generic LSP client works. For Neovim's built-in client, something like:

```lua
vim.api.nvim_create_autocmd('FileType', {
  pattern = 'ulexite', -- register .ulx -> ulexite filetype separately
  callback = function()
    vim.lsp.start({
      name = 'ulx-lsp',
      cmd = { '/full/path/to/ulx-lsp' },
      root_dir = vim.fs.dirname(vim.fs.find({ 'ulexite.toml', '.git' }, { upward = true })[1]),
    })
  end,
})
```

Any other editor with a configurable LSP client (Helix, Emacs's `eglot`/`lsp-mode`, Zed, Sublime's LSP package) should work the same way: point it at the `ulx-lsp` binary over stdio.
