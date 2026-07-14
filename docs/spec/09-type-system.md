# 9. Type System

## 9.1 Static, gradual at the FFI boundary, sound everywhere else

Ulexite is statically typed in its own grammar's terms: artifact types, record types, and closed unions (§8) are checked at compile time. Gradual typing is permitted only at the explicit FFI boundary (§15.12–15.13, `python`/`javascript`/`shell` validators) — a call out to a host-language function returns a value whose Ulexite-facing type must be declared at the call site (e.g. `validator CheckInvariant(x: json) -> Verdict { python: "checks.py:validate" }`), and the compiler treats the declared type as a checked contract, not an inferred one. This mirrors Gleam's "no `any`, no null, sound everywhere the compiler owns the code" stance (§2.5) while acknowledging, unlike Gleam, that a conversation-first language must call out to Python/JS tooling routinely (§1.3) — the boundary is where soundness is deliberately traded for interop, and it is the *only* place.

## 9.2 Artifact types

The fourteen artifact types listed in §8's `artifact_type` production (`text`, `markdown`, `image`, `audio`, `video`, `pdf`, `json`, `xml`, `html`, `csv`, `embedding`, `vector`, `tool_output`, plus user-defined `record_type`s built from them) form a closed lattice the compiler reasons about structurally:

- Every stdlib capability (§15.1) declares an `accepts: [ArtifactType...]` and `produces: [ArtifactType...]` signature. `ask <capability>(artifact)` type-checks the artifact's static type against `accepts` and the return binding's declared type against `produces`, at compile time — this is the mechanism behind §4.2 and §7.5's "a `video` artifact routed into a text-only model is a compile error," resolving the gap documented against every framework in §2.3 and §3.2, none of which perform this check before a request is sent.
- Structured artifacts (`json`, `record_type`s) carry an optional schema reference (a `type Name = { field: Type, ... }` declaration), and a step returning structured output is checked against that schema the way GraphQL validates a selection set's shape before a resolver runs (§2.5) — not "call the model and hope the JSON matches," which is the failure mode in every surveyed framework's structured-output story except Guidance/LMQL's token-constrained decoding (§2.1), whose guarantee Ulexite adopts *as an optional, negotiated runtime capability* (§9.6) rather than a universal assumption.

## 9.3 Non-determinism is a type, not a convention

A model call's raw return type is not `T`; it is:

```
Draft<T> = Settled(T) | Refused(reason: text) | RateLimited(retry_after: duration) | Timeout
```

Every `ask`/message-literal binding is a `Draft<T>`, and a program cannot use it as a plain `T` without either (a) a `match` that handles all four variants, or (b) piping it through a `judge`/`validator`, whose own `Verdict` result the program must likewise handle. This is Zig's explicit error-union philosophy (§2.5) applied to LLM non-determinism specifically: retryable, rate-limited, refused, and successful outcomes are four different shapes visible in the type signature, not four cases an exception handler might or might not catch — closing §2.8's "determinism-by-convention, not by construction" gap directly. The common case (§7.3's terse `assistant -> draft: text` followed immediately by a `judge`/`retry`) is sugar that expands to exactly this `match`, so ordinary programs read as if the type were bare `T` without the guarantee being optional.

## 9.4 Verdict: a closed union, exhaustively matched

```
Verdict = Pass | Fail(reason: text) | Score(value: float) | Escalate
```

Both `judge` and `validator` declarations return `Verdict` (§7.2–7.3); a `match` over any value of a closed union type — `Verdict`, `Draft<T>`, or a user-declared union (§8's `union_type` production) — must cover every variant or the compiler rejects the program (§8.1). This is Rust's exhaustive-`match`-over-ADTs discipline and Gleam's total-function-over-mailbox-type discipline (§2.5), applied to judge/validator outcomes specifically to close the single most consistently reinvented-and-unenforced gap in §2.7 (point 4): every framework surveyed has *some* verdict-like return value, and none of them forces the caller to handle every case at compile time.

## 9.5 Merge semantics for concurrent writes

When two branches of a `with` block (§7.4) or two concurrently-scheduled steps produce artifacts bound to the same name in an enclosing scope, the compiler requires an explicit merge function, declared once per type:

```
type Notes = list<text> merge with concat
type Score = float merge with max
```

This is LangGraph's `Annotated[type, reducer]` idea (§2.3, §2.7) promoted from an opt-in field annotation a user must remember to add, to a mandatory declaration checked at compile time: a program with two concurrent writers to the same binding and no declared merge function is a compile error, not a last-write-wins runtime surprise.

## 9.6 Capability negotiation as a type-level contract

A provider plugin (§12.4) declares, at registration, which capabilities it satisfies and at what guarantee tier:

```
capability chat {
  accepts: [text, image]
  produces: [text]
  structured_output: negotiated   // "guaranteed" | "negotiated" | "unsupported"
}
```

`structured_output: guaranteed` means the provider offers token-level grammar constraint (Guidance/LMQL-style, §2.1) and the compiler may rely on it never producing a schema-invalid `json` artifact; `negotiated` means the runtime validates post hoc and a schema mismatch surfaces as `Draft<T>.Refused`, not a crash; `unsupported` is a compile-time error if the program's step requires a guarantee the resolved provider cannot offer. This is the direct fix for the OpenAI Agents SDK's LiteLLM caveat and Semantic Kernel's per-connector feature matrix (§2.3) both being *runtime* surprises today — Ulexite moves the same information one phase earlier, into the type checker.

## 9.7 Purity and independence checking

Every binding inside a `with` block (§7.4, §8) is checked to reference only bindings from *outside* the block, never a sibling — a static, syntactic guarantee (not a data-flow analysis that could in principle be fooled) that the block's members are independent and therefore safe to parallelize, cache, and reorder (§10.2, §4.5). This is deliberately stricter and simpler than Pulumi's approach of inferring a resource graph from arbitrary imperative code with no purity guarantee (§2.4) — Ulexite trades some expressiveness for a soundness guarantee the compiler, not the programmer's discipline, upholds.

## 9.8 Generics

A small, deliberately limited generic vocabulary — `Draft<T>`, `dataset<Row>`, `list<T>` — covers the recurring shapes above without importing a general-purpose generics system's complexity (no higher-kinded types, no trait bounds beyond "has a declared merge function" for §9.5). This mirrors Go's original minimalism-over-generality stance more than Rust's trait system; §24 (Limitations) discusses what this rules out and why that trade is acceptable for this domain.
