# 11. Artifact System

## 11.1 Artifacts as typed, content-addressed values

An artifact is never a bare string, byte blob, or untyped dict — it is a value of one of the fourteen types in §9.2, carrying:

- **Content hash** — a stable hash of its serialized bytes, computed once at creation (git's blob-addressing model, §2.6, §2.8).
- **Type metadata** — mime/dimensions for `image`/`video`/`audio`, schema reference for `json`/structured records, encoding for `text`/`markdown`.
- **Provenance** — the step, capability, and input artifacts it was derived from, if any (§11.3).

Identical content is deduplicated for free by construction (two steps producing byte-identical `image` output share one stored artifact), which is also what makes §10.3's caching sound: the cache key includes input artifacts' content hashes, not their in-memory identity.

## 11.2 Storage model

Artifacts are stored the way git stores objects: content-addressed, immutable, in a local or remote object store (§12.7) keyed by hash, with a conversation's checkpoint log (§10.4, §18) holding pointers (hashes) rather than inline copies of large artifacts (video, PDF, embeddings) — a checkpoint is small and fast to write even when the artifacts it references are large, and two checkpoints referencing the same unchanged artifact share storage automatically. Branching a conversation (running an alternate prompt from turn 3 onward, say) is cheap for the same reason a git branch is cheap: it is a new pointer into an otherwise-shared object graph, not a copy.

## 11.3 The artifact dependency graph and memoization

Every artifact records what it was derived from, forming a DAG independent of (but recorded alongside) the control-flow trace (§10.2, §18). This is React's dependency-array/memoization idea (§2.5, §2.7) applied to conversation steps instead of UI components: a step is treated as pure-until-its-declared-inputs-change, and re-running a conversation after editing one upstream artifact (a source document, a rubric) re-executes only the steps whose recorded dependency set actually changed, using §10.3's cache for everything else — turning an edit-and-rerun cycle into an incremental recompute instead of a full re-run, which is the norm in every framework surveyed (re-running a LangChain chain or a DSPy program after a prompt tweak re-executes the whole thing unless the developer manually re-implements memoization).

## 11.4 Multimodal artifact types in detail

| Type | Notes |
|---|---|
| `text` | UTF-8, default artifact type when unannotated (§7.3). |
| `markdown` | `text` with a declared dialect (CommonMark by default); structurally distinguishable from `text` so a step requiring rendered structure can require it specifically. |
| `image` | width/height/mime metadata; capability `accepts`/`produces` checks (§9.2) apply. |
| `audio` | duration/sample-rate/mime metadata. |
| `video` | duration/resolution/mime metadata; §4.2's canonical "reject at compile time" example. |
| `pdf` | page count; a `pdf` is not implicitly a `text` (OCR/extraction is an explicit capability call, §15.6, §21.3, not an automatic coercion — avoiding the silent, lossy auto-conversions that make multimodal handling in every surveyed framework ad hoc, §3.2). |
| `json` | optional schema reference (§9.2); validated structurally at the type level, not just at runtime. |
| `xml` / `html` | analogous to `json`, with an optional schema/DTD-equivalent reference. |
| `csv` | optional column-type schema. |
| `embedding` | fixed-dimension float vector, dimension is part of the type (`embedding<1536>`) so a dimension mismatch between a step's output and a downstream vector-store capability is a compile error. |
| `vector` | a general fixed-dimension numeric vector not tied to an embedding model, used for arbitrary numeric artifacts (e.g. classifier logits). |
| `tool_output` | the result of a `tool`/`function` message (§5.2); structurally a tagged union of the other artifact types plus a `raw` fallback for genuinely unstructured tool results, closing the same gap §9.1 leaves open for FFI: the compiler checks what it can, and `raw` is the explicit, visible escape hatch rather than a silent `Any`. |

## 11.5 Artifact routing checks (worked example of §4.2/§9.2)

```
conversation Caption(clip: video) -> text {
  ask vision(clip) { user: "Describe this clip." } -> caption: text   // OK: vision.accepts includes video
  ask chat(clip)    { user: "Summarize this clip." } -> bad: text     // compile error: chat.accepts = [text, image]
}
```

The second `ask` fails to compile with a diagnostic naming the capability's declared `accepts` set and the artifact's actual type — a message-composition mistake that, per §2.3 and §3.2, surfaces in every surveyed framework only as a runtime 400 from the provider (or, worse, a silently truncated/ignored input).

## 11.6 Versioned and derived artifacts

A `dataset` (§7.6, §16.2) is itself a versioned artifact collection — each row is content-addressed the same as any other artifact, so a `benchmark` run against `dataset` version `v3` can be replayed byte-for-byte against that exact version even after the dataset file on disk has since changed, the same guarantee OpenAI Evals' `name.split.version` registry convention (§2.2) provides by naming discipline alone; Ulexite provides it structurally, by content-addressing, so it cannot be violated by forgetting to bump a version string.
