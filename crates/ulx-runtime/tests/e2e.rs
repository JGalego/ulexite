use std::collections::BTreeMap;

use ulx_ir::lower_program;
use ulx_runtime::value::{Value, Verdict};
use ulx_runtime::{Cache, ProviderRegistry, RunContext, RuntimeError, TraceWriter};

fn setup(src: &str, tmp: &tempfile::TempDir) -> ulx_ir::IrProgram {
    let program = ulx_syntax::parse_source(src).expect("must parse");
    let diags = ulx_sema::analyze(&program);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == ulx_sema::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "semantic errors: {errors:?}");
    let _ = tmp;
    lower_program(&program).expect("must lower")
}

fn make_ctx<'a>(
    program: &'a ulx_ir::IrProgram,
    tmp: &tempfile::TempDir,
    run_id: &str,
) -> RunContext<'a> {
    let cache = Cache::new(tmp.path().join("cache")).unwrap();
    let trace = TraceWriter::create(tmp.path().join("traces"), run_id).unwrap();
    RunContext::new(
        program,
        ProviderRegistry::with_mock(),
        cache,
        trace,
        run_id.to_string(),
        tmp.path().to_path_buf(),
    )
}

const TRANSLATE_SRC: &str = r#"
judge Fluency(subject: text) -> Verdict {
  rubric: """Is this fluent?"""
}

conversation Translate(source: text, target_lang: text) -> text {
  system: """You are a professional translator."""
  user: """Translate to {target_lang}: {source}"""
  assistant -> draft: text

  match judge Fluency(draft) {
    Pass          => draft
    Fail(reason)  => retry(2) {
                        user: """The previous translation was rejected: {reason}. Try again."""
                        assistant -> draft
                      } else escalate(human_approval, reason: reason)
    Escalate      => escalate(human_approval, reason: "judge could not decide")
    Score(_)      => draft
  }
}
"#;

