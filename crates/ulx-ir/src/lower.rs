use std::collections::HashSet;

use ulx_ast::*;

use crate::types::*;

#[derive(Debug, Clone, PartialEq)]
pub enum LowerError {
    /// `system:`/`user:` message statements must be immediately followed by
    /// `assistant -> name` (§7.3) — anything else is a sequencing error at
    /// this v0.1 lowering stage.
    DanglingMessages {
        conversation: String,
    },
    UnsupportedAskBodyStatement {
        conversation: String,
    },
}

pub fn lower_program(program: &Program) -> Result<IrProgram, LowerError> {
    let known_conversations: HashSet<String> = program
        .decls
        .iter()
        .filter_map(|(d, _)| match d {
            TopDecl::Conversation(c) => Some(c.name.clone()),
            _ => None,
        })
        .collect();

    let mut out = IrProgram {
        conversations: Vec::new(),
        judges: Vec::new(),
        validators: Vec::new(),
        datasets: Vec::new(),
        benchmarks: Vec::new(),
    };

    for (decl, _) in &program.decls {
        match decl {
            TopDecl::Conversation(c) => {
                out.conversations
                    .push(lower_conversation(c, &known_conversations)?);
            }
            TopDecl::Judge(r) => out.judges.push(lower_rubric(r, &known_conversations)?),
            TopDecl::Validator(r) => out.validators.push(lower_rubric(r, &known_conversations)?),
            TopDecl::Dataset(d) => out.datasets.push(lower_dataset(d, &known_conversations)?),
            TopDecl::Type(_) => {}
            TopDecl::Benchmark(b) => out
                .benchmarks
                .push(lower_benchmark(b, &known_conversations)?),
        }
    }

    Ok(out)
}

fn lower_conversation(
    c: &ConversationDecl,
    known: &HashSet<String>,
) -> Result<IrConversation, LowerError> {
    Ok(IrConversation {
        name: c.name.clone(),
        params: c
            .params
            .iter()
            .map(|p| (p.name.clone(), p.ty.0.clone()))
            .collect(),
        ret: c.ret.as_ref().map(|(t, _)| t.clone()),
        body: lower_block(&c.body, &c.name, known)?,
    })
}

fn lower_rubric(r: &RubricDecl, known: &HashSet<String>) -> Result<IrRubric, LowerError> {
    Ok(IrRubric {
        name: r.name.clone(),
        params: r
            .params
            .iter()
            .map(|p| (p.name.clone(), p.ty.0.clone()))
            .collect(),
        ret: r.ret.0.clone(),
        fields: r
            .fields
            .iter()
            .map(|(k, v)| Ok::<_, LowerError>((k.clone(), lower_expr(&v.0, known)?)))
            .collect::<Result<_, _>>()?,
    })
}

fn lower_dataset(d: &DatasetDecl, known: &HashSet<String>) -> Result<IrDataset, LowerError> {
    Ok(IrDataset {
        name: d.name.clone(),
        ty: d.ty.0.clone(),
        source: match &d.source {
            DatasetSource::FromFile(path) => IrDatasetSource::FromFile(path.clone()),
            DatasetSource::Rows(rows) => IrDatasetSource::Rows(
                rows.iter()
                    .map(|row| {
                        row.iter()
                            .map(|(k, v)| {
                                Ok::<_, LowerError>((k.clone(), lower_expr(&v.0, known)?))
                            })
                            .collect::<Result<_, _>>()
                    })
                    .collect::<Result<_, _>>()?,
            ),
        },
    })
}

