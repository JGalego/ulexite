# 23. Migration Paths

## 23.1 Principle: migrate incrementally, at a conversation's boundary

No existing codebase is expected to rewrite wholesale. Because a `conversation` value satisfies a single, simple execution protocol (§10.1) and Ulexite ships with an FFI boundary designed in from the start (§9.1, §15.12), the recommended path is always **wrap one conversation at a time**, called from the existing host application, not a big-bang port.

## 23.2 From LangChain / LangGraph

- **LCEL chains** map most directly onto Ulexite's imperative statement sequence (§7.3); a `prompt | model | parser` pipeline becomes a message-literal block ending in a typed binding — the `Runnable` uniform-interface idea (§2.3, §10.1) is the same idea Ulexite generalizes, so this is largely a mechanical translation of pipe stages into sequential statements.
- **`RunnableWithMessageHistory` + session-id config** collapses entirely: Ulexite's automatic structural history (§5.1) replaces it outright — a migration should *delete* the wrapper and session-store code, not port it.
- **LangGraph's `StateGraph`/reducers** map onto §9.5's merge-function declarations plus `with` blocks (§7.4) for the parallelizable subset of the graph, and onto ordinary imperative control flow (§7.3) for genuinely sequential/conditional nodes — a graph with heavy conditional routing (`add_conditional_edges`) migrates most naturally to `match` statements, not a forced `with` block.
- **Checkpointers** are unnecessary to port — every Ulexite run is checkpointed by default (§10.4); a team relying on LangGraph's `(thread_id, checkpoint_id)` model gets the equivalent for free.
- **LangSmith evals** migrate to `benchmark`/`judge` declarations (§16, §17); existing rubric text is largely reusable verbatim inside a `judge`'s `rubric:` field.

## 23.3 From Semantic Kernel

- **Plugins/`KernelFunction`s** map onto tool adapters (§12.6); the `[KernelFunction]`-attribute ceremony documented as verbose in §2.3 collapses to a plain tool declaration with a checked input/output artifact schema.
- **`ChatHistory`** collapses the same way LangChain's history wrapper does (§23.2) — replaced, not ported, by automatic structural history (§5.1).
- **Filters** are the one SK concept with a near-1:1 target: they map directly onto Ulexite's `next(context)`-style middleware extension point (§12.6), and porting filter logic is close to mechanical.
- Given SK's own maintenance-mode status and its successor's abandonment of the `Kernel` object entirely (§2.3), teams already mid-migration to Microsoft Agent Framework may find it lower-risk to evaluate Ulexite against Agent Framework directly rather than against legacy SK.

## 23.4 From DSPy

- **Signatures** map onto a `conversation`'s typed parameter/return declaration (§7.2) — the cleanest of any migration in this section, since DSPy's Signature concept and Ulexite's typed conversation boundary are solving the same problem the same way.
- **Modules** (`Predict`, `ChainOfThought`, `ReAct`) map onto conversation bodies with the corresponding control-flow shape written out explicitly (a `ChainOfThought`'s implicit `reasoning` field becomes an explicit intermediate binding, §7.3; a `ReAct` loop becomes a `while`/`match` loop over tool-call results, §21.6).
- **Optimizers** are not reimplemented — `optimize.mipro`/`optimize.bootstrap_demos` (§15.14) wrap the same technique, and an existing DSPy-compiled prompt artifact can, if desired, seed the corresponding Ulexite conversation's initial few-shot examples rather than being discarded.
- **`dspy.Evaluate`** maps onto `benchmark`/`dataset` (§16).

## 23.5 From Promptfoo / OpenAI Evals

- A `promptfooconfig.yaml`'s `tests` array or an OpenAI Evals JSONL dataset maps directly onto a `dataset` declaration (§7.2, §16.2) — this is close to a mechanical data-format conversion, not a redesign.
- `llm-rubric`/`ModelBasedClassify` assertions map onto `judge` declarations (§7.2, §15.2); deterministic assertions (`regex`, `json-schema`, `is-json`) map onto `validator` declarations (§7.2, §15.6).
- The thing that does **not** migrate mechanically is control flow buried in Nunjucks templates or an escape-hatch Python/JS grader function (§2.2) — this logic must be rewritten as ordinary Ulexite statements, which is the point: it is exactly the class of logic that had nowhere legitimate to live in a config-file format.

## 23.6 From LlamaIndex

- **`Workflow`'s typed-event routing** is the closest conceptual match to Ulexite's own step-to-step data flow (§2.3, §10.2) — a `@step` function's `Event -> Event` signature translates fairly directly to a sequence of typed bindings and `match` branches.
- **`ChatEngine`/`ChatMemoryBuffer`** collapse the same way every other framework's bespoke history object does (§5.1).
- **Retrieval/ingestion pipelines** (node parsers, retrievers, rerankers) are the one area where a migration should *keep* LlamaIndex, not replace it: register it as a `vector_index` capability provider (§12.4, §15.16) behind Ulexite's `vector`/`embedding` stdlib calls, rather than reimplementing years of retrieval engineering (§22.1).

## 23.7 From OpenAI Agents SDK

- **Agents/Handoffs** map onto nested conversations (§5.1, §21.5) — a handoff's model-visible `transfer_to_<agent>` tool call becomes an ordinary nested `conversation` invocation with routing expressed as `match`/`if` rather than a special handoff primitive.
- **Guardrails** map onto `validator`/`judge` declarations gating a `match` (§7.3, §9.4).
- **Sessions** collapse into automatic history (§5.1), as in every other migration path above.
- **Tracing** — because Ulexite's trace format (§18) can export to OpenTelemetry (§18.6), an existing investment in a tracing backend (Langfuse, Arize, etc.) connected to the Agents SDK's OTel-compatible export can often be pointed at Ulexite's export with minimal reconfiguration.

## 23.8 What never needs to migrate

Retrieval engines, vector databases, and provider SDKs (LlamaIndex's ingestion stack, LiteLLM's provider matrix, an existing Pinecone/Qdrant deployment) are not replaced — they become provider/tool plugins (§12.4, §12.6) behind Ulexite's capability interfaces, per §1.3's explicit non-goal of reinventing everything outside the conversation-orchestration domain.
