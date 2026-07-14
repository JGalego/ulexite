//! Correctness tests for `ulx_syntax::fmt` (`ulx fmt`, §20.10).
//!
//! `ulx fmt` is an AST-based pretty-printer, not a lossless formatter (see
//! `crates/ulx-syntax/src/fmt.rs`'s module docs) — comments are dropped and
//! source-level parentheses are recomputed from operator precedence rather
//! than preserved verbatim. So "round-trip" here means *semantic*
//! equivalence: parsing the formatted output must produce an AST equal to
//! the original one, ignoring spans (byte offsets necessarily change) and
//! `doc` comments (always `None` today — doc-comment capture is a stub
//! everywhere in the parser, so there's nothing to lose there either way).
//!
//! Two properties are checked for every file under `examples/`:
//!   1. Semantic round-trip: `parse(fmt(parse(src)))` == `parse(src)`
//!      (span-blind).
//!   2. Idempotency: `fmt(fmt(src))` == `fmt(src)` (byte-for-byte).

use std::path::Path;

use ulx_ast::Program;

fn examples_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn example_files() -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(examples_dir())
        .expect("examples/ directory must exist")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ulx"))
        .collect();
    files.sort();
    files
}

/// Replaces every `<digits>..<digits>` substring (the `Debug` rendering of
/// a `Span`/`std::ops::Range<usize>`) with a fixed placeholder, so two
/// `Debug` strings can be compared while ignoring byte offsets. Plain
/// numbers (e.g. `Int(42)`) are left untouched since they don't have the
/// `N..M` shape.
fn strip_spans(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j + 1 < chars.len() && chars[j] == '.' && chars[j + 1] == '.' {
                let mut k = j + 2;
                while k < chars.len() && chars[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j + 2 {
                    out.push_str("SPAN");
                    i = k;
                    continue;
                }
            }
            out.extend(&chars[start..j]);
            i = j;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn assert_ast_eq(expected: &Program, actual: &Program, context: &str) {
    let expected_s = strip_spans(&format!("{expected:#?}"));
    let actual_s = strip_spans(&format!("{actual:#?}"));
    assert_eq!(
        expected_s, actual_s,
        "{context}: AST changed across format round-trip (span-blind comparison)"
    );
}

#[test]
fn fmt_round_trips_every_example_semantically() {
    let files = example_files();
    assert!(
        files.len() >= 8,
        "expected at least 8 example files, found {}",
        files.len()
    );
    for path in &files {
        let src = std::fs::read_to_string(path).unwrap();
        let original = ulx_syntax::parse_source(&src)
            .unwrap_or_else(|e| panic!("{}: original file failed to parse: {e:#?}", path.display()));

        let formatted = ulx_syntax::format_program(&original);

        let reparsed = ulx_syntax::parse_source(&formatted).unwrap_or_else(|e| {
            panic!(
                "{}: formatted output failed to parse:\n---\n{formatted}\n---\nerrors: {e:#?}",
                path.display()
            )
        });

        assert_ast_eq(&original, &reparsed, &path.display().to_string());
    }
}

#[test]
fn fmt_is_idempotent_for_every_example() {
    for path in example_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        let once = ulx_syntax::format_source(&src)
            .unwrap_or_else(|e| panic!("{}: failed to format: {e:#?}", path.display()));
        let twice = ulx_syntax::format_source(&once).unwrap_or_else(|e| {
            panic!(
                "{}: formatted-once output failed to re-parse:\n---\n{once}\n---\nerrors: {e:#?}",
                path.display()
            )
        });
        assert_eq!(
            once,
            twice,
            "{}: formatting is not idempotent (format(format(src)) != format(src))",
            path.display()
        );
    }
}

#[test]
fn fmt_smoke_test_no_panics_and_reparseable() {
    // A lighter-weight duplicate of the two properties above, but phrased
    // as an explicit smoke test per-file so a failure names exactly which
    // example broke and shows the offending formatted source directly.
    for path in example_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        let formatted = ulx_syntax::format_source(&src)
            .unwrap_or_else(|e| panic!("{}: failed to format: {e:#?}", path.display()));
        assert!(
            !formatted.trim().is_empty(),
            "{}: formatter produced empty output for non-empty input",
            path.display()
        );
        ulx_syntax::parse_source(&formatted).unwrap_or_else(|e| {
            panic!(
                "{}: formatted output is not valid Ulexite source:\n---\n{formatted}\n---\nerrors: {e:#?}",
                path.display()
            )
        });
    }
}