fn lower_benchmark(b: &BenchmarkDecl, known: &HashSet<String>) -> Result<IrBenchmark, LowerError> {
    let steps = b
        .stmts
        .iter()
        .map(|(s, _)| {
            Ok::<_, LowerError>(match s {
                BenchmarkStmt::Dataset(name) => IrBenchmarkStep::Dataset(name.clone()),
                BenchmarkStmt::Run { expr, bind } => IrBenchmarkStep::Run {
                    expr: lower_expr(&expr.0, known)?,
                    bind: bind.clone(),
                },
                BenchmarkStmt::Expect {
                    expr,
                    judge,
                    threshold,
                } => IrBenchmarkStep::Expect {
                    expr: lower_expr(&expr.0, known)?,
                    judge: lower_expr(&judge.0, known)?,
                    threshold: *threshold,
                },
                BenchmarkStmt::Assert(e) => IrBenchmarkStep::Assert(lower_expr(&e.0, known)?),
                BenchmarkStmt::Snapshot { expr, key } => IrBenchmarkStep::Snapshot {
                    expr: lower_expr(&expr.0, known)?,
                    key: lower_expr(&key.0, known)?,
                },
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(IrBenchmark {
        name: b.name.clone(),
        steps,
    })
}

/// Lowers a block, desugaring the `system:`/`user:` ... `assistant -> name`
/// message-literal pattern (§7.3) into an explicit `chat` effect.
fn lower_block(
    block: &Block,
    conv_name: &str,
    known: &HashSet<String>,
) -> Result<IrBlock, LowerError> {
    let mut insts = Vec::new();
    let mut pending: Vec<(MessageRole, IrExpr)> = Vec::new();

    for (stmt, _span) in &block.stmts {
        match stmt {
            Stmt::Message { role, text } => {
                pending.push((*role, lower_expr(&text.0, known)?));
            }
            Stmt::AssistantBind { name, .. } => {
                let messages = std::mem::take(&mut pending);
                insts.push(IrInst {
                    bind: Some(name.clone()),
                    expr: IrExpr::Effect(Box::new(IrEffect::Ask {
                        capability: "chat".to_string(),
                        args: Vec::new(),
                        messages,
                    })),
                });
            }
            other => {
                if !pending.is_empty() {
                    return Err(LowerError::DanglingMessages {
                        conversation: conv_name.to_string(),
                    });
                }
                insts.push(lower_stmt(other, conv_name, known)?);
            }
        }
    }
    if !pending.is_empty() {
        return Err(LowerError::DanglingMessages {
            conversation: conv_name.to_string(),
        });
    }

    let tail = match &block.tail {
        Some(t) => Some(Box::new(lower_expr(&t.0, known)?)),
        None => None,
    };

    Ok(IrBlock { insts, tail })
}

fn lower_stmt(stmt: &Stmt, conv_name: &str, known: &HashSet<String>) -> Result<IrInst, LowerError> {
    Ok(match stmt {
        Stmt::Message { .. } | Stmt::AssistantBind { .. } => unreachable!("handled in lower_block"),
        Stmt::With(bindings) => {
            let members = bindings
                .iter()
                .map(|b| Ok::<_, LowerError>((b.name.clone(), lower_expr(&b.value.0, known)?)))
                .collect::<Result<_, _>>()?;
            IrInst {
                bind: None,
                expr: IrExpr::Parallel(members),
            }
        }
        Stmt::Ask {
            capability,
            args,
            body,
            bind_name,
            ..
        } => IrInst {
            bind: Some(bind_name.clone()),
            expr: IrExpr::Effect(Box::new(IrEffect::Ask {
                capability: capability.clone(),
                args: lower_args(args, known)?,
                messages: lower_ask_body_messages(body, conv_name)?,
            })),
        },
        Stmt::Binding(b) => IrInst {
            bind: Some(b.name.clone()),
            expr: lower_expr(&b.value.0, known)?,
        },
        Stmt::Match(m) => IrInst {
            bind: None,
            expr: lower_match(m, known)?,
        },
        Stmt::For { var, iter, body } => IrInst {
            bind: None,
            expr: IrExpr::For {
                var: var.clone(),
                iter: Box::new(lower_expr(&iter.0, known)?),
                body: lower_block(body, conv_name, known)?,
            },
        },
        Stmt::While { cond, body } => IrInst {
            bind: None,
            expr: IrExpr::While {
                cond: Box::new(lower_expr(&cond.0, known)?),
                body: lower_block(body, conv_name, known)?,
            },
        },
        Stmt::Break(e) => IrInst {
            bind: None,
            expr: IrExpr::Break(match e {
                Some(e) => Some(Box::new(lower_expr(&e.0, known)?)),
                None => None,
            }),
        },
        Stmt::Expr(e) => IrInst {
            bind: None,
            expr: lower_expr(&e.0, known)?,
        },
    })
}

/// An `ask` body block (§7.5) is, in well-formed v0 programs, purely a
/// sequence of `system:`/`user:` message statements — no control flow. This
/// is a deliberate, documented restriction (see module docs); anything else
/// is rejected rather than silently dropped.
fn lower_ask_body_messages(
    body: &Block,
    conv_name: &str,
) -> Result<Vec<(MessageRole, IrExpr)>, LowerError> {
    let empty = HashSet::new();
    let mut messages = Vec::new();
    for (stmt, _) in &body.stmts {
        match stmt {
            Stmt::Message { role, text } => messages.push((*role, lower_expr(&text.0, &empty)?)),
            _ => {
                return Err(LowerError::UnsupportedAskBodyStatement {
                    conversation: conv_name.to_string(),
                })
            }
        }
    }
    if body.tail.is_some() {
        return Err(LowerError::UnsupportedAskBodyStatement {
            conversation: conv_name.to_string(),
        });
    }
    Ok(messages)
}

fn lower_match(m: &MatchStmt, known: &HashSet<String>) -> Result<IrExpr, LowerError> {
    let arms = m
        .arms
        .iter()
        .map(|arm| {
            Ok::<_, LowerError>(IrMatchArm {
                pattern: match &arm.pattern {
                    Pattern::Variant { name, bindings } => IrPattern::Variant {
                        name: name.clone(),
                        bindings: bindings.clone(),
                    },
                    Pattern::Wildcard => IrPattern::Wildcard,
                },
                body: match &arm.body {
                    MatchArmBody::Expr(e) => IrArmBody::Expr(lower_expr(&e.0, known)?),
                    MatchArmBody::Block(b) => {
                        IrArmBody::Block(lower_block(b, "<match arm>", known)?)
                    }
                },
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(IrExpr::Match {
        scrutinee: Box::new(lower_expr(&m.scrutinee.0, known)?),
        arms,
    })
}

fn lower_args(args: &[Arg], known: &HashSet<String>) -> Result<Vec<IrArg>, LowerError> {
    args.iter()
        .map(|a| {
            Ok::<_, LowerError>(IrArg {
                name: a.name.clone(),
                value: lower_expr(&a.value.0, known)?,
            })
        })
        .collect()
}

fn lower_expr(expr: &Expr, known: &HashSet<String>) -> Result<IrExpr, LowerError> {
    Ok(match expr {
        Expr::Int(i) => IrExpr::Int(*i),
        Expr::Float(f) => IrExpr::Float(*f),
        Expr::Str(s) => IrExpr::Str(s.clone()),
        Expr::TextBlock(parts) => IrExpr::TextBlock(
            parts
                .iter()
                .map(|p| {
                    Ok::<_, LowerError>(match p {
                        TextPart::Literal(s) => IrTextPart::Literal(s.clone()),
                        TextPart::Interp((e, _)) => IrTextPart::Interp(lower_expr(e, known)?),
                    })
                })
                .collect::<Result<_, _>>()?,
        ),
        Expr::Ident(name) => IrExpr::Var(name.clone()),
        Expr::RecordLit(fields) => IrExpr::Record(
            fields
                .iter()
                .map(|(k, (v, _))| Ok::<_, LowerError>((k.clone(), lower_expr(v, known)?)))
                .collect::<Result<_, _>>()?,
        ),
        Expr::If {
            cond,
            then_block,
            else_block,
        } => IrExpr::If {
            cond: Box::new(lower_expr(&cond.0, known)?),
            then_block: lower_block(then_block, "<if>", known)?,
            else_block: lower_block(else_block, "<else>", known)?,
        },
        Expr::GenericCall { name, ty_arg, args } => IrExpr::GenericCall {
            name: name.clone(),
            ty_arg: ty_arg.0.clone(),
            args: lower_args(args, known)?,
        },
        Expr::Retry {
            count,
            body,
            else_expr,
        } => IrExpr::Retry {
            count: *count,
            body: lower_block(body, "<retry>", known)?,
            else_expr: match else_expr {
                Some(e) => Some(Box::new(lower_expr(&e.0, known)?)),
                None => None,
            },
        },
        Expr::Escalate { target, args } => IrExpr::Effect(Box::new(IrEffect::Escalate {
            target: target.clone(),
            args: args
                .iter()
                .map(|(k, (v, _))| Ok::<_, LowerError>((k.clone(), lower_expr(v, known)?)))
                .collect::<Result<_, _>>()?,
        })),
        Expr::JudgeCall { name, args } => IrExpr::Effect(Box::new(IrEffect::Judge {
            name: name.clone(),
            args: lower_args(args, known)?,
        })),
        Expr::ValidatorCall { name, args } => IrExpr::Effect(Box::new(IrEffect::Validator {
            name: name.clone(),
            args: lower_args(args, known)?,
        })),
        Expr::AskExpr {
            capability,
            args,
            body,
        } => IrExpr::Effect(Box::new(IrEffect::Ask {
            capability: capability.clone(),
            args: lower_args(args, known)?,
            messages: lower_ask_body_messages(body, "<ask expr>")?,
        })),
        Expr::RowRef => IrExpr::RowRef,
        Expr::FieldAccess { base, field } => IrExpr::FieldAccess {
            base: Box::new(lower_expr(&base.0, known)?),
            field: field.clone(),
        },
        Expr::Call { callee, args } => {
            if let Expr::Ident(name) = &callee.0 {
                if known.contains(name) {
                    return Ok(IrExpr::Effect(Box::new(IrEffect::ConversationCall {
                        name: name.clone(),
                        args: lower_args(args, known)?,
                    })));
                }
            }
            IrExpr::OpaqueCall {
                callee: Box::new(lower_expr(&callee.0, known)?),
                args: lower_args(args, known)?,
            }
        }
        Expr::Index { base, index } => IrExpr::Index {
            base: Box::new(lower_expr(&base.0, known)?),
            index: Box::new(lower_expr(&index.0, known)?),
        },
        Expr::Unary { op, expr } => IrExpr::Unary {
            op: op.clone(),
            expr: Box::new(lower_expr(&expr.0, known)?),
        },
        Expr::Binary { op, lhs, rhs } => IrExpr::Binary {
            op: *op,
            lhs: Box::new(lower_expr(&lhs.0, known)?),
            rhs: Box::new(lower_expr(&rhs.0, known)?),
        },
    })
}
