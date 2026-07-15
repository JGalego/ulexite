use ulx_sema::Severity;

fn diags(src: &str) -> Vec<ulx_sema::Diagnostic> {
    let program = ulx_syntax::parse_source(src).expect("must parse");
    ulx_sema::analyze(&program)
}

fn has_error_containing(diags: &[ulx_sema::Diagnostic], needle: &str) -> bool {
    diags
        .iter()
        .any(|d| d.severity == Severity::Error && d.message.contains(needle))
}

#[test]
fn standalone_provider_without_vendor_is_rejected() {
    let src = r#"
        provider Broken {
          chat: "some-model"
        }
    "#;
    let d = diags(src);
    assert!(
        has_error_containing(&d, "no `from` and no `vendor`"),
        "expected a missing-vendor error, got: {d:?}"
    );
}

#[test]
fn standalone_provider_with_vendor_is_accepted() {
    let src = r#"
        provider Fine {
          vendor: "anthropic"
          chat: "claude-3-5-sonnet-20241022"
        }
    "#;
    let d = diags(src);
    assert!(d.is_empty(), "expected no diagnostics, got: {d:?}");
}

#[test]
fn provider_with_both_from_and_vendor_is_rejected() {
    let src = r#"
        provider Conflicted from "anthropic" {
          vendor: "openai"
          chat: "some-model"
        }
    "#;
    let d = diags(src);
    assert!(
        has_error_containing(&d, "declares both `from` and `vendor`"),
        "expected a from/vendor conflict error, got: {d:?}"
    );
}

#[test]
fn provider_with_duplicate_field_is_rejected() {
    let src = r#"
        provider Dup {
          vendor: "anthropic"
          chat: "claude-3-5-sonnet-20241022"
          chat: "claude-3-opus-20240229"
        }
    "#;
    let d = diags(src);
    assert!(
        has_error_containing(&d, "more than one `chat` field"),
        "expected a duplicate-field error, got: {d:?}"
    );
}

#[test]
fn provider_capability_record_with_non_literal_field_is_rejected() {
    let src = r#"
        conversation UsesIt() -> text {
          "unused"
        }

        provider Bad {
          vendor: "anthropic"
          chat: { model: "claude-3-5-sonnet-20241022", weird: UsesIt() }
        }
    "#;
    let d = diags(src);
    assert!(
        has_error_containing(&d, "must be a plain string, int, or float"),
        "expected a non-literal capability field error, got: {d:?}"
    );
}

#[test]
fn provider_capability_bad_shape_is_rejected() {
    let src = r#"
        provider Bad {
          vendor: "anthropic"
          chat: 42
        }
    "#;
    let d = diags(src);
    assert!(
        has_error_containing(&d, "must be a bare model-name string"),
        "expected a bad capability shape error, got: {d:?}"
    );
}

