---
title: Type System and Artifacts
description: How Ulexite's static type system reasons about artifacts, non-determinism, and judge/validator verdicts — and how artifacts are stored and versioned.
---

# Type System and Artifacts

Ulexite's type system and its artifact model are really one story: every value flowing through a conversation is a typed *artifact*, and the type system's job is almost entirely about reasoning over what kind of artifact something is, what a model call is allowed to do with it, and what shape of non-determinism or verdict a step might hand back. This page covers both together.

## Static, checked everywhere except the FFI boundary

Ulexite is statically typed in its own grammar's terms: artifact types, record types, and closed unions are all checked at compile time. Gradual typing shows up in exactly one place — the explicit FFI boundary, where a `python`, `javascript`, or `shell` validator calls out to host-language code. A call like this:

```ulexite
validator CheckInvariant(x: json) -> Verdict {
  python: "checks.py:validate"
}
```

returns a value whose Ulexite-facing type you declare at the call site, and the compiler treats that declared type as a checked contract, not something it infers. Everywhere the compiler owns the code, there's no `any` and no null — the FFI boundary is the one deliberate place where soundness is traded for interop, and it's the only place.

## Artifact types: a closed lattice

An artifact is never a bare string, byte blob, or untyped dict — it's always a value of one specific type, and the compiler reasons about those types structurally. The type lattice is closed: fourteen built-in artifact types, plus user-defined record types built out of them.

| Type | Notes |
|---|---|
| `text` | UTF-8; the default artifact type when unannotated. |
| `markdown` | `text` with a declared dialect (CommonMark by default); kept structurally distinct from `text` so a step that needs rendered structure can require it specifically. |
| `image` | Carries width/height/mime metadata; subject to the capability `accepts`/`produces` checks described below. |
| `audio` | Carries duration/sample-rate/mime metadata. |
| `video` | Carries duration/resolution/mime metadata — the canonical "rejected at compile time" example (see below). |
| `pdf` | Carries a page count. A `pdf` is *not* implicitly a `text` — OCR/extraction is an explicit capability call, never an automatic, lossy coercion. |
| `json` | Carries an optional schema reference, validated structurally at the type level, not just at runtime. |
| `xml` / `html` | Analogous to `json`, with an optional schema/DTD-equivalent reference. |
| `csv` | Carries an optional column-type schema. |
| `embedding` | A fixed-dimension float vector; the dimension is part of the type itself (`embedding<1536>`), so a dimension mismatch between a step's output and a downstream vector-store capability is a compile error. |
| `vector` | A general fixed-dimension numeric vector not tied to any embedding model — used for arbitrary numeric artifacts such as classifier logits. |
| `tool_output` | The result of a tool/function message; structurally a tagged union of the other artifact types plus a `raw` fallback for genuinely unstructured tool results — an explicit, visible escape hatch rather than a silent "any." |

Every standard-library capability declares an `accepts: [ArtifactType...]` and `produces: [ArtifactType...]` signature, and `ask <capability>(artifact)` type-checks the artifact you pass in against `accepts`, and the return binding's declared type against `produces` — at compile time. That's what makes this fail before any request ever reaches a provider:

```ulexite
conversation Caption(clip: video) -> text {
  ask vision(clip) { user: "Describe this clip." } -> caption: text   // OK: vision.accepts includes video
  ask chat(clip)    { user: "Summarize this clip." } -> bad: text     // compile error: chat.accepts = [text, image]
}
```

The second call fails to compile with a diagnostic naming the capability's declared `accepts` set and the artifact's actual type — a routing mistake caught before you pay for an API call, not a runtime 400 (or worse, a silently truncated input) from whichever vendor you happened to be pointed at.

Structured artifacts (`json`, record types) carry an optional schema reference — a `type Name = { field: Type, ... }` declaration — and a step returning structured output is checked against that schema before a resolver runs, the way a GraphQL server validates a selection set's shape. This is only as strong as the underlying provider allows, though: see [capability negotiation](#capability-negotiation) below.

