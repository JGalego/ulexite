---
title: Roadmap
description: What's next for Ulexite, on the RFC track after the core spec stabilizes.
---

# Roadmap

These are concrete, RFC-track proposals for after the core spec stabilizes and the reference implementation matures further. They're ordered roughly by dependency — later items generally assume earlier ones have landed. None of this is committed or scheduled; it's the direction the design is headed.

- **Effect-tracked parallelism beyond `with`.** An effect-tracking type-system extension (in the spirit of algebraic effects) that lets the compiler prove independence for a broader class of programs than today's syntactic sibling-reference restriction allows — without giving up the soundness guarantee. `with` would become sugar for a more general, effect-typed scheduling primitive.

- **Distributed execution engine.** A distributed variant of the current single-process engine: sharding the scheduler across a worker fleet, replicating the trace engine for high-availability checkpoint writes, and defining exactly-once semantics for effect nodes across worker failover — building on the lessons of Temporal's distributed durable-execution model.

- **Formal verification of scheduling and replay.** A machine-checked proof (e.g. in Lean or Coq) that the two-pass scheduler never reorders across a true dependency, and that replay is exactly equivalent to live execution given an identical trace — moving today's informal argument to a verified one, in the spirit of seL4/CompCert.

- **Cost-aware automatic provider routing.** Extending today's static routing policies (`cheapest`, `pinned(...)`) into a learned/adaptive router that uses accumulated trace data to pick a provider per call based on observed quality-cost tradeoffs for that specific capability and input distribution.

- **Multi-language host bindings beyond FFI.** Today, Python/JS/shell are an escape hatch called *from* Ulexite. A stable C ABI would let a host application in any language embed the Ulexite runtime and drive conversations from outside `.ulx` source entirely — useful for gradually introducing Ulexite into an existing large application.

- **A standardized capability conformance suite.** A published, versioned test suite that any provider plugin author can run to certify which `structured_output` tier and which artifact types their plugin actually delivers under load — turning today's "depends on honest plugin authors" limitation into a certifiable, third-party-auditable property, the way TCK suites work for JVM languages.

- **A visual/notebook authoring surface.** A structured-editing surface for authoring `with`-block dataflow and `match` branching visually, extending the trace viewer's timeline UI from a read-only debugging tool into an optional bidirectional authoring tool — aimed at prompt engineers and analysts who aren't primarily software engineers.

- **Judge ensembles and adversarial verification as language sugar.** Promoting patterns like multiple independent judges voting or refutation-biased verifiers from a standard-library convention into dedicated grammar, once real-world usage shows which ensemble shapes recur often enough to deserve first-class syntax.

- **Formal semantics for cross-conversation supervision.** Extending today's single-conversation `supervise` scope across nested conversations — a parent conversation declaring a supervision policy that governs how child-conversation failures propagate, generalizing Elixir's supervision-tree model to Ulexite's conversation-nesting structure.

For the full design rationale, see [§25 of the spec](https://github.com/JGalego/ulexite/tree/main/docs/spec/25-future-directions.md).