#[test]
fn from_reference_is_validated_against_known_manifest_providers_when_given() {
    let dir = std::env::temp_dir().join(format!("ulexite-sema-from-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.ulx");
    std::fs::write(
        &file,
        r#"
        provider MyAnthropic from "anthropic" {
          chat: "claude-3-5-sonnet-20241022"
        }
        "#,
    )
    .unwrap();

    let mut known = std::collections::HashSet::new();
    known.insert("anthropic".to_string());
    let ws = ulx_sema::analyze_file(&file, Some(&known)).expect("must load");
    let ok_diags: Vec<_> = ws
        .modules
        .values()
        .flat_map(|m| m.diagnostics.iter())
        .collect();
    assert!(
        ok_diags.is_empty(),
        "expected no diagnostics when `anthropic` is a known manifest entry, got: {ok_diags:?}"
    );

    let empty = std::collections::HashSet::new();
    let ws = ulx_sema::analyze_file(&file, Some(&empty)).expect("must load");
    let bad_diags: Vec<_> = ws
        .modules
        .values()
        .flat_map(|m| m.diagnostics.iter())
        .collect();
    assert!(
        bad_diags
            .iter()
            .any(|d| d.message.contains("no `[providers.anthropic]` entry")),
        "expected a missing-manifest-entry error when `anthropic` isn't known, got: {bad_diags:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn video_into_chat_is_rejected() {
    // §11.5's worked example: chat() only accepts text/markdown/json/image.
    let src = r#"
        conversation Caption(clip: video) -> text {
          ask chat(clip) { user: """Summarize this clip.""" } -> bad: text
        }
    "#;
    let d = diags(src);
    assert!(
        has_error_containing(&d, "accepts"),
        "expected an artifact-type routing error, got: {d:?}"
    );
}

#[test]
fn video_into_vision_is_accepted() {
    let src = r#"
        conversation Caption(clip: video) -> text {
          ask vision(clip) { user: """Describe this clip.""" } -> caption: text
          caption
        }
    "#;
    let d = diags(src);
    assert!(
        !has_error_containing(&d, "accepts"),
        "did not expect a routing error, got: {d:?}"
    );
}

#[test]
fn non_exhaustive_verdict_match_is_rejected() {
    let src = r#"
        judge Fluency(subject: text) -> Verdict {
          rubric: """is it fluent"""
        }
        conversation Translate(source: text) -> text {
          assistant -> draft: text
          match judge Fluency(draft) {
            Pass => draft
          }
        }
    "#;
    let d = diags(src);
    assert!(
        has_error_containing(&d, "non-exhaustive match"),
        "expected an exhaustiveness error, got: {d:?}"
    );
}

#[test]
fn exhaustive_verdict_match_with_wildcard_is_accepted() {
    let src = r#"
        judge Fluency(subject: text) -> Verdict {
          rubric: """is it fluent"""
        }
        conversation Translate(source: text) -> text {
          assistant -> draft: text
          match judge Fluency(draft) {
            Pass => draft
            _ => draft
          }
        }
    "#;
    let d = diags(src);
    assert!(
        !has_error_containing(&d, "non-exhaustive"),
        "did not expect an exhaustiveness error, got: {d:?}"
    );
}

#[test]
fn with_block_sibling_reference_is_rejected() {
    let src = r#"
        conversation Summarize(doc: pdf) -> text {
          with {
            outline = ask vision(doc) { user: """Extract an outline.""" }
            combined = outline
          }
          combined
        }
    "#;
    let d = diags(src);
    assert!(
        has_error_containing(&d, "sibling binding"),
        "expected a with-block independence error, got: {d:?}"
    );
}

#[test]
fn independent_with_block_is_accepted() {
    let src = r#"
        conversation Summarize(doc: pdf) -> text {
          with {
            outline  = ask vision(doc) { user: """Extract an outline.""" }
            keyfacts = ask vision(doc) { user: """List facts.""" }
          }
          outline
        }
    "#;
    let d = diags(src);
    assert!(
        !has_error_containing(&d, "sibling binding"),
        "did not expect an independence error, got: {d:?}"
    );
}

#[test]
fn duplicate_top_level_name_is_rejected() {
    let src = r#"
        conversation Foo() -> text {
          assistant -> x: text
          x
        }
        conversation Foo() -> text {
          assistant -> y: text
          y
        }
    "#;
    let d = diags(src);
    assert!(
        has_error_containing(&d, "duplicate top-level declaration"),
        "expected a duplicate-name error, got: {d:?}"
    );
}

/// Regression test for a real bug: `if`/`retry`/`ask` bodies were checked
/// against a *fresh* scope instead of extending the enclosing one, so
/// conversation params and match-arm pattern bindings (e.g. `reason` in
/// `Fail(reason) => retry(2) { ...{reason}... }`) spuriously looked
/// undefined inside them. This only surfaces as a *warning* (undefined-name
/// checking needs `globals`, i.e. the `load_and_analyze` workspace path),
/// which `real_examples_have_no_errors` below doesn't check for — hence a
/// dedicated test asserting zero warnings, not just zero errors.
#[test]
fn nested_blocks_see_enclosing_scope_bindings() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/translate.ulx");
    let ws = ulx_sema::analyze_file(&dir, None).expect("must load");
    for m in ws.modules.values() {
        assert!(
            m.diagnostics.is_empty(),
            "{} produced diagnostics: {:#?}",
            m.path.display(),
            m.diagnostics
        );
    }
}

/// Regression test for a real bug found by actually running the examples:
/// `multi_agent.ulx` called `judge Quality(report)` without ever declaring
/// a `judge Quality` — sema had no check for this at all (it only checked
/// `ask` capability names), so the mistake only surfaced as a runtime
/// error. This is the workspace-path check that now catches it statically.
#[test]
fn undeclared_judge_reference_is_flagged() {
    let file = std::env::temp_dir().join("ulx_sema_test_undeclared_judge.ulx");
    std::fs::write(
        &file,
        r#"
        conversation Foo(x: text) -> Verdict {
          judge NoSuchJudge(x)
        }
        "#,
    )
    .unwrap();
    let ws = ulx_sema::analyze_file(&file, None).expect("must load");
    let all_diags: Vec<_> = ws
        .modules
        .values()
        .flat_map(|m| m.diagnostics.iter())
        .collect();
    assert!(
        all_diags.iter().any(|d| d
            .message
            .contains("not declared as a `judge` or `validator`")),
        "expected an undeclared-judge diagnostic, got: {all_diags:?}"
    );
    let _ = std::fs::remove_file(&file);
}

#[test]
fn capability_hint_argument_is_not_flagged_as_undefined() {
    let file = std::env::temp_dir().join("ulx_sema_test_capability_hint.ulx");
    std::fs::write(
        &file,
        r#"
        conversation Foo(x: text) -> text {
          y = capability(embed)
          x
        }
        "#,
    )
    .unwrap();
    let ws = ulx_sema::analyze_file(&file, None).expect("must load");
    let all_diags: Vec<_> = ws
        .modules
        .values()
        .flat_map(|m| m.diagnostics.iter())
        .collect();
    assert!(
        !all_diags.iter().any(|d| d.message.contains("`embed`")),
        "capability(embed)'s argument should not be flagged as undefined, got: {all_diags:?}"
    );
    let _ = std::fs::remove_file(&file);
}

#[test]
fn real_examples_have_no_errors() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("ulx") {
            continue;
        }
        match ulx_sema::analyze_file(&path, None) {
            Ok(ws) => {
                for m in ws.modules.values() {
                    let errors: Vec<_> = m
                        .diagnostics
                        .iter()
                        .filter(|d| d.severity == Severity::Error)
                        .collect();
                    assert!(
                        errors.is_empty(),
                        "{} produced semantic errors: {errors:#?}",
                        m.path.display()
                    );
                }
            }
            Err(e) => panic!("{}: {e}", path.display()),
        }
    }
}

