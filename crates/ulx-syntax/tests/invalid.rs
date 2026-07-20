//! Negative parser tests: deliberately malformed `.ulx` snippets that must
//! be rejected. These check the parser is not silently accepting garbage —
//! semantic checks (exhaustiveness, artifact-type routing, §9) aren't
//! implemented yet, so these only exercise syntax-level validity (§13.3).

fn assert_rejected(src: &str, label: &str) {
    let result = ulx_syntax::parse_source(src);
    assert!(
        result.is_err(),
        "expected `{label}` to fail to parse, but it succeeded: {result:?}"
    );
}

#[test]
fn unclosed_brace_is_rejected() {
    assert_rejected(
        r#"
        conversation Foo() -> text {
          system: """hi"""
        "#,
        "unclosed conversation body",
    );
}

#[test]
fn ask_stmt_dangling_arrow_is_rejected() {
    assert_rejected(
        r#"
        conversation Foo() -> text {
          ask chat() { user: """hi""" } ->
        }
        "#,
        "ask statement with `->` but no bind name",
    );
}

#[test]
fn match_arm_missing_fat_arrow_is_rejected() {
    assert_rejected(
        r#"
        conversation Foo() -> text {
          match draft {
            Pass draft
          }
        }
        "#,
        "match arm missing `=>`",
    );
}

#[test]
fn dataset_missing_type_is_rejected() {
    assert_rejected(
        r#"
        dataset Foo {
          from "x.jsonl"
        }
        "#,
        "dataset missing `: type`",
    );
}

#[test]
fn stray_top_level_token_is_rejected() {
    assert_rejected(
        "this is not a valid top-level form &&&",
        "garbage top-level input",
    );
}

#[test]
fn oversized_integer_literal_reports_overflow_not_unrecognized_character() {
    // §24.12: an integer literal with more digits than fit in an `i64` used
    // to be misreported as "unrecognized character" with a 1-byte span.
    let src = "conversation Foo() -> text {\n  99999999999999999999999\n}";
    let err = ulx_syntax::parse_source(src).expect_err("must fail to parse");
    let msg = ulx_syntax::format_error(&err[0]);
    assert!(
        msg.contains("too large to fit"),
        "expected an integer-overflow message, got: {msg}"
    );
}

#[test]
fn pathologically_deep_nesting_is_a_clean_parse_error_not_a_crash() {
    // §24.12: tens of thousands of nested `(` used to exhaust the parser's
    // stack and crash the process outright. 50_000 levels is well within
    // the range empirically observed to crash an unguarded parse.
    let depth = 50_000;
    let src = format!(
        "conversation Foo() -> int {{\n  {}1{}\n}}",
        "(".repeat(depth),
        ")".repeat(depth)
    );
    let err = ulx_syntax::parse_source(&src).expect_err("must be rejected, not crash");
    let msg = ulx_syntax::format_error(&err[0]);
    assert!(
        msg.contains("nesting depth exceeds"),
        "expected a nesting-depth error, got: {msg}"
    );
}
