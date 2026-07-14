# 14. Package System

## 14.1 Manifest

Every package is a directory with an `ulexite.toml` manifest, deliberately modeled on `cargo`'s `Cargo.toml` — a proven, minimal-ceremony format rather than a novel one:

```toml
[package]
name = "acme-support-bot"
version = "0.3.0"
ulexite = "^1.0"          # language/compiler version requirement

[dependencies]
translation-judges = "^2.1"
rag-toolkit = { git = "https://example.com/rag-toolkit.git", tag = "v0.9.0" }

[providers]
default_chat = { capability = "chat", policy = "cheapest" }

[runtime]
concurrency = 8
cache_backend = "local"    # or "remote", configured below
```

## 14.2 Dependency resolution

Dependencies are `conversation`/`judge`/`validator`/`dataset`/`type` packages (§7.7 imports resolve against this registry) plus provider/tool plugins (§12.4, §12.6). Resolution produces a lockfile (`ulexite.lock`) pinning exact versions and content hashes of every transitive dependency — the same content-addressing discipline as artifacts (§11.1) and IR nodes (§13.7), so a locked build is bit-for-bit reproducible, closing the version-churn gap documented against every framework in §2.8: an Ulexite package's *dependencies* can churn, but the *program* that depends on them does not silently break, because the lockfile pins exactly what compiled last time until a developer deliberately runs `ulx update`.

## 14.3 Registry and distribution

A central registry (`packages.ulexite.dev`, mirroring `crates.io`/`npm`) hosts published packages; git/path dependencies (as in §14.1) are supported directly for private or in-development packages without requiring a publish step, the same escape hatch `cargo` and `npm` both provide. Provider and tool plugins are ordinary packages implementing the traits from §12.4/§12.6 — publishing a new provider adapter requires no change to the compiler or the central registry's own code, directly satisfying the mission's "providers should be plugins" requirement.

## 14.4 Semantic versioning applied to language-level contracts

Because judges, validators, and datasets are typed values with a declared signature (§7.2, §9), a package's semver commitment is checked, not just documented: a minor-version bump may not change a published `judge`'s `Verdict` variant set or a `dataset`'s row schema without a major-version bump — the compiler can verify this automatically against the previous published version at publish time (`ulx publish` runs this check before upload), giving semver teeth in exactly the place §2.8's version-churn catalogue shows every surveyed framework lacked it (a "patch" release breaking a subclassed interface, as documented against LangGraph's `langgraph-prebuilt==1.0.2`, §2.3, is the failure mode this check exists to catch mechanically).

## 14.5 Workspace support

A single repository may contain multiple packages sharing one lockfile (`[workspace]` in a root manifest, again mirroring `cargo`), letting a large organization split shared judges/datasets/conversations into independently versioned packages within one build graph — relevant because §3.1's core complaint about existing frameworks is that internal refactors propagate as breaking changes to *everyone*; workspace-local packages let an organization iterate on shared judges/conversations with the same lockfile discipline as external dependencies, catching a breaking internal change before it reaches a dependent package rather than after.
