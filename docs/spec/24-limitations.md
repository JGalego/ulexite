# 24. Limitations

Honest accounting of what this design does not solve, trades away, or leaves genuinely uncertain — a spec that only lists strengths has not been stress-tested.

## 24.1 Zero production track record

Every "genuinely superior" row conceded in §22.1 is a real cost: LangGraph's checkpointer has years of production edge cases behind it (including the bugs documented in §2.3 — bugs are evidence of real usage, not just weakness); Ulexite's checkpoint/replay design (§10.4, §18) is unvalidated against production failure modes (partial writes during a crash mid-checkpoint, clock skew across a distributed provider registry, adversarial trace-log tampering) that only surface at scale. The spec asserts these are solvable within the architecture; it does not claim they are solved.

## 24.2 The `with`-block independence rule trades expressiveness for soundness

§9.7's "no sibling references inside a `with` block" is a parser-enforced, sound guarantee, but it is also strictly less expressive than Pulumi's or Beam's inferred-dependency graphs (§2.4) — a genuinely useful pattern like "three retrieval calls that could theoretically run in parallel, but the second wants to see the first's result to decide whether to bother" cannot be expressed inside one `with` block at all; it must be written sequentially, forfeiting the parallelism the author might have wanted. This is a deliberate trade (§9.7 states it explicitly), but it is a real expressiveness ceiling, not a solved problem.

## 24.3 The generics system is deliberately thin

§9.8's small, closed generic vocabulary (`Draft<T>`, `dataset<Row>`, `list<T>`) cannot express, e.g., a user-defined generic container over artifacts with its own merge semantics, or higher-kinded abstractions over "any capability that produces a T." A library author who needs this must drop to the stdlib's Rust implementation layer (§13.1, §15.17) rather than expressing it in Ulexite itself — an acceptable trade for a domain-specific language per §1.3, but a genuine limitation relative to a general-purpose language with a fuller type system.

## 24.4 Capability negotiation cannot fully close the provider-parity gap

§9.6's `structured_output: guaranteed | negotiated | unsupported` tiering is a real improvement over silent runtime failure (§3.4), but it can only reflect a capability difference the provider plugin author correctly declares — it cannot detect an *undeclared* gap (a provider that claims `guaranteed` but is subtly wrong under some input shape). This converts most of §2.3's "provider-agnostic, but leaky" problem into a compile-time-checkable one, but it depends on honest, well-tested plugin authors the same way any trait-based plugin system does (§12.4) — it is a mitigation, not an elimination, of the underlying problem.

## 24.5 Non-determinism typed at the call boundary, not eliminated

§9.3's `Draft<T>` makes non-determinism visible and exhaustively handled, but it does not make an LLM's output *correct* or *stable* — two runs with `cache: off` (§10.3) against the same prompt can still legitimately produce different `Settled(T)` values satisfying the same type. Ulexite's type system disciplines how a program *reacts* to non-determinism; it does not, and cannot, reduce the model's actual sampling variance. Judges (§5.6, §17.1) mitigate this at the evaluation layer, not the type layer, and judges themselves are probabilistic instruments requiring the calibration discipline in §17.1 to be trustworthy — a discipline the language encourages (§20.7's lint) but cannot force.

## 24.6 IR interpretation has a real (if usually dominated) performance ceiling

§13.6's choice to interpret rather than natively compile is justified by network-bound latency dominating interpretation overhead — but for a program with a very large, mostly-`Pure` computational core (heavy client-side artifact post-processing, large-scale embedding math done in-language rather than via a provider capability), interpretation overhead is no longer dominated by network latency and becomes a real, measurable cost. The mitigation is architectural, not eliminative: push genuinely heavy computation into a `python`/`shell` FFI call (§15.12) or a provider/tool plugin (§12.4/§12.6) written in Rust, rather than expressing it as Ulexite `Pure` IR nodes.

## 24.7 The declarative/imperative boundary is a design bet, not a proof

§4.5 and §10.2's split — declare the provably independent part, write the rest imperatively — is a bet that this two-region model covers the common case well enough that authors rarely fight it. It is possible real-world usage reveals a large class of programs that are "almost" parallelizable but don't fit `with`'s strict independence rule (§24.2) often enough that the ergonomic cost outweighs the soundness benefit; if so, a future revision (§25) may need a more permissive, effect-tracked parallelism model instead of the current syntactic one, at the cost of the simplicity §9.7 currently provides.

## 24.8 Tooling and ecosystem debt is real and unhidden

§22's package-ecosystem-maturity row honestly rates Ulexite "Low (new)" against LangChain/LlamaIndex's very large integration catalogues. Every provider, every vector store, every tool integration those ecosystems have accumulated over years must be either reimplemented as an Ulexite plugin or wrapped via FFI (§15.12) before parity is reached — §23.8 describes the wrapping path, but it is still work, not a solved problem, and some long tail of niche integrations may never justify a native port.

## 24.9 A new language imposes a real learning cost

Every team adopting Ulexite must learn a new grammar, a new type system's specific vocabulary (`Draft<T>`, `Verdict`, capability negotiation), and a new toolchain (§13, §20) — even with the migration paths in §23 designed to be incremental, this is a strictly higher up-front cost than adding one more Python import to an existing codebase, and that cost is only justified for teams whose conversation-orchestration surface is large/critical enough for the static guarantees (§9) to pay for themselves — §25 and §1.4 argue this bar is increasingly common, not that it is universal.
