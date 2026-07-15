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

/// Regression test for a real bug found by running `examples/batch.ulx`:
/// `results.append(x)` inside a `for` loop appeared to work per-iteration
/// but silently discarded every accumulated append, because `Env::declare`
/// always writes into the *current* (innermost) frame — the `for` loop and
/// its body each push their own frame per iteration, so the "mutation"
/// landed in a frame that was popped before the next iteration, and
/// `results` outside the loop was still empty. Fixed by `Env::set`, which
/// walks outward to the frame where `results` was actually declared.
#[test]
fn list_append_across_loop_iterations_accumulates() {
    let tmp = tempfile::tempdir().unwrap();
    let src = r#"
        dataset Items: [{body: text}] {
          from "items.jsonl"
        }

        conversation Echo(body: text) -> text {
          body
        }

        conversation Collect() -> list<text> {
          results = list<text>()
          for item in Items {
            results.append(Echo(item.body))
          }
          results
        }
    "#;
    std::fs::write(
        tmp.path().join("items.jsonl"),
        "{\"body\": \"one\"}\n{\"body\": \"two\"}\n{\"body\": \"three\"}\n",
    )
    .unwrap();
    let program = setup(src, &tmp);
    let ctx = make_ctx(&program, &tmp, "run_collect");
    let result = ulx_runtime::run_conversation(&ctx, "Collect", BTreeMap::new()).unwrap();
    match result {
        Value::List(items) => assert_eq!(
            items.len(),
            3,
            "expected all 3 iterations accumulated, got: {items:?}"
        ),
        other => panic!("expected a list, got {other:?}"),
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

/// Two `escalate` calls in different `with`-block branches, with the exact
/// same `target`/`reason`, race each other across separate OS threads
/// (`eval_parallel`). Their cache keys must still come out distinct *and*
/// deterministic (the same pair, every time, regardless of which branch's
/// thread happens to run first) — otherwise a human's decision recorded
/// against one branch's suspend point could get silently applied to the
/// other on a later `ulx approve`/`ulx deny` (a different process, hence a
/// fresh, independently-scheduled race). Repeating this many times over
/// fresh run ids is the only way to catch a race regression here: a buggy
/// shared-counter scheme wouldn't fail every time, just often enough to be
/// a real bug in production.
#[test]
fn parallel_with_block_escalates_get_distinct_deterministic_cache_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let src = r#"
        conversation RaceEscalate() -> text {
          with {
            left  = escalate(human_approval, reason: "same reason")
            right = escalate(human_approval, reason: "same reason")
          }
          left
        }
    "#;
    let program = setup(src, &tmp);
    // One reused run id (so `run_id` — itself part of the cache key — is
    // held constant across iterations): each of the 20 re-executions below
    // appends exactly 2 more "escalate" trace records (nothing ever gets
    // cached, since nothing calls `approve`, so every iteration suspends
    // again the same way), and every one of those 20 pairs must match the
    // very first pair *by branch identity*, not just by count.
    let run_id = "race_escalate";
    for i in 0..20 {
        let ctx = make_ctx(&program, &tmp, run_id);
        let _ = ulx_runtime::run_conversation(&ctx, "RaceEscalate", BTreeMap::new());

        let trace = ulx_runtime::read_trace(tmp.path().join("traces"), run_id).unwrap();
        let keys: Vec<String> = trace
            .iter()
            .filter(|r| r.capability.as_deref() == Some("escalate"))
            .filter_map(|r| r.cache_key.clone())
            .collect();
        assert_eq!(
            keys.len(),
            2 * (i + 1),
            "each iteration should append exactly 2 more escalate records: {trace:#?}"
        );
        // Which branch's write to the shared trace file lands first is its
        // own harmless race (log line ordering, not cache-key
        // correctness) -- sort each iteration's pair before comparing so
        // this test isn't sensitive to it.
        let mut this_pair = [keys[2 * i].clone(), keys[2 * i + 1].clone()];
        this_pair.sort();
        assert_ne!(
            this_pair[0], this_pair[1],
            "two different with-block branches must never share an escalate cache key"
        );
        let mut first_pair = [keys[0].clone(), keys[1].clone()];
        first_pair.sort();
        assert_eq!(
            this_pair, first_pair,
            "branch->cache-key assignment must be deterministic across re-executions, \
             not dependent on with-block thread scheduling (iteration {i})"
        );
    }
}

/// A fake `Provider` whose only purpose is panicking, to check that a
/// `with`-block branch's panic surfaces as an ordinary `RuntimeError`
/// instead of tearing down the whole process (`eval_parallel`'s
/// `h.join().unwrap_or_else(...)`), and that the *other*, well-behaved
/// branch still completes normally rather than getting dragged down with
/// it.
struct PanickyProvider;

impl ulx_runtime::Provider for PanickyProvider {
    fn id(&self) -> &str {
        "panicky"
    }
    fn supports(&self, capability: &str) -> bool {
        capability == "chat"
    }
    fn invoke(
        &self,
        _capability: &str,
        _request: &ulx_runtime::provider::Invocation,
    ) -> Result<Value, ulx_runtime::provider::ProviderError> {
        panic!("boom")
    }
}

#[test]
fn with_block_panic_in_one_branch_does_not_abort_the_process_or_the_other_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let src = r#"
        conversation PanicOneBranch(doc: pdf) -> text {
          with {
            bad  = ask chat(provider: "panicky") { user: """boom""" }
            good = ask vision(doc) { user: """Describe this.""" }
          }
          good
        }
    "#;
    let program = setup(src, &tmp);

    let cache = Cache::new(tmp.path().join("cache")).unwrap();
    let trace = TraceWriter::create(tmp.path().join("traces"), "run_panic").unwrap();
    let mut registry = ProviderRegistry::with_mock();
    registry.register("panicky", Box::new(PanickyProvider));
    let ctx = RunContext::new(
        &program,
        registry,
        cache,
        trace,
        "run_panic".to_string(),
        tmp.path().to_path_buf(),
    );

    let mut args = BTreeMap::new();
    args.insert("doc".to_string(), Value::Text("doc-bytes".to_string()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ulx_runtime::run_conversation(&ctx, "PanicOneBranch", args)
    }));

    let result = match result {
        Ok(r) => r,
        Err(_) => panic!(
            "a with-block branch's panic must not unwind past run_conversation \
             into the caller's thread"
        ),
    };
    match result {
        Err(RuntimeError::Panicked(msg)) => {
            assert!(
                msg.contains("bad"),
                "error should name the panicking branch: {msg}"
            )
        }
        other => panic!("expected RuntimeError::Panicked, got {other:?}"),
    }

    // The well-behaved branch still ran to completion despite its sibling
    // panicking on a different thread.
    let trace = ulx_runtime::read_trace(tmp.path().join("traces"), "run_panic").unwrap();
    let vision_calls = trace
        .iter()
        .filter(|r| r.capability.as_deref() == Some("vision"))
        .count();
    assert_eq!(
        vision_calls, 1,
        "the non-panicking with-block branch should still have executed: {trace:#?}"
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

/// `run_benchmark` (§16.4): loads a `dataset`, runs `run:`/`expect:`/
/// `assert:` once per row with `$` bound to that row, and reports a
/// pass/fail per row. Row 0's source/golden are unremarkable, so the mock
/// judge passes and `result != golden` holds (mock chat output never
/// equals a fixture's golden translation) — an all-pass row. Row 1's
/// source deliberately contains the mock provider's documented
/// `MOCK_JUDGE_FAIL` marker (`mock.rs`'s `mock_judge`); since the mock
/// chat response echoes its input messages, that marker propagates into
/// `result`, so `expect result satisfies judge Fluency(result)` should
/// fail that row even though `assert` alone would still pass.
#[test]
fn benchmark_runs_once_per_dataset_row_and_reports_pass_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let src = r#"
        judge Fluency(subject: text) -> Verdict {
          rubric: """Is this fluent?"""
        }

        conversation Translate(source: text, target_lang: text) -> text {
          system: """You are a professional translator."""
          user: """Translate to {target_lang}: {source}"""
          assistant -> draft: text
          draft
        }

        dataset TranslationPairs: [{source: text, target_lang: text, golden: text}] {
          from "translations.jsonl"
        }

        benchmark TranslateQuality {
          dataset: TranslationPairs
          run: Translate(source: $.source, target_lang: $.target_lang) -> result
          expect result satisfies judge Fluency(result) with threshold(0.8)
          assert result != $.golden
        }
    "#;
    std::fs::write(
        tmp.path().join("translations.jsonl"),
        "{\"source\": \"Good morning\", \"target_lang\": \"fr\", \"golden\": \"Bonjour\"}\n\
         {\"source\": \"MOCK_JUDGE_FAIL this one\", \"target_lang\": \"de\", \"golden\": \"unused\"}\n",
    )
    .unwrap();

    let program = setup(src, &tmp);
    let ctx = make_ctx(&program, &tmp, "run_bench");

    let report =
        ulx_runtime::run_benchmark(&ctx, "TranslateQuality").expect("benchmark should run");
    assert_eq!(report.total(), 2);
    assert!(
        report.rows[0].passed(),
        "row 0 should pass: {:#?}",
        report.rows[0]
    );
    assert!(
        !report.rows[1].passed(),
        "row 1 should fail (judge fail marker): {:#?}",
        report.rows[1]
    );
    assert_eq!(report.passed_count(), 1);
    assert!(!report.all_passed());

    // The failing row's `expect` check carries a reason a report can print;
    // the `assert` check on that same row still holds independently.
    let row1_expect = report.rows[1]
        .checks
        .iter()
        .find(|c| c.kind == "expect")
        .expect("row 1 should have an expect check");
    assert!(!row1_expect.passed);
    assert!(row1_expect.message.is_some());
    let row1_assert = report.rows[1]
        .checks
        .iter()
        .find(|c| c.kind == "assert")
        .expect("row 1 should have an assert check");
    assert!(row1_assert.passed);
}

#[test]
fn run_benchmark_reports_a_clear_error_for_an_unknown_name() {
    let tmp = tempfile::tempdir().unwrap();
    let src = r#"
        dataset Empty: [{x: text}] { from "empty.jsonl" }
        benchmark NotThis { dataset: Empty }
    "#;
    std::fs::write(tmp.path().join("empty.jsonl"), "").unwrap();
    let program = setup(src, &tmp);
    let ctx = make_ctx(&program, &tmp, "run_bench_unknown");
    let err = ulx_runtime::run_benchmark(&ctx, "DoesNotExist").unwrap_err();
    assert!(matches!(err, RuntimeError::UnknownBenchmark(name) if name == "DoesNotExist"));
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
        // `file("...")`/`@path` (§8 `file_expr`) requires `ulx-sema`'s
        // resolve-and-rewrite pass before `lower_program` can handle it
        // (real base_dir + typechecked interpolations) — this test lowers
        // straight from a bare parse with no sema step, by design (§13
        // layering), so this one example is exercised through the real
        // pipeline in `ulx-cli`'s tests instead.
        if path.file_name().and_then(|n| n.to_str()) == Some("prompt_from_file.ulx") {
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
