use ulx_ir::*;

fn lower(src: &str) -> IrProgram {
    let program = ulx_syntax::parse_source(src).expect("must parse");
    lower_program(&program).expect("must lower")
}

#[test]
fn message_sugar_desugars_to_chat_effect() {
    let ir = lower(
        r#"
        conversation Translate(source: text) -> text {
          system: """You are a translator."""
          user: """Translate: {source}"""
          assistant -> draft: text
          draft
        }
        "#,
    );
    let conv = &ir.conversations[0];
    assert_eq!(conv.body.insts.len(), 1);
    let inst = &conv.body.insts[0];
    assert_eq!(inst.bind.as_deref(), Some("draft"));
    match &inst.expr {
        IrExpr::Effect(e) => match e.as_ref() {
            IrEffect::Ask {
                capability,
                messages,
                ..
            } => {
                assert_eq!(capability, "chat");
                assert_eq!(messages.len(), 2);
            }
            other => panic!("expected Ask effect, got {other:?}"),
        },
        other => panic!("expected Effect, got {other:?}"),
    }
}

#[test]
fn conversation_call_is_recognized_as_effect() {
    let ir = lower(
        r#"
        conversation Helper() -> text {
          assistant -> x: text
          x
        }
        conversation Main() -> text {
          y = Helper()
          y
        }
        "#,
    );
    let main = ir.conversations.iter().find(|c| c.name == "Main").unwrap();
    let inst = &main.body.insts[0];
    match &inst.expr {
        IrExpr::Effect(e) => assert!(
            matches!(e.as_ref(), IrEffect::ConversationCall { name, .. } if name == "Helper")
        ),
        other => panic!("expected a ConversationCall effect, got {other:?}"),
    }
}

#[test]
fn dead_with_binding_is_eliminated() {
    let program = ulx_syntax::parse_source(
        r#"
        conversation Summarize(doc: pdf) -> text {
          with {
            outline  = ask vision(doc) { user: """Extract an outline.""" }
            keyfacts = ask vision(doc) { user: """List facts.""" }
          }
          outline
        }
        "#,
    )
    .unwrap();
    let mut ir = lower_program(&program).unwrap();
    let conv = &mut ir.conversations[0];
    eliminate_dead_bindings(&mut conv.body);

    let with_inst = conv
        .body
        .insts
        .iter()
        .find(|i| matches!(i.expr, IrExpr::Parallel(_)))
        .expect("with-block instruction should survive (outline is used)");
    match &with_inst.expr {
        IrExpr::Parallel(members) => {
            assert_eq!(
                members.len(),
                1,
                "keyfacts should have been eliminated: {members:?}"
            );
            assert_eq!(members[0].0, "outline");
        }
        _ => unreachable!(),
    }
}

#[test]
fn all_examples_lower_successfully() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("ulx") {
            continue;
        }
        // `file("...")`/`@path` (§8 `file_expr`) is deliberately *not*
        // resolvable by a bare parse -> `lower_program`: it requires
        // `ulx-sema`'s resolve-and-rewrite pass first (real base_dir +
        // typechecked interpolations), which `ulx-ir` has no dependency on
        // by design (§13's layering). `ulx-sema`/`ulx-cli` test this
        // example through the real pipeline instead.
        if path.file_name().and_then(|n| n.to_str()) == Some("prompt_from_file.ulx") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let program =
            ulx_syntax::parse_source(&src).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
        lower_program(&program).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
    }
}
