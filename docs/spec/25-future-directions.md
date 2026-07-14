# 25. Future Directions

Concrete RFC-track proposals for after the core spec (this document) stabilizes and a reference implementation exists, ordered roughly by dependency (later items assume earlier ones landed).

## RFC-2: Effect-tracked parallelism beyond `with`

Address §24.2/§24.7's expressiveness ceiling: an effect-tracking type-system extension (in the spirit of algebraic effects) that lets the compiler prove independence for a broader class of programs than §9.7's syntactic sibling-reference restriction currently allows, without giving up the soundness guarantee — the `with` block would become sugar for the common case of a more general, effect-typed scheduling primitive.

## RFC-3: Distributed execution engine

§12 describes a single execution engine; a follow-on RFC would specify a distributed variant — sharding the scheduler (§12.3) across a worker fleet, replicating the trace engine (§12.5) for high-availability checkpoint writes, and defining exactly-once semantics for effect nodes (§13.4) across worker failover, directly building on Temporal's own distributed-durable-execution lessons (§2.4) rather than the current single-process assumption.

## RFC-4: Formal verification of `with`-block scheduling and replay determinism

A machine-checked proof (e.g., in Lean or Coq, over a formalized subset of the IR, §13.4) that the two-pass scheduler (§10.2) never reorders across a true dependency and that replay (§10.4, §18.3) is exactly equivalent to live execution given an identical trace — moving §10's current informal argument to a verified one, in the spirit of the seL4/CompCert tradition of verifying exactly the properties a system's marketing claims.

## RFC-5: Cost-aware automatic provider routing

Extend §12.4's policy resolution from static policy expressions (`cheapest`, `pinned(...)`) to a learned/adaptive router that uses accumulated trace data (§18, §17.6's sweep tooling) to pick a provider per call based on observed quality-cost tradeoffs for that specific capability and input distribution — an evolution of §17.6's manual sweep into an automatic, continuously-updated policy.

## RFC-6: Multi-language host bindings beyond FFI

§9.1/§15.12 treat Python/JS/shell as an escape hatch called *from* Ulexite; a follow-on RFC would specify the inverse — a stable C ABI (naturally, given §13.1's Rust implementation) letting a host application in any language embed the Ulexite runtime and drive conversations from outside `.ulx` source files entirely, useful for gradually introducing Ulexite conversations into an existing large application without adopting the CLI/package workflow (§14, §20) wholesale.

## RFC-7: Standardized capability conformance suite

A published, versioned conformance test suite (§25 testing plan, extending §13's provider-conformance tests) that any provider plugin author can run to certify which `structured_output` tier (§9.6) and which artifact types (§9.2) their plugin actually delivers under load — converting §24.4's "depends on honest plugin authors" limitation into a certifiable, third-party-auditable property, the way TCK (Technology Compatibility Kit) suites work for JVM-language implementations.

## RFC-8: Visual/notebook authoring surface

A structured-editing surface (not competing with §20's text-first LSP tooling, but a genuinely different modality) for authoring `with`-block dataflow and `match` branching visually — informed by the trace viewer's timeline UI (§20.6) already existing, extending it from a read-only debugging tool into an optional bidirectional authoring tool for the declarative subset of the language, aimed at the population of prompt engineers and analysts who are not primarily software engineers.

## RFC-9: Judge ensembles and adversarial verification as language sugar

Promote the adversarial-verification and judge-ensemble patterns (multiple independent judges voting, refutation-biased verifiers) from a stdlib convention (composing `judge.pairwise`/`judge.meta` calls by hand, §15.2, §17.1) to dedicated grammar (`judge_panel`, `adversarial_judge`) once real-world usage shows which ensemble shapes recur often enough to deserve first-class syntax rather than a library function — deliberately deferred past the core spec so it is designed from observed usage, not speculation.

## RFC-10: Formal semantics for cross-conversation supervision

§10.7's `supervise` currently scopes to a named group of steps within one conversation; a follow-on RFC would extend supervision trees (§2.5) across nested conversations (§5.1, §21.5) — a parent conversation declaring a supervision policy that governs how child-conversation failures propagate, generalizing Elixir's supervision-tree model from single-process trees to the conversation-nesting structure Ulexite already has.
