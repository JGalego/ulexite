//! §24.12: `///` doc comments used to be discarded identically to `//` —
//! the grammar has a distinct `doc_comment` production (§8), but nothing in
//! the compiler ever recovered the text. These tests exercise the recovery
//! path end to end, through `parse_source`.

#[test]
fn doc_comment_preceding_a_decl_is_recovered_against_its_span_start() {
    let src = "/// Translates text into another language.\nconversation Translate(source: text) -> text {\n  source\n}\n";
    let program = ulx_syntax::parse_source(src).expect("must parse");
    let (_, span) = &program.decls[0];
    assert_eq!(
        program.doc_comments.get(&span.start).map(String::as_str),
        Some("Translates text into another language.")
    );
}

#[test]
fn consecutive_doc_comment_lines_are_joined_with_newlines() {
    let src = "/// Line one.\n/// Line two.\nconversation Foo() -> text {\n  \"x\"\n}\n";
    let program = ulx_syntax::parse_source(src).expect("must parse");
    let (_, span) = &program.decls[0];
    assert_eq!(
        program.doc_comments.get(&span.start).map(String::as_str),
        Some("Line one.\nLine two.")
    );
}

#[test]
fn a_plain_double_slash_comment_is_not_recovered_as_a_doc_comment() {
    let src =
        "// just a regular comment, not a doc comment\nconversation Foo() -> text {\n  \"x\"\n}\n";
    let program = ulx_syntax::parse_source(src).expect("must parse");
    assert!(
        program.doc_comments.is_empty(),
        "expected no doc comments, got: {:?}",
        program.doc_comments
    );
}

#[test]
fn a_decl_with_no_preceding_doc_comment_has_no_entry() {
    let src = "conversation Foo() -> text {\n  \"x\"\n}\n";
    let program = ulx_syntax::parse_source(src).expect("must parse");
    assert!(program.doc_comments.is_empty());
}
