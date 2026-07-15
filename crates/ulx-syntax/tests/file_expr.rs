//! `file("...")`/`@path` (§8 `file_expr`) — a text block loaded from disk
//! instead of an inline `"""..."""`. These tests check the parser produces
//! the right AST shape and that `ulx fmt` preserves whichever concrete
//! syntax was written.

use ulx_ast::{Expr, MessageRole, Stmt, TopDecl};

fn parse(src: &str) -> ulx_ast::Program {
    ulx_syntax::parse_source(src).unwrap_or_else(|e| panic!("failed to parse {src:?}: {e:#?}"))
}

fn only_conversation_body(program: &ulx_ast::Program) -> &ulx_ast::Block {
    match &program.decls[0].0 {
        TopDecl::Conversation(c) => &c.body,
        other => panic!("expected a conversation decl, got {other:?}"),
    }
}

#[test]
fn file_call_form_parses_as_file_text_in_a_message() {
    let program = parse(
        r#"
        conversation Greet(name: text) -> text {
          system: file("prompts/system.txt")
          user: """Hi {name}"""
          assistant -> reply: text
          reply
        }
        "#,
    );
    let body = only_conversation_body(&program);
    let Stmt::Message { role, text } = &body.stmts[0].0 else {
        panic!("expected a message statement");
    };
    assert_eq!(*role, MessageRole::System);
    assert_eq!(
        text.0,
        Expr::FileText {
            path: "prompts/system.txt".to_string(),
            shorthand: false,
        }
    );
}

#[test]
fn at_path_shorthand_parses_as_file_text_in_a_message() {
    let program = parse(
        r#"
        conversation Greet(name: text) -> text {
          system: @prompts/system.txt
          user: """Hi {name}"""
          assistant -> reply: text
          reply
        }
        "#,
    );
    let body = only_conversation_body(&program);
    let Stmt::Message { role, text } = &body.stmts[0].0 else {
        panic!("expected a message statement");
    };
    assert_eq!(*role, MessageRole::System);
    assert_eq!(
        text.0,
        Expr::FileText {
            path: "prompts/system.txt".to_string(),
            shorthand: true,
        }
    );
}

#[test]
fn file_expr_is_usable_as_a_general_expression_in_a_with_binding() {
    let program = parse(
        r#"
        conversation Greet(name: text) -> text {
          with {
            sys = file("prompts/system.txt")
          }
          system: """{sys}"""
          user: """Hi {name}"""
          assistant -> reply: text
          reply
        }
        "#,
    );
    let body = only_conversation_body(&program);
    let Stmt::With(bindings) = &body.stmts[0].0 else {
        panic!("expected a with block");
    };
    assert_eq!(bindings[0].name, "sys");
    assert_eq!(
        bindings[0].value.0,
        Expr::FileText {
            path: "prompts/system.txt".to_string(),
            shorthand: false,
        }
    );
}

#[test]
fn fmt_preserves_the_file_call_form() {
    let program = parse(
        r#"
        conversation Greet(name: text) -> text {
          system: file("prompts/system.txt")
          user: """Hi {name}"""
          assistant -> reply: text
          reply
        }
        "#,
    );
    let formatted = ulx_syntax::format_program(&program);
    assert!(
        formatted.contains(r#"file("prompts/system.txt")"#),
        "expected canonical `file(...)` form preserved, got:\n{formatted}"
    );
}

#[test]
fn fmt_preserves_the_at_path_shorthand() {
    let program = parse(
        r#"
        conversation Greet(name: text) -> text {
          system: @prompts/system.txt
          user: """Hi {name}"""
          assistant -> reply: text
          reply
        }
        "#,
    );
    let formatted = ulx_syntax::format_program(&program);
    assert!(
        formatted.contains("@prompts/system.txt"),
        "expected bare `@path` shorthand preserved, got:\n{formatted}"
    );
}
