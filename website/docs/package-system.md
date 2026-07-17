---
title: Package System
description: The ulexite.toml manifest format, real git/path dependency resolution, and the registry/lockfile design that isn't built yet.
---

# Package System

Every Ulexite package is a directory with an `ulexite.toml` manifest, deliberately modeled on Cargo's `Cargo.toml` — a proven, minimal-ceremony format rather than a novel one.

Manifest parsing and validation are real and working today: `ulx init` scaffolds one, `ulx manifest` parses and prints one, and `ulx run`/`ulx check` read a manifest's `[providers.*]` tables to resolve provider configuration. **A `git`/`path` dependency is real too** — it's actually resolved onto disk (cloned via the system `git` binary for `git`, joined against the manifest's directory for `path`) and importable across files. **A central registry, a lockfile, and semver-contract checks at publish time are not built yet.** This page covers the manifest format in full, then is direct about that remaining gap.

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
- **`[dependencies]`** — see below; `git`/`path` entries really resolve, a bare version string doesn't (there's no registry to resolve it against).
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

## Dependencies — `git`/`path` really resolve

A dependency entry is either a bare version-requirement string, or a table naming a `git` or `path` source (optionally with a `tag`):

```toml
[dependencies]
translation-judges = "^2.1"
rag-toolkit = { git = "https://example.com/rag-toolkit.git", tag = "v0.9.0" }
local-experiment = { path = "../local-experiment" }
```

`local-experiment` resolves by joining `path` against the manifest's own directory — an import from another file referencing this dependency's name resolves straight into that directory, no different from an ordinary relative import.

`rag-toolkit` really gets cloned: `ulx` shells out to the system `git` binary, clones the URL, and checks out `tag` (if given), landing the checkout in a local, vendored directory under `<package-dir>/.ulexite/git-deps/<hash-of-url-and-tag>/` — the same place `.ulexite/cache`/`.ulexite/traces` already live, and gitignored the same way. A later `ulx` invocation against the same `(git, tag)` pair reuses that existing checkout rather than re-cloning, so this doesn't re-fetch on every command. If `git` isn't installed, the clone fails, or the tag doesn't exist, you get a clear error naming the dependency — not a silent fallback or a confusing "file not found" from whatever import happens to reference it.

`translation-judges = "^2.1"` (a bare version string, no `git`/`path`) is the one shape that still doesn't resolve to anything — there's no registry to resolve a named version against. Referencing it from an `import` fails with a clear "dependency unresolvable" error rather than mishandling it.

What's still missing, for context on where this is headed: a lockfile (`ulexite.lock`) pinning exact versions and content hashes of every transitive dependency — the same content-addressing discipline the runtime already applies to artifacts and IR nodes — so a locked build would be bit-for-bit reproducible even as upstream `git` refs move, plus an `ulx update` command to deliberately refresh it. Today, a `git` dependency without a `tag` resolves to whatever the default branch's HEAD was at first clone, cached from then on — stable across repeated local builds, but not something a lockfile pins or that a second machine is guaranteed to reproduce identically without also copying the same `.ulexite/git-deps/` checkout.

## Registry and distribution — not built

The design calls for a central registry (`packages.ulexite.dev`, mirroring `crates.io`/npm) hosting published packages, with git/path dependencies supported directly for private or in-development packages without requiring a publish step — the same escape hatch Cargo and npm both provide (and the part that's real today, per above). Provider and tool plugins would be ordinary packages implementing the same traits a built-in provider adapter implements, so publishing a new provider adapter wouldn't require a compiler or registry change.

**The registry itself doesn't exist.** There's no `packages.ulexite.dev`, no `ulx publish`, and no way to resolve a bare version string to a real package — that needs real server infrastructure this repository doesn't have. The git/path escape hatch above is a real, working substitute for private or in-development packages, just not a substitute for a public, versioned registry.

## Semantic versioning with teeth — not built

The design's more ambitious claim is that because judges, validators, and datasets are typed values with a declared signature, a package's semver commitment could be checked, not just documented: a minor-version bump wouldn't be allowed to change a published judge's `Verdict` variant set or a dataset's row schema without a major-version bump, with `ulx publish` verifying this automatically against the previous published version at publish time. Since there's no registry and no `ulx publish`, there's nothing to check this against — a package's declared `version` in `[package]` is validated only for well-formed semver shape today, not for any compatibility guarantee against a prior release.

## Workspaces — not built

The design allows a single repository to contain multiple packages sharing one lockfile (a `[workspace]` table in a root manifest, again mirroring Cargo), letting a larger project split shared judges/datasets/conversations into independently versioned packages within one build graph. There's no `[workspace]` support in the manifest schema today, and no multi-package build graph — every `ulexite.toml` describes exactly one package.

## What this means in practice

Today, a "package" is a directory with a valid `ulexite.toml` whose `[providers.*]` tables `ulx run`/`ulx check` read, plus whatever `.ulx` files you organize underneath it and wire together with ordinary `import` statements — either directly by relative path, or through a `git`/`path` dependency entry that really resolves onto disk. That's a real, useful unit for sharing judges/conversations/datasets across projects or private repos — it's just not yet the full registry-backed, lockfile-pinned dependency-graph story the full design describes.

For the full design rationale, see [§14 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/14-package-system.md).