## Artifacts are content-addressed values, not bare bytes

Beyond its type, every artifact carries:

- **A content hash** — a stable hash of its serialized bytes, computed once at creation, in the same spirit as git's blob-addressing model.
- **Type metadata** — mime/dimensions for `image`/`video`/`audio`, a schema reference for `json`/structured records, encoding for `text`/`markdown`.
- **Provenance** — the step, capability, and input artifacts it was derived from, if any.

Identical content is deduplicated for free by construction: two steps that happen to produce byte-identical `image` output share one stored artifact. That's also what makes caching sound rather than approximate — a cache key includes input artifacts' content hashes, not their in-memory identity.

### Storage model

The design calls for artifacts to be stored the way git stores objects: content-addressed, immutable, in a local or remote object store keyed by hash. A conversation's checkpoint log holds pointers (hashes) rather than inline copies of large artifacts, so a checkpoint stays small and fast to write even when the artifacts it references are large, and two checkpoints referencing the same unchanged artifact share storage automatically. Branching a conversation — running an alternate prompt from turn three onward, say — is meant to be as cheap as a git branch: a new pointer into an otherwise-shared object graph, not a copy.

That's the design target, and it's worth being direct about where the current implementation stands relative to it: there is no pluggable, content-addressed artifact/blob store yet. A file argument passed on the command line is read off disk by the provider adapter itself at the point it makes the HTTP call, not by a general artifact layer; output from capabilities like `speak` or `generate_image` is written to a temporary directory under a content-addressed filename, which gets you the hash-naming convention but not the shared object store, branching-is-cheap, or cross-run deduplication the full design describes. Treat the git-like storage model above as where the language is headed, not as a guarantee your program can currently depend on.

### Dependency graph and memoization

Every artifact records what it was derived from, forming a dependency graph alongside the conversation's control-flow trace — the same idea as a UI framework's dependency-array memoization, applied to conversation steps instead of components. A step is treated as pure-until-its-declared-inputs-change, so re-running a conversation after editing one upstream artifact (a source document, a rubric) is intended to re-execute only the steps whose recorded dependencies actually changed, reusing the cache for everything else, rather than forcing a full re-run on every edit.

### Versioned and derived artifacts

A `dataset` is itself a versioned artifact collection: each row is content-addressed the same as any other artifact, so a `benchmark` run against one version of a dataset can in principle be replayed byte-for-byte against that exact version even after the file on disk has since changed — a guarantee that falls out of content-addressing structurally, rather than depending on a human remembering to bump a version string in a filename.

## Non-determinism is a type, not a convention

A model call's raw return type is never a plain `T`. It's:

```ulexite
Draft<T> = Settled(T) | Refused(reason: text) | RateLimited(retry_after: duration) | Timeout
```

Every `ask`/message-literal binding is a `Draft<T>`, and a program can't use it as a plain `T` without either (a) a `match` that handles all four variants, or (b) piping it through a `judge`/`validator`, whose own `Verdict` result the program must in turn handle. Retryable, rate-limited, refused, and successful outcomes are four different shapes visible in the type signature — not four cases an exception handler might or might not happen to catch. The common case (a terse `assistant -> draft: text` immediately followed by a `judge`/`retry`, as in the [syntax page](./syntax.md)) is sugar that expands to exactly this `match`, so ordinary programs read as if the type were bare `T` without the guarantee actually being optional.

It's worth being precise about what this guarantee is and isn't: `Draft<T>` makes non-determinism *visible* and forces you to handle it. It does not make a model's output correct or stable — two runs against the same prompt with caching turned off can still legitimately produce two different `Settled(T)` values that both satisfy the type. The type system disciplines how your program *reacts* to non-determinism; it does not, and cannot, reduce the model's actual sampling variance. Judges mitigate that at the evaluation layer, not the type layer.

## `Verdict`: a closed union, exhaustively matched