#[test]
fn translate_happy_path_returns_text() {
    let tmp = tempfile::tempdir().unwrap();
    let program = setup(TRANSLATE_SRC, &tmp);
    let ctx = make_ctx(&program, &tmp, "run1");

    let mut args = BTreeMap::new();
    args.insert("source".to_string(), Value::Text("hello".to_string()));
    args.insert("target_lang".to_string(), Value::Text("fr".to_string()));

    let result = ulx_runtime::run_conversation(&ctx, "Translate", args).expect("should succeed");
    match result {
        Value::Text(s) => assert!(s.contains("mock:chat"), "unexpected output: {s}"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn repeated_call_is_a_cache_hit_across_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let program = setup(TRANSLATE_SRC, &tmp);

    let mut args = BTreeMap::new();
    args.insert("source".to_string(), Value::Text("hello".to_string()));
    args.insert("target_lang".to_string(), Value::Text("fr".to_string()));

    let ctx1 = make_ctx(&program, &tmp, "run_a");
    ulx_runtime::run_conversation(&ctx1, "Translate", args.clone()).unwrap();

    let ctx2 = make_ctx(&program, &tmp, "run_b");
    ulx_runtime::run_conversation(&ctx2, "Translate", args).unwrap();

    let trace2 = ulx_runtime::read_trace(tmp.path().join("traces"), "run_b").unwrap();
    assert!(
        trace2.iter().any(|r| r.cache_hit),
        "second run should have at least one cache hit, got: {trace2:#?}"
    );
}

/// Forces the judge's mock `Escalate` path via the documented test marker,
/// which routes straight into `escalate(human_approval, ...)` — the
/// `retry(2){...}` branch (the `Fail` arm) deliberately isn't exercised
/// here: this interpreter's `retry` (§7.3) only retries on the body
/// *erroring* (§9.3's `Draft<T>` non-settlement), not on re-checking a
/// judge verdict inside the loop — see `interp.rs`'s `eval_retry` docs —
/// so with a mock chat provider that never errors, `retry` always succeeds
/// after one attempt regardless of translation quality. `Escalate` is the
/// direct, unambiguous way to reach a suspend point.
#[test]
fn judge_escalate_suspends_then_resumes_after_approval() {
    let tmp = tempfile::tempdir().unwrap();
    let program = setup(TRANSLATE_SRC, &tmp);

    let mut args = BTreeMap::new();
    args.insert(
        "source".to_string(),
        Value::Text("MOCK_JUDGE_ESCALATE please".to_string()),
    );
    args.insert("target_lang".to_string(), Value::Text("fr".to_string()));

    let ctx = make_ctx(&program, &tmp, "run_suspend");
    let err = ulx_runtime::run_conversation(&ctx, "Translate", args.clone()).unwrap_err();
    let (cache_key, target) = match err {
        RuntimeError::Suspended {
            cache_key, target, ..
        } => (cache_key, target),
        other => panic!("expected Suspended, got {other:?}"),
    };
    assert_eq!(target, "human_approval");

    // Simulate `ulx approve <run_id> --value ...`: record a human decision
    // under the exact cache key the suspended run reported, then re-run.
    ctx.cache
        .put(
            &cache_key,
            &Value::Text("approved: use the last draft".to_string()),
        )
        .unwrap();

    let ctx2 = make_ctx(&program, &tmp, "run_suspend"); // same run_id, fresh context
    let result =
        ulx_runtime::run_conversation(&ctx2, "Translate", args).expect("should resume and succeed");
    assert_eq!(
        result,
        Value::Text("approved: use the last draft".to_string())
    );
}

/// Regression test for a real bug: escalate cache keys didn't mix in
/// `run_id`, so two unrelated runs reaching an identically-worded
/// `escalate(target, reason: "...")` (a fixed string literal, not derived
/// from the run's own arguments — exactly what `Translate`'s `Escalate`
/// arm does) would silently share one human decision across runs.
#[test]
fn escalate_decisions_do_not_leak_across_different_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let program = setup(TRANSLATE_SRC, &tmp);

    let mut args_a = BTreeMap::new();
    args_a.insert(
        "source".to_string(),
        Value::Text("MOCK_JUDGE_ESCALATE one".to_string()),
    );
    args_a.insert("target_lang".to_string(), Value::Text("fr".to_string()));

    let mut args_b = BTreeMap::new();
    args_b.insert(
        "source".to_string(),
        Value::Text("MOCK_JUDGE_ESCALATE two".to_string()),
    );
    args_b.insert("target_lang".to_string(), Value::Text("de".to_string()));

    let ctx_a = make_ctx(&program, &tmp, "run_a_escalate");
    let err_a = ulx_runtime::run_conversation(&ctx_a, "Translate", args_a).unwrap_err();
    let cache_key_a = match err_a {
        RuntimeError::Suspended { cache_key, .. } => cache_key,
        other => panic!("expected Suspended, got {other:?}"),
    };
    ctx_a
        .cache
        .put(&cache_key_a, &Value::Text("decision for run A".to_string()))
        .unwrap();

    // A *different* run reaching the exact same `escalate(human_approval,
    // reason: "judge could not decide")` call site must still suspend —
    // it must not see run A's decision.
    let ctx_b = make_ctx(&program, &tmp, "run_b_escalate");
    let err_b = ulx_runtime::run_conversation(&ctx_b, "Translate", args_b).unwrap_err();
    match err_b {
        RuntimeError::Suspended { cache_key, .. } => {
            assert_ne!(
                cache_key, cache_key_a,
                "different runs must not compute the same escalate cache key"
            );
        }
        other => {
            panic!("expected run B to also suspend (not reuse run A's decision), got {other:?}")
        }
    }
}

#[test]
fn with_block_runs_members_concurrently_and_binds_both() {
    let tmp = tempfile::tempdir().unwrap();
    let src = r#"
        conversation Summarize(doc: pdf) -> text {
          with {
            outline  = ask vision(doc) { user: """Extract an outline.""" }
            keyfacts = ask vision(doc) { user: """List facts.""" }
          }
          outline
        }
    "#;
    let program = setup(src, &tmp);
    let ctx = make_ctx(&program, &tmp, "run_with");

    let mut args = BTreeMap::new();
    args.insert("doc".to_string(), Value::Text("doc-bytes".to_string()));
    let result = ulx_runtime::run_conversation(&ctx, "Summarize", args).unwrap();
    match result {
        Value::Text(s) => assert!(s.contains("mock:vision")),
        other => panic!("expected Text, got {other:?}"),
    }

    let trace = ulx_runtime::read_trace(tmp.path().join("traces"), "run_with").unwrap();
    let vision_calls = trace
        .iter()
        .filter(|r| r.capability.as_deref() == Some("vision"))
        .count();
    assert_eq!(
        vision_calls, 2,
        "both with-block members should have executed: {trace:#?}"
    );
}

#[test]
fn dataset_and_vector_nearest_work_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let src = r#"
        dataset KnowledgeBase: [{doc_id: text, chunk: text, embedding: embedding<8>}] {
          from "kb.jsonl"
        }

        conversation AnsweredByRAG(question: text) -> text {
          q_embedding = embedding.of(question, model: capability(embed))
          top_chunks  = vector.nearest(query: q_embedding, index: KnowledgeBase, k: 1)
          ask chat() {
            system: """Answer only from context."""
            user: """Context:\n{top_chunks}\n\nQuestion: {question}"""
          } -> answer: text
          answer
        }
    "#;
    // A tiny inline JSONL fixture so `from "kb.jsonl"` resolves relative to
    // `base_dir` (the temp dir we pass into `RunContext`).
    std::fs::write(
        tmp.path().join("kb.jsonl"),
        r#"{"doc_id":"a","chunk":"about cats","embedding":[0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8]}
{"doc_id":"b","chunk":"about dogs","embedding":[0.9,0.8,0.7,0.6,0.5,0.4,0.3,0.2]}
"#,
    )
    .unwrap();

    let program = setup(src, &tmp);
    let ctx = make_ctx(&program, &tmp, "run_rag");
    let mut args = BTreeMap::new();
    args.insert(
        "question".to_string(),
        Value::Text("tell me about cats".to_string()),
    );
    let result =
        ulx_runtime::run_conversation(&ctx, "AnsweredByRAG", args).expect("should succeed");
    assert!(matches!(result, Value::Text(_)));
}