/// Stronger than `real_examples_have_no_errors`: every shipped example
/// should be entirely diagnostic-free (no warnings either), the same bar
/// `ulx check` reports against. This is what actually caught the
/// undeclared-`judge Quality`-reference bug in `multi_agent.ulx` and the
/// `capability(embed)` false-positive in `rag.ulx` — both were only
/// warnings, so the error-only check above didn't (and, by design,
/// shouldn't have to) catch them.
#[test]
fn real_examples_have_no_diagnostics_at_all() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("ulx") {
            continue;
        }
        let ws = ulx_sema::analyze_file(&path, None)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for m in ws.modules.values() {
            assert!(
                m.diagnostics.is_empty(),
                "{} produced diagnostics: {:#?}",
                m.path.display(),
                m.diagnostics
            );
        }
    }
}

/// A `from "..."` import whose first path segment (`"other"`) names a
/// `path` dependency (`ulexite.toml`'s `[dependencies.other] path = "..."`,
/// translated by `ulx-cli`'s `pipeline::dependency_paths` into
/// `DependencyPaths::path_deps`) should resolve against that dependency's
/// directory rather than the importing file's own — i.e. `ulexite.toml`'s
/// `[dependencies]` table actually does something for cross-file imports.
#[test]
fn path_dependency_import_resolves_into_dependency_directory() {
    let base =
        std::env::temp_dir().join(format!("ulexite-sema-path-dep-test-{}", std::process::id()));
    let main_dir = base.join("main-pkg");
    let other_dir = base.join("other-pkg");
    std::fs::create_dir_all(&main_dir).unwrap();
    std::fs::create_dir_all(&other_dir).unwrap();

    let other_file = other_dir.join("shared.ulx");
    std::fs::write(
        &other_file,
        r#"
        judge Fluency(subject: text) -> Verdict {
          rubric: """is it fluent"""
        }
        "#,
    )
    .unwrap();

    let main_file = main_dir.join("main.ulx");
    std::fs::write(
        &main_file,
        r#"
        import judge Fluency from "other/shared.ulx"

        conversation UseIt(x: text) -> Verdict {
          judge Fluency(x)
        }
        "#,
    )
    .unwrap();

    let mut deps = ulx_sema::DependencyPaths::default();
    deps.path_deps.insert("other".to_string(), other_dir);

    let ws = ulx_sema::analyze_file_with_deps(&main_file, None, &deps).expect("must load");

    let all_diags: Vec<_> = ws
        .modules
        .values()
        .flat_map(|m| m.diagnostics.iter())
        .collect();
    assert!(
        all_diags.is_empty(),
        "expected no diagnostics resolving a path dependency, got: {all_diags:?}"
    );

    let expected_target = other_file.canonicalize().unwrap();
    assert!(
        ws.modules.contains_key(&expected_target),
        "expected workspace to contain the dependency's `shared.ulx` at {}, got modules: {:?}",
        expected_target.display(),
        ws.modules.keys().collect::<Vec<_>>()
    );

    std::fs::remove_dir_all(&base).ok();
}