```ulexite
Verdict = Pass | Fail(reason: text) | Score(value: float) | Escalate
```

Both `judge` and `validator` declarations return `Verdict`. A `match` over any value of a closed union type — `Verdict`, `Draft<T>`, or a union type you declare yourself — must cover every variant, or the compiler rejects the program. This is the same discipline as Rust's exhaustive `match` over an enum, applied specifically to judge/validator outcomes so that handling every case is enforced by the compiler rather than left to a reviewer's memory.

One current-implementation caveat worth knowing if you're relying on this guarantee heavily: exhaustiveness checking today specifically recognizes the literal type name `Verdict` — it isn't yet a fully general check over any closed union or over `Draft<T>`. A user-declared union type gets no exhaustiveness checking at all right now, and a type you happen to name `Verdict` yourself could confuse the check. Treat `match` exhaustiveness as reliable for the built-in `Verdict` type today, and as a documented design intent — not yet a general guarantee — for arbitrary unions.

## Merge semantics for concurrent writes

When two branches of a `with` block, or two concurrently scheduled steps, produce artifacts bound to the same name in an enclosing scope, the compiler requires an explicit merge function, declared once per type:

```ulexite
type Notes = list<text> merge with concat
type Score = float merge with max
```

A program with two concurrent writers to the same binding and no declared merge function is a compile error, not a silent last-write-wins surprise at runtime.

## Capability negotiation

A provider plugin declares, at registration, which capabilities it satisfies and at what guarantee tier:

```ulexite
capability chat {
  accepts: [text, image]
  produces: [text]
  structured_output: negotiated   // "guaranteed" | "negotiated" | "unsupported"
}
```

`structured_output: guaranteed` means the provider offers token-level grammar constraint, and the compiler can rely on it never producing a schema-invalid `json` artifact. `negotiated` means the runtime validates after the fact, and a schema mismatch surfaces as `Draft<T>.Refused` rather than a crash. `unsupported` is a compile-time error if your program's step requires a guarantee the resolved provider can't offer.

This tiering is a real improvement over silent runtime failure, but it's only as good as the plugin authors declaring it: it can catch a *declared* capability gap, not an undeclared one where a provider claims `guaranteed` but is subtly wrong under some input shape. It converts most of the provider-parity problem into something compile-time-checkable, but it's a mitigation, not an elimination — it still depends on honest, well-tested provider plugins the same way any trait-based plugin system does.

## Purity and independence checking in `with` blocks

Every binding inside a `with` block (see the [syntax page](./syntax.md#declarative-with-blocks)) is checked to reference only bindings from *outside* the block, never a sibling. That's a static, syntactic guarantee, not a data-flow analysis that could in principle be fooled — the block's members are independent by construction, which is what makes it safe for the compiler to parallelize, cache, and reorder them.

This is a deliberate trade of expressiveness for soundness. A genuinely useful pattern — three retrieval calls that could theoretically run in parallel, except the second wants to see the first's result before deciding whether to bother — cannot be expressed inside one `with` block at all; it has to be written sequentially, forfeiting the parallelism you might have wanted. If your steps have that kind of soft dependency, write them as sequential statements outside the block rather than fighting the checker.

## Generics

Ulexite's generic vocabulary is small and deliberately closed: `Draft<T>`, `dataset<Row>`, `list<T>` cover the recurring shapes above without importing a general-purpose generics system's complexity — no higher-kinded types, no trait bounds beyond "has a declared merge function" for the concurrent-write rule above. If you need a user-defined generic container over artifacts with its own merge semantics, or an abstraction over "any capability that produces a `T`," that's genuinely out of reach in Ulexite itself today — you'd need to drop to the standard library's implementation layer instead. That's an accepted trade for a domain-specific language, not an oversight, but it is a real ceiling relative to a general-purpose type system.

For the full design rationale, see [§9](https://github.com/JGalego/ulexite/tree/main/docs/spec/09-type-system.md) and [§11 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/11-artifact-system.md).
