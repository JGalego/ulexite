---
title: Package System
description: The ulexite.toml manifest format — package metadata, providers, runtime config — and the dependency-resolution design that isn't built yet.
---

# Package System

Every Ulexite package is a directory with an `ulexite.toml` manifest, deliberately modeled on Cargo's `Cargo.toml` — a proven, minimal-ceremony format rather than a novel one.

Manifest parsing and validation are real and working today: `ulx init` scaffolds one, `ulx manifest` parses and prints one, and `ulx run`/`ulx check` read a manifest's `[providers.*]` tables to resolve provider configuration. **Dependency resolution — a registry, a lockfile, semver-contract checks at publish time — is not built yet.** This page covers the manifest format in full, then is direct about that gap.

## The manifest

```toml
[package]
name = "acme-support-bot"
version = "0.3.0"
ulexite = "^1.0"          # language/compiler version requirement

[dependencies]
translation-judges = "^2.1"
rag-toolkit = { git = "https://example.com/rag-toolkit.git", tag = "v0.9.0" }

[providers.openai]
vendor = "openai"                # mandatory — never inferred from the table name
api_key_env = "OPENAI_API_KEY"   # names an env var; never a literal key in this file
chat = "gpt-4o-mini"             # every other key is a capability, bare model name or
vision = "gpt-4o-mini"           # a { model = "...", params = { ... } } table for overrides

[runtime]
concurrency = 8
cache_backend = "local"    # or "remote", configured below
```

- **`[package]`** — `name`, `version` (checked as a real `x.y.z` semver string), and `ulexite` (the language/compiler version requirement your package needs).
- **`[dependencies]`** — see below; parsing works, resolution doesn't.
- **`[providers.<name>]`** — one table per configured vendor account/deployment. See [Providers](./providers.md) for the full capability/vendor reference; a provider table with no capabilities declared is rejected as invalid at parse time.
- **`[runtime]`** — `concurrency` (defaults to your machine's available parallelism) and `cache_backend` (`"local"` by default; `"remote"` is accepted as a value but has no backend implementation behind it today).

Scaffold one with `ulx init`:

```bash
ulx init my-first-package /tmp/my-first-package
```

This writes a minimal `ulexite.toml` (empty `[dependencies]`, no `[providers.*]` yet) plus a starter `main.ulx` conversation into the target directory. Inspect any manifest — the one you just scaffolded, or one you're handed — with `ulx manifest`:

```bash
ulx manifest                          # ulexite.toml in the current directory
ulx manifest path/to/other/ulexite.toml
```

It prints the package name/version/required-`ulexite`-version, the dependency list, configured providers and their capabilities, and the runtime settings — the same validation `ulx run`/`ulx check` apply internally, surfaced as a readable report.

## Dependencies — parses, doesn't resolve

A dependency entry is either a bare version-requirement string, or a table naming a `git` or `path` source (optionally with a `tag`):

```toml
[dependencies]
translation-judges = "^2.1"
rag-toolkit = { git = "https://example.com/rag-toolkit.git", tag = "v0.9.0" }
local-experiment = { path = "../local-experiment" }
```

The manifest parser accepts and validates this shape — a `{ ... }` dependency table with neither `git` nor `path` is rejected — but nothing downstream of parsing does anything with a dependency entry today. There's no registry to resolve a bare version string like `"^2.1"` against, no code that fetches a `git`/`path` dependency, and no lockfile. If your package declares dependencies, `import`ing across files still works for files you provide directly (see [Syntax → Imports and reuse](./language/syntax.md#imports-and-reuse)) — what doesn't exist is a resolver that turns a `[dependencies]` table into actual code pulled from somewhere else.

The intended design, for context on where this is headed: dependencies would be `conversation`/`judge`/`validator`/`dataset`/`type` packages plus provider/tool plugins, and resolution would produce a lockfile (`ulexite.lock`) pinning exact versions and content hashes of every transitive dependency — the same content-addressing discipline the runtime already applies to artifacts and IR nodes — so a locked build would be bit-for-bit reproducible. A package's *dependencies* could churn, but the *program* that depends on them wouldn't silently break, because the lockfile would pin exactly what compiled last time until you deliberately ran `ulx update`. None of `ulexite.lock`, `ulx update`, or a resolver exists yet.

## Registry and distribution — not built

The design calls for a central registry (`packages.ulexite.dev`, mirroring `crates.io`/npm) hosting published packages, with git/path dependencies supported directly for private or in-development packages without requiring a publish step — the same escape hatch Cargo and npm both provide. Provider and tool plugins would be ordinary packages implementing the same traits a built-in provider adapter implements, so publishing a new provider adapter wouldn't require a compiler or registry change.

**None of this exists.** There's no registry, no `ulx publish`, and no package-fetching mechanism at all. A `git`/`path` dependency entry parses, as noted above, but nothing fetches it.

## Semantic versioning with teeth — not built

The design's more ambitious claim is that because judges, validators, and datasets are typed values with a declared signature, a package's semver commitment could be checked, not just documented: a minor-version bump wouldn't be allowed to change a published judge's `Verdict` variant set or a dataset's row schema without a major-version bump, with `ulx publish` verifying this automatically against the previous published version at publish time. Since there's no registry and no `ulx publish`, there's nothing to check this against — a package's declared `version` in `[package]` is validated only for well-formed semver shape today, not for any compatibility guarantee against a prior release.

## Workspaces — not built

The design allows a single repository to contain multiple packages sharing one lockfile (a `[workspace]` table in a root manifest, again mirroring Cargo), letting a larger project split shared judges/datasets/conversations into independently versioned packages within one build graph. There's no `[workspace]` support in the manifest schema today, and no multi-package build graph — every `ulexite.toml` describes exactly one package.

## What this means in practice

Today, a "package" is really just a directory with a valid `ulexite.toml` whose `[providers.*]` tables `ulx run`/`ulx check` read, plus whatever `.ulx` files you organize underneath it and wire together with ordinary `import` statements. That's a real, useful unit — it's just not yet the dependency-graph-with-a-registry story the full design describes. If you need to share judges/conversations/datasets across projects today, the working mechanism is `import ... from "path/to/file.ulx"` against a file you have on disk, not a versioned package pulled from anywhere.

For the full design rationale, see [§14 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/14-package-system.md).
