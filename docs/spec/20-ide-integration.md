# 20. IDE Integration

## 20.1 Why this is possible at all

Every capability in this section depends on §8's grammar and §9's type system being real — a language server can only offer artifact-field autocomplete, exhaustiveness warnings, or unreachable-branch detection because the compiler has an actual AST, a real type checker, and a closed set of union variants to reason about (§13.3). None of the systems in §2.3 can offer this, structurally: LangGraph's state is a runtime-checked Pydantic model with documented validation gaps (§2.3, langgraph#1977); LlamaIndex's event routing is verified only at first-run, not by a static type-checker (§2.3); LangChain's prompt templates are plain strings with no static link to the parser that will consume the model's output (§2.3). A language server for any of them can, at best, offer host-language (Python/TS) completions — never anything that understands "artifact," "capability," or "Verdict," because those concepts don't exist in the host language's own type system.

## 20.2 Language server

`ulx-lsp` implements the standard Language Server Protocol, so it works unmodified in VS Code, JetBrains, Neovim, Zed, and any other LSP-capable editor — reusing an existing protocol rather than inventing a new IDE integration surface per editor, mirroring `rust-analyzer`'s approach (§13.1). It is built directly on the compiler's incremental pipeline (§13.7): keystroke-level edits re-run only the affected AST/IR subtrees, giving sub-second diagnostics even in large multi-file packages (§14).

## 20.3 Autocomplete

- **Artifact field completion**: typing `translation.` after a binding of type `text` — or a `record_type` — completes with exactly that type's declared fields (§9.2), because the compiler's typed AST has already resolved the binding's static type.
- **Capability completion**: typing `ask ` completes with every capability kind in scope (stdlib §15.1 plus any imported custom capability), each annotated with its `accepts`/`produces` types (§9.6) inline in the completion item.
- **Judge/validator/dataset completion**: imported values (§7.7) autocomplete with their declared signature shown, the same as a function signature would in a general-purpose language's IDE.

## 20.4 Artifact inspection

Hovering a binding shows its full type, including nested record/union structure, plus — when a language-server session is attached to a recent `ulx run` (§20.8) — the actual runtime content hash and a preview of the artifact's value (rendered inline: an `image` artifact shows a thumbnail, a `json` artifact shows formatted JSON, a `pdf` shows a page-count and first-page preview) directly in the editor, closing the gap where every framework in §2.3 requires leaving the editor entirely (to a Jupyter cell, a LangSmith trace page, or a print statement) to see what a step actually produced.

## 20.5 Trace navigation

`ulx-lsp` exposes a "jump to conversation" / "jump to trace" code lens above every `conversation`/`judge`/`benchmark` declaration, linking directly into the trace viewer (§20.6) filtered to runs of that declaration — collapsing the "find the right dashboard, find the right trace ID, find the right span" multi-tool hop documented against LangSmith/LlamaTrace-style external tooling (§2.3) into a single click from the source location.

## 20.6 Trace viewer

A dedicated webview (bundled with the VS Code extension, also runnable standalone as `ulx trace view`) renders a run's trace (§18) as a scrubbable timeline — directly modeled on Playwright's trace viewer (§2.6): each statement is a row with its resolved provider, cache-hit status, input/output artifact previews, and (for judge/validator statements) the `Verdict` with its reasoning, scrubbable forward and backward, with a "fork from here" action wired directly to `ulx fork` (§18.4, §19.3).

## 20.7 Static analysis

Beyond the hard compile errors in §9 (artifact-type mismatch, non-exhaustive `match`, `with`-block sibling references, capability-negotiation failures), the language server surfaces warnings for:

- **Unused outputs** — a binding never referenced downstream and not the subject of `expect`/`snapshot`/`assert` (§13.5's dead-artifact elimination pass, surfaced as a diagnostic rather than only a silent optimization).
- **Unreachable branches** — a `match` arm whose pattern can never be reached given prior arms (standard exhaustive-match tooling, applied here to `Verdict`/`Draft<T>` specifically).
- **Untrusted FFI validators** — a `python`/`javascript`/`shell`-backed validator (§15.12) is flagged distinctly from a fully-checked native validator, since the compiler cannot verify its internal purity (§9.1) — a visible reminder rather than a silent trust assumption.
- **Uncalibrated judges** — a `judge` used to gate a `retry`/`escalate` decision (§7.3) that has no corresponding calibration `benchmark` (§17.1) in the package is flagged as a lint warning, encouraging the methodology in §17.1 to be the default, not an opt-in best practice easily skipped.

## 20.8 Attaching to live/recent runs

The VS Code extension can attach to a locally running `ulx run --debug` session or load the most recent trace for the file open in the editor, so hovering a binding (§20.4) or setting a breakpoint (§19.2) works against real data during active development, not only in the abstract.

## 20.9 Documentation generator

`ulx doc` extracts `///` doc-comments (§7.1) attached to `conversation`/`judge`/`validator`/`dataset`/`type` declarations into a static site, including each declaration's checked type signature (so documentation cannot silently drift out of sync with the actual signature the way a hand-maintained README can) — modeled on `rustdoc`/`cargo doc`.

## 20.10 Formatter and syntax highlighting

`ulx fmt` is a deterministic, opinionated formatter (à la `gofmt`/`rustfmt` — no configuration knobs, one canonical style) operating on the AST (§13.3), guaranteeing formatted code round-trips through parsing unchanged. A TextMate grammar and a Tree-sitter grammar are both published from one source-of-truth grammar description (generated from §8's EBNF where the tooling allows, hand-maintained where it doesn't) so editor syntax highlighting, code folding, and structural selection work consistently across every supported editor without each maintaining a separately drifting grammar file.

## 20.11 REPL

`ulx repl` evaluates conversation fragments interactively against a live provider registry (§12.4), with every expression's result available for immediate `judge`/`assert` inspection — useful for iterating on a rubric or a single step before wiring it into a full `conversation` declaration, the interactive-exploration workflow every framework in §2 currently only offers via a Jupyter notebook with no static checking at all.

## 20.12 Package manager integration

`ulx add <package>`, `ulx build`, `ulx test`, `ulx publish` round out the CLI surface referenced throughout (§14), with IDE integration surfacing dependency versions, lockfile drift, and semver-contract-check failures (§14.4) as inline diagnostics on the `ulexite.toml` manifest itself.