#[test]
fn validator_regex_is_real_not_mocked() {
    let tmp = tempfile::tempdir().unwrap();
    let src = r#"
        validator LooksLikeEmail(subject: text) -> Verdict {
          regex: """^[^@]+@[^@]+\.[^@]+$"""
        }

        conversation CheckEmail(addr: text) -> Verdict {
          validator LooksLikeEmail(addr)
        }
    "#;
    let program = setup(src, &tmp);
    let ctx = make_ctx(&program, &tmp, "run_validator");

    let mut ok_args = BTreeMap::new();
    ok_args.insert("addr".to_string(), Value::Text("a@b.com".to_string()));
    assert_eq!(
        ulx_runtime::run_conversation(&ctx, "CheckEmail", ok_args).unwrap(),
        Value::Verdict(Verdict::Pass)
    );

    let mut bad_args = BTreeMap::new();
    bad_args.insert("addr".to_string(), Value::Text("not-an-email".to_string()));
    match ulx_runtime::run_conversation(&ctx, "CheckEmail", bad_args).unwrap() {
        Value::Verdict(Verdict::Fail(_)) => {}
        other => panic!("expected Fail, got {other:?}"),
    }
}

#[test]
fn real_examples_run_or_fail_cleanly() {
    // Not every example is fully executable (some reference illustrative
    // dataset/file paths that don't exist), but every one must fail with a
    // clean `RuntimeError`, never a panic.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("ulx") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let program = ulx_syntax::parse_source(&src).unwrap();
        let ir = match lower_program(&program) {
            Ok(ir) => ir,
            Err(e) => panic!("{}: lowering failed: {e:?}", path.display()),
        };
        if ir.conversations.is_empty() {
            continue;
        }
        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::new(tmp.path().join("cache")).unwrap();
        let trace = TraceWriter::create(tmp.path().join("traces"), "smoke").unwrap();
        let ctx = RunContext::new(
            &ir,
            ProviderRegistry::with_mock(),
            cache,
            trace,
            "smoke".to_string(),
            tmp.path().to_path_buf(),
        );
        // Best-effort: call the first conversation with no arguments filled
        // in beyond what's required; a missing-argument TypeError is fine,
        // a panic is not.
        let conv = &ir.conversations[0];
        let args: BTreeMap<String, Value> = conv
            .params
            .iter()
            .map(|(name, _)| (name.clone(), Value::Text("smoke-test".to_string())))
            .collect();
        let _ = ulx_runtime::run_conversation(&ctx, &conv.name, args);
    }
}