/// A dependency declared with only `git` (no `path`) can't be fetched by
/// this v0.1 — an import referencing it should fail with a clear,
/// unambiguous error naming the dependency, not silently fall through to
/// relative-path resolution and fail with a confusing "file not found".
#[test]
fn import_referencing_git_only_dependency_fails_clearly() {
    let base =
        std::env::temp_dir().join(format!("ulexite-sema-git-dep-test-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let main_file = base.join("main.ulx");
    std::fs::write(
        &main_file,
        r#"
        import judge Fluency from "other/shared.ulx"
        "#,
    )
    .unwrap();

    let mut deps = ulx_sema::DependencyPaths::default();
    deps.unresolvable.insert("other".to_string());

    let err = match ulx_sema::analyze_file_with_deps(&main_file, None, &deps) {
        Ok(_) => panic!("expected resolving a git-only dependency to fail"),
        Err(e) => e,
    };
    assert!(
        err.contains("dependency `other` has no `path`") && err.contains("not implemented"),
        "expected a clear not-implemented error naming the dependency, got: {err}"
    );

    std::fs::remove_dir_all(&base).ok();
}

/// An import whose first segment doesn't match *any* declared dependency
/// name (listed or not) must behave exactly as it did before dependency
/// resolution existed — plain relative-path resolution, failing with the
/// same "could not read" error it always has. No regression for
/// single-package projects, and no accidental matching against unrelated
/// dependency names.
#[test]
fn import_referencing_unlisted_package_name_fails_like_before() {
    let base = std::env::temp_dir().join(format!(
        "ulexite-sema-unlisted-dep-test-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&base).unwrap();
    let main_file = base.join("main.ulx");
    std::fs::write(
        &main_file,
        r#"
        import judge Fluency from "nope/shared.ulx"
        "#,
    )
    .unwrap();

    // No dependencies declared at all — today's plain behavior.
    let no_deps_err = match ulx_sema::analyze_file(&main_file, None) {
        Ok(_) => panic!("expected resolving a nonexistent relative import to fail"),
        Err(e) => e,
    };
    assert!(
        no_deps_err.contains("could not read"),
        "expected a plain file-not-found error, got: {no_deps_err}"
    );

    // An unrelated dependency *is* declared, but "nope" doesn't match it —
    // must fail identically to the no-dependencies case.
    let mut deps = ulx_sema::DependencyPaths::default();
    deps.path_deps
        .insert("unrelated".to_string(), base.join("unrelated-pkg"));
    let with_unrelated_deps_err = match ulx_sema::analyze_file_with_deps(&main_file, None, &deps) {
        Ok(_) => panic!("expected resolving a nonexistent relative import to fail"),
        Err(e) => e,
    };
    assert_eq!(no_deps_err, with_unrelated_deps_err);

    std::fs::remove_dir_all(&base).ok();
}

/// `file("...")`/`@path` (§8 `file_expr`) needs a real `.ulx` file on disk
/// to resolve against (`base_dir`), so these tests go through
/// `load_and_analyze` rather than the no-filesystem-home `analyze()` used
/// by `diags()` above.
fn workspace_diags(main_file: &std::path::Path) -> Vec<ulx_sema::Diagnostic> {
    let ws = ulx_sema::load_and_analyze(main_file, None).expect("must load");
    ws.modules
        .values()
        .flat_map(|m| m.diagnostics.clone())
        .collect()
}

#[test]
fn file_prompt_with_valid_interpolation_has_no_diagnostics() {
    let base = std::env::temp_dir().join(format!("ulexite-sema-file-ok-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("user.txt"), "Hi {name}, welcome to {occasion}.").unwrap();
    let main_file = base.join("main.ulx");
    std::fs::write(
        &main_file,
        r#"
        conversation Greet(name: text, occasion: text) -> text {
          system: @user.txt
          user: file("user.txt")
          assistant -> reply: text
          reply
        }
        "#,
    )
    .unwrap();

    let d = workspace_diags(&main_file);
    assert!(d.is_empty(), "expected no diagnostics, got: {d:?}");

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn undefined_variable_inside_a_loaded_prompt_file_is_flagged_against_that_file() {
    let base =
        std::env::temp_dir().join(format!("ulexite-sema-file-badvar-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let prompt_file = base.join("user.txt");
    std::fs::write(&prompt_file, "Hi {name}, welcome to {oopsUndefined}.").unwrap();
    let main_file = base.join("main.ulx");
    std::fs::write(
        &main_file,
        r#"
        conversation Greet(name: text) -> text {
          system: """You are nice."""
          user: file("user.txt")
          assistant -> reply: text
          reply
        }
        "#,
    )
    .unwrap();

    let d = workspace_diags(&main_file);
    assert!(
        d.iter().any(|diag| diag.message.contains("oopsUndefined")),
        "expected an undefined-variable diagnostic, got: {d:?}"
    );
    let expected_path = prompt_file.canonicalize().unwrap();
    assert!(
        d.iter()
            .any(|diag| diag.source_file.as_deref() == Some(expected_path.as_path())),
        "expected the diagnostic's source_file to point at the prompt file {}, got: {d:?}",
        expected_path.display()
    );

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn missing_prompt_file_is_a_clear_diagnostic() {
    let base =
        std::env::temp_dir().join(format!("ulexite-sema-file-missing-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let main_file = base.join("main.ulx");
    std::fs::write(
        &main_file,
        r#"
        conversation Greet(name: text) -> text {
          system: file("does_not_exist.txt")
          user: """Hi {name}"""
          assistant -> reply: text
          reply
        }
        "#,
    )
    .unwrap();

    let d = workspace_diags(&main_file);
    assert!(
        has_error_containing(&d, "could not read prompt file"),
        "expected a missing-file error, got: {d:?}"
    );

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn with_block_sibling_reference_through_a_loaded_file_is_still_rejected() {
    let base = std::env::temp_dir().join(format!(
        "ulexite-sema-file-with-sibling-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&base).unwrap();
    // References `{outline}`, a sibling `with`-binding — §9.7 forbids this
    // regardless of whether the referencing text is inline or file-sourced.
    std::fs::write(base.join("combined.txt"), "Combined: {outline}").unwrap();
    let main_file = base.join("main.ulx");
    std::fs::write(
        &main_file,
        r#"
        conversation Summarize(doc: pdf) -> text {
          with {
            outline = ask vision(doc) { user: """Extract an outline.""" }
            combined = file("combined.txt")
          }
          combined
        }
        "#,
    )
    .unwrap();

    let d = workspace_diags(&main_file);
    assert!(
        has_error_containing(&d, "sibling binding"),
        "expected a with-block independence error, got: {d:?}"
    );

    std::fs::remove_dir_all(&base).ok();
}
