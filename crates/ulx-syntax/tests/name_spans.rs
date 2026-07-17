//! Every top-level declaration's `name_span` (and `Param::name_span`)
//! should cover exactly the name identifier's own bytes in the source —
//! not the whole declaration, not the type/params that follow it. This is
//! what lets `ulx-lsp`'s goto-definition land the cursor on just `Foo`
//! rather than selecting `conversation Foo(...) { ... }` in full.

use ulx_ast::TopDecl;

fn name_span_text<'a>(src: &'a str, span: &std::ops::Range<usize>) -> &'a str {
    &src[span.clone()]
}

#[test]
fn conversation_name_span_is_exactly_the_identifier() {
    let src = "conversation Translate(source: text) -> text {\n  source\n}\n";
    let program = ulx_syntax::parse_source(src).expect("must parse");
    let TopDecl::Conversation(c) = &program.decls[0].0 else {
        panic!("expected a conversation decl");
    };
    assert_eq!(name_span_text(src, &c.name_span), "Translate");
    // The name span must be strictly smaller than the whole declaration's
    // own span (the second element of the `Spanned<TopDecl>` pair).
    let whole_span = &program.decls[0].1;
    assert!(c.name_span.end - c.name_span.start < whole_span.end - whole_span.start);
}

#[test]
fn param_name_span_is_exactly_the_identifier_not_the_type() {
    let src = "conversation Greet(name: text) -> text {\n  name\n}\n";
    let program = ulx_syntax::parse_source(src).expect("must parse");
    let TopDecl::Conversation(c) = &program.decls[0].0 else {
        panic!("expected a conversation decl");
    };
    let param = &c.params[0];
    assert_eq!(name_span_text(src, &param.name_span), "name");
}

#[test]
fn judge_and_validator_name_spans_are_precise() {
    let src = r#"
        judge Fluency(subject: text) -> Verdict {
          rubric: """Is this fluent?"""
        }
        validator NonEmpty(subject: text) -> Verdict {
          regex: "."
        }
    "#;
    let program = ulx_syntax::parse_source(src).expect("must parse");
    let mut saw_judge = false;
    let mut saw_validator = false;
    for (decl, _) in &program.decls {
        match decl {
            TopDecl::Judge(r) => {
                assert_eq!(name_span_text(src, &r.name_span), "Fluency");
                saw_judge = true;
            }
            TopDecl::Validator(r) => {
                assert_eq!(name_span_text(src, &r.name_span), "NonEmpty");
                saw_validator = true;
            }
            _ => {}
        }
    }
    assert!(saw_judge && saw_validator);
}

#[test]
fn dataset_type_and_benchmark_name_spans_are_precise() {
    let src = r#"
        type Pair = { a: text, b: text }
        dataset Rows: [{x: text}] { from "rows.jsonl" }
        conversation Echo(x: text) -> text { x }
        benchmark EchoBench {
          dataset: Rows
          run: Echo(x: $.x) -> result
        }
    "#;
    let program = ulx_syntax::parse_source(src).expect("must parse");
    let mut seen = std::collections::HashSet::new();
    for (decl, _) in &program.decls {
        match decl {
            TopDecl::Type(t) => {
                assert_eq!(name_span_text(src, &t.name_span), "Pair");
                seen.insert("type");
            }
            TopDecl::Dataset(d) => {
                assert_eq!(name_span_text(src, &d.name_span), "Rows");
                seen.insert("dataset");
            }
            TopDecl::Benchmark(b) => {
                assert_eq!(name_span_text(src, &b.name_span), "EchoBench");
                seen.insert("benchmark");
            }
            _ => {}
        }
    }
    assert_eq!(
        seen.len(),
        3,
        "expected type/dataset/benchmark all checked, got {seen:?}"
    );
}

#[test]
fn provider_name_span_is_precise() {
    let src = r#"
        provider LocalAssistant {
          vendor: "ollama"
          chat: "llama3"
        }
    "#;
    let program = ulx_syntax::parse_source(src).expect("must parse");
    let TopDecl::Provider(p) = &program.decls[0].0 else {
        panic!("expected a provider decl");
    };
    assert_eq!(name_span_text(src, &p.name_span), "LocalAssistant");
}
