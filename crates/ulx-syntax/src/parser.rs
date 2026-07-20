//! Parser for Ulexite (§13.2 — chumsky, per the compiler-architecture RFC),
//! implementing the grammar in `docs/spec/08-grammar.md` one production at a
//! time. Keywords are recognized contextually over plain `Ident` tokens
//! (see `lexer.rs`); `kw("match")` etc. below is how a grammar keyword is
//! matched.
//!
//! `chumsky::error::Simple<Token>` is inherently large (it carries a set of
//! expected tokens); clippy's `result_large_err` fires on nearly every
//! combinator below as a result. Boxing it would mean threading `Box<Simple<..>>`
//! through every parser signature in this file for no correctness benefit,
//! so it's silenced at the module level rather than piecemeal.
#![allow(clippy::result_large_err)]

use chumsky::prelude::*;
use ulx_ast::*;

use crate::lexer::{self, Token};

pub type Err = Simple<Token>;
type PResult<T> = Result<T, Vec<Err>>;

fn ident_p() -> impl Parser<Token, String, Error = Err> + Clone {
    filter_map(|span, t: Token| match t {
        Token::Ident(s) => Ok(s),
        other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
    })
}

/// Matches an `Ident` token whose text is exactly `word` — our stand-in for
/// a reserved keyword (see module docs).
fn kw(word: &'static str) -> impl Parser<Token, (), Error = Err> + Clone {
    filter(move |t: &Token| matches!(t, Token::Ident(s) if s == word)).ignored()
}

fn spanned<T>(
    p: impl Parser<Token, T, Error = Err> + Clone,
) -> impl Parser<Token, Spanned<T>, Error = Err> + Clone {
    p.map_with_span(|node, span| (node, span))
}

// ---------------------------------------------------------------------
// Types (§8 `type_expr` and friends)
// ---------------------------------------------------------------------

fn artifact_type_p() -> impl Parser<Token, ArtifactType, Error = Err> + Clone {
    ident_p().try_map(|s, span| {
        ArtifactType::from_keyword(&s)
            .ok_or_else(|| Simple::custom(span, format!("`{s}` is not an artifact type")))
    })
}

fn type_expr_p() -> impl Parser<Token, Spanned<TypeExpr>, Error = Err> + Clone {
    recursive(|type_expr| {
        let field_type = ident_p()
            .then_ignore(just(Token::Colon))
            .then(type_expr.clone());

        let record_type = spanned(
            field_type
                .clone()
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .delimited_by(just(Token::LBrace), just(Token::RBrace))
                .map(TypeExpr::Record),
        );

        let array_type = spanned(
            type_expr
                .clone()
                .delimited_by(just(Token::LBracket), just(Token::RBracket))
                .map(|inner| TypeExpr::Array(Box::new(inner))),
        );

        let generic_arg = type_expr
            .clone()
            .map(|t| GenericArg::Type(Box::new(t)))
            .or(filter_map(|span, t: Token| match t {
                Token::Int(i) => Ok(GenericArg::Const(i)),
                other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
            }));

        let generic_type = spanned(
            ident_p()
                .then(generic_arg.delimited_by(just(Token::Lt), just(Token::Gt)))
                .map(|(name, arg)| TypeExpr::Generic { name, arg }),
        );

        let variant = ident_p()
            .then(
                type_expr
                    .clone()
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .or_not(),
            )
            .map(|(name, payload)| Variant {
                name,
                payload: payload.map(Box::new),
            });

        let union_type = spanned(
            variant
                .clone()
                .separated_by(just(Token::Pipe))
                .at_least(2)
                .map(TypeExpr::Union),
        );

        let named_or_artifact = spanned(ident_p().map(|s| {
            ArtifactType::from_keyword(&s)
                .map(TypeExpr::Artifact)
                .unwrap_or(TypeExpr::Named(s))
        }));

        choice((
            record_type,
            array_type,
            generic_type,
            union_type,
            named_or_artifact,
        ))
    })
}

// silence "unused" lint for a helper kept for future callers (§9.2 diagnostics)
#[allow(dead_code)]
fn _use_artifact_type_p() -> impl Parser<Token, ArtifactType, Error = Err> + Clone {
    artifact_type_p()
}

// ---------------------------------------------------------------------
// Expressions, statements, blocks (mutually recursive, §8)
// ---------------------------------------------------------------------

fn arg_list_p(
    expr: impl Parser<Token, Spanned<Expr>, Error = Err> + Clone + 'static,
) -> impl Parser<Token, Vec<Arg>, Error = Err> + Clone {
    let arg = ident_p()
        .then_ignore(just(Token::Colon))
        .or_not()
        .then(expr)
        .map(|(name, value)| Arg { name, value });
    arg.separated_by(just(Token::Comma)).allow_trailing()
}

fn named_args_p(
    expr: impl Parser<Token, Spanned<Expr>, Error = Err> + Clone + 'static,
) -> impl Parser<Token, Vec<(String, Spanned<Expr>)>, Error = Err> + Clone {
    let named = ident_p().then_ignore(just(Token::Colon)).then(expr);
    named.separated_by(just(Token::Comma)).allow_trailing()
}

/// Builds the whole mutually-recursive expr/stmt/block parser family and
/// returns their three entry points.
#[allow(clippy::type_complexity)]
pub fn program_pieces() -> (
    impl Parser<Token, Spanned<Expr>, Error = Err> + Clone,
    impl Parser<Token, Spanned<Stmt>, Error = Err> + Clone,
    impl Parser<Token, Block, Error = Err> + Clone,
) {
    let mut expr = Recursive::declare();
    let mut stmt = Recursive::declare();
    let mut block = Recursive::declare();

    let type_expr = type_expr_p();

    // ---- text blocks: split raw content into literal/interpolation parts
    let text_block = filter_map(|span: std::ops::Range<usize>, t: Token| match t {
        Token::TextBlock(s) => Ok((s, span)),
        other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
    })
    .try_map(|(raw, span): (String, std::ops::Range<usize>), _| {
        split_text_block(&raw, span.start)
            .map(Expr::TextBlock)
            .map_err(|msg| Simple::custom(span, msg))
    });

    // ---- primary expressions
    let int_lit = filter_map(|span, t: Token| match t {
        Token::Int(i) => Ok(Expr::Int(i)),
        other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
    });
    let float_lit = filter_map(|span, t: Token| match t {
        Token::Float(f) => Ok(Expr::Float(f)),
        other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
    });
    let str_lit = filter_map(|span, t: Token| match t {
        Token::Str(s) => Ok(Expr::Str(s)),
        other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
    });
    let row_ref = just(Token::Dollar).to(Expr::RowRef);

    // ---- `file("path")` / bare `@path` shorthand (§8 `file_expr`) — both
    // produce the same node; a loaded prompt file's content is split into
    // literal/interpolation parts later, once its content is known (see
    // `ulx-sema`'s prompt-file resolution), the same way `text_block` above
    // is split eagerly here.
    let file_call = kw("file")
        .ignore_then(
            filter_map(|span, t: Token| match t {
                Token::Str(s) => Ok(s),
                other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
            })
            .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .map(|path| Expr::FileText {
            path,
            shorthand: false,
        });
    let at_path = filter_map(|span, t: Token| match t {
        Token::AtPath(path) => Ok(Expr::FileText {
            path,
            shorthand: true,
        }),
        other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
    });
    let file_text_expr = file_call.or(at_path);

    let field_assign = ident_p().then_ignore(just(Token::Colon)).then(expr.clone());
    let record_lit = field_assign
        .clone()
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .delimited_by(just(Token::LBrace), just(Token::RBrace))
        .map(Expr::RecordLit);

    let if_expr = kw("if")
        .ignore_then(expr.clone())
        .then(block.clone())
        .then_ignore(kw("else"))
        .then(block.clone())
        .map(|((cond, then_block), else_block)| Expr::If {
            cond: Box::new(cond),
            then_block,
            else_block,
        });

    let generic_call = ident_p()
        .then_ignore(just(Token::Lt))
        .then(type_expr.clone())
        .then_ignore(just(Token::Gt))
        .then(arg_list_p(expr.clone()).delimited_by(just(Token::LParen), just(Token::RParen)))
        .map(|((name, ty_arg), args)| Expr::GenericCall { name, ty_arg, args });

    let retry_expr = kw("retry")
        .ignore_then(
            filter_map(|span, t: Token| match t {
                Token::Int(i) => Ok(i as u64),
                other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
            })
            .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(block.clone())
        .then(kw("else").ignore_then(expr.clone()).or_not())
        .map(|((count, body), else_expr)| Expr::Retry {
            count,
            body,
            else_expr: else_expr.map(Box::new),
        });

    let escalate_expr = kw("escalate")
        .ignore_then(
            ident_p()
                .then(
                    just(Token::Comma)
                        .ignore_then(named_args_p(expr.clone()))
                        .or_not(),
                )
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .map(|(target, args)| Expr::Escalate {
            target,
            args: args.unwrap_or_default(),
        });

    let judge_call = kw("judge")
        .ignore_then(ident_p())
        .then(arg_list_p(expr.clone()).delimited_by(just(Token::LParen), just(Token::RParen)))
        .map(|(name, args)| Expr::JudgeCall { name, args });

    let validator_call = kw("validator")
        .ignore_then(ident_p())
        .then(arg_list_p(expr.clone()).delimited_by(just(Token::LParen), just(Token::RParen)))
        .map(|(name, args)| Expr::ValidatorCall { name, args });

    let ask_expr = kw("ask")
        .ignore_then(ident_p())
        .then(arg_list_p(expr.clone()).delimited_by(just(Token::LParen), just(Token::RParen)))
        .then(block.clone())
        .map(|((capability, args), body)| Expr::AskExpr {
            capability,
            args,
            body,
        });

    let paren = expr
        .clone()
        .delimited_by(just(Token::LParen), just(Token::RParen));

    let ident_expr = ident_p().map(Expr::Ident);

    let primary = spanned(choice((
        float_lit,
        int_lit,
        str_lit,
        text_block,
        file_text_expr.clone(),
        if_expr,
        generic_call,
        retry_expr,
        escalate_expr,
        judge_call,
        validator_call,
        ask_expr,
        row_ref,
        record_lit,
        ident_expr,
    )))
    .or(paren);

    // ---- postfix: field access, call, index
    #[derive(Clone)]
    enum Postfix {
        Field(String),
        Call(Vec<Arg>),
        Index(Spanned<Expr>),
    }
    let postfix_op = choice((
        just(Token::Dot).ignore_then(ident_p()).map(Postfix::Field),
        arg_list_p(expr.clone())
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(Postfix::Call),
        expr.clone()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(Postfix::Index),
    ));
    let postfix = primary.then(postfix_op.repeated()).foldl(|base, op| {
        let span = base.1.start..base.1.end;
        let node = match op {
            Postfix::Field(field) => Expr::FieldAccess {
                base: Box::new(base),
                field,
            },
            Postfix::Call(args) => Expr::Call {
                callee: Box::new(base),
                args,
            },
            Postfix::Index(index) => Expr::Index {
                base: Box::new(base),
                index: Box::new(index),
            },
        };
        (node, span)
    });

    // ---- unary
    let unary = choice((
        just(Token::Minus).to(UnaryOp::Neg),
        kw("not").to(UnaryOp::Not),
    ))
    .repeated()
    .then(postfix)
    .foldr(|op, e| {
        let span = e.1.clone();
        (
            Expr::Unary {
                op,
                expr: Box::new(e),
            },
            span,
        )
    });

    // ---- binary precedence chain
    macro_rules! left_assoc_bin {
        ($base:expr, $ops:expr) => {
            $base
                .clone()
                .then($ops.then($base).repeated())
                .foldl(|lhs, (op, rhs)| {
                    let span = lhs.1.start..rhs.1.end;
                    (
                        Expr::Binary {
                            op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        span,
                    )
                })
        };
    }

    let mul = left_assoc_bin!(
        unary,
        choice((
            just(Token::Star).to(BinaryOp::Mul),
            just(Token::Slash).to(BinaryOp::Div),
        ))
    );
    let add = left_assoc_bin!(
        mul,
        choice((
            just(Token::Plus).to(BinaryOp::Add),
            just(Token::Minus).to(BinaryOp::Sub),
        ))
    );
    let cmp = left_assoc_bin!(
        add,
        choice((
            just(Token::EqEq).to(BinaryOp::Eq),
            just(Token::Ne).to(BinaryOp::Ne),
            just(Token::Le).to(BinaryOp::Le),
            just(Token::Ge).to(BinaryOp::Ge),
            just(Token::Lt).to(BinaryOp::Lt),
            just(Token::Gt).to(BinaryOp::Gt),
        ))
    );
    let and_expr = left_assoc_bin!(cmp, kw("and").to(BinaryOp::And));
    let or_expr = left_assoc_bin!(and_expr, kw("or").to(BinaryOp::Or));

    expr.define(or_expr);

    // ---- statements
    let role = choice((
        kw("system").to(MessageRole::System),
        kw("user").to(MessageRole::User),
    ));
    let message_stmt = role
        .then_ignore(just(Token::Colon))
        .then(spanned(text_block.or(file_text_expr.clone())))
        .map(|(role, text)| Stmt::Message { role, text });

    let assistant_bind = kw("assistant")
        .ignore_then(just(Token::Arrow))
        .ignore_then(ident_p())
        .then(just(Token::Colon).ignore_then(type_expr.clone()).or_not())
        .map(|(name, ty)| Stmt::AssistantBind { name, ty });

    let binding = ident_p()
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .map(|(name, value)| Binding { name, value });

    let with_block = kw("with")
        .ignore_then(
            binding
                .clone()
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(Stmt::With);

    let ask_stmt = ask_expr_stmt_head(expr.clone(), block.clone())
        .then_ignore(just(Token::Arrow))
        .then(ident_p())
        .then(just(Token::Colon).ignore_then(type_expr.clone()).or_not())
        .map(
            |(((capability, args, body), bind_name), bind_ty)| Stmt::Ask {
                capability,
                args,
                body,
                bind_name,
                bind_ty,
            },
        );

    let pattern = choice((
        just(Token::Ident("_".to_string())).to(Pattern::Wildcard),
        ident_p()
            .then(
                ident_p()
                    .separated_by(just(Token::Comma))
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .or_not(),
            )
            .map(|(name, bindings)| Pattern::Variant {
                name,
                bindings: bindings.unwrap_or_default(),
            }),
    ));
    let match_arm_body = block
        .clone()
        .map(MatchArmBody::Block)
        .or(expr.clone().map(MatchArmBody::Expr));
    let match_arm = pattern
        .then_ignore(just(Token::FatArrow))
        .then(match_arm_body)
        .map(|(pattern, body)| MatchArm { pattern, body });
    let match_stmt = kw("match")
        .ignore_then(expr.clone())
        .then(
            match_arm
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(scrutinee, arms)| {
            Stmt::Match(MatchStmt {
                scrutinee: Box::new(scrutinee),
                arms,
            })
        });

    let for_stmt = kw("for")
        .ignore_then(ident_p())
        .then_ignore(kw("in"))
        .then(expr.clone())
        .then(block.clone())
        .map(|((var, iter), body)| Stmt::For { var, iter, body });

    let while_stmt = kw("while")
        .ignore_then(expr.clone())
        .then(block.clone())
        .map(|(cond, body)| Stmt::While { cond, body });

    let break_stmt = kw("break")
        .ignore_then(expr.clone().or_not())
        .map(Stmt::Break);

    let expr_stmt = expr.clone().map(Stmt::Expr);

    let stmt_body = choice((
        message_stmt,
        with_block,
        assistant_bind,
        ask_stmt,
        match_stmt,
        for_stmt,
        while_stmt,
        break_stmt,
        binding.map(Stmt::Binding),
        expr_stmt,
    ));
    stmt.define(spanned(stmt_body));

    let block_body = stmt
        .clone()
        .repeated()
        .delimited_by(just(Token::LBrace), just(Token::RBrace))
        .map(finish_block);
    block.define(block_body);

    (expr, stmt, block)
}

/// Splits a flat `Vec<Spanned<Stmt>>` into `(stmts, tail)`: a trailing
/// bare-expression statement becomes the block's value (§8 `block`).
fn finish_block(mut stmts: Vec<Spanned<Stmt>>) -> Block {
    let tail = match stmts.last() {
        Some((Stmt::Expr(_), _)) => {
            let (last, _span) = stmts.pop().unwrap();
            match last {
                Stmt::Expr(e) => Some(Box::new(e)),
                _ => unreachable!(),
            }
        }
        _ => None,
    };
    Block { stmts, tail }
}

/// Shared head of `ask_stmt`/`ask_expr`: `"ask" ident "(" args ")" block`,
/// returned as a flat tuple since chumsky's fold combinators want `Clone`
/// closures rather than the `Expr::AskExpr` variant directly.
fn ask_expr_stmt_head(
    expr: impl Parser<Token, Spanned<Expr>, Error = Err> + Clone + 'static,
    block: impl Parser<Token, Block, Error = Err> + Clone + 'static,
) -> impl Parser<Token, (String, Vec<Arg>, Block), Error = Err> + Clone {
    kw("ask")
        .ignore_then(ident_p())
        .then(arg_list_p(expr).delimited_by(just(Token::LParen), just(Token::RParen)))
        .then(block)
        .map(|((capability, args), body)| (capability, args, body))
}

/// Splits triple-quoted text-block content into literal/interpolation
/// parts, re-lexing and re-parsing each `{expr}` span as an ordinary
/// expression (§7.1). Interpolations are not allowed to nest braces.
///
/// Also used (with `base_offset = 0`) by `ulx-sema` to split the content of
/// a file loaded via `file("...")`/`@path` (§8 `file_expr`) — an externally
/// loaded prompt's `{var}` interpolations are checked with exactly the same
/// pass as an inline `"""..."""` block.
pub fn split_text_block(raw: &str, base_offset: usize) -> Result<Vec<TextPart>, String> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = raw.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '{' {
            if !literal.is_empty() {
                parts.push(TextPart::Literal(std::mem::take(&mut literal)));
            }
            let start = i + 1;
            let mut end = None;
            for (j, cc) in chars.by_ref() {
                if cc == '}' {
                    end = Some(j);
                    break;
                }
            }
            let end = end.ok_or_else(|| "unterminated interpolation `{`".to_string())?;
            let sub = &raw[start..end];
            let tokens = lexer::lex(sub).map_err(|span| {
                format!("{} at offset {}", lex_error_message(sub, &span), span.start)
            })?;
            if let Some(span) = find_excess_nesting(&tokens) {
                return Err(format!(
                    "{} at offset {}",
                    nesting_error_message(),
                    span.start
                ));
            }
            let shifted: Vec<_> = tokens
                .into_iter()
                .map(|(t, span)| {
                    (
                        t,
                        (span.start + base_offset + start)..(span.end + base_offset + start),
                    )
                })
                .collect();
            let eoi = base_offset + start + sub.len()..base_offset + start + sub.len();
            let stream = chumsky::Stream::from_iter(eoi, shifted.into_iter());
            let (expr, stmt, _block) = program_pieces();
            let _ = &stmt; // only `expr` is needed for an interpolation
            let parsed = expr
                .then_ignore(end_no_input())
                .parse(stream)
                .map_err(|e| format!("{e:?}"))?;
            parts.push(TextPart::Interp(parsed));
        } else {
            literal.push(c);
        }
    }
    if !literal.is_empty() {
        parts.push(TextPart::Literal(literal));
    }
    Ok(parts)
}

fn end_no_input() -> impl Parser<Token, (), Error = Err> + Clone {
    end()
}

// ---------------------------------------------------------------------
// Top-level declarations (§8 `top_decl`, `import_decl`)
// ---------------------------------------------------------------------

fn param_list_p(
    type_expr: impl Parser<Token, Spanned<TypeExpr>, Error = Err> + Clone,
) -> impl Parser<Token, Vec<Param>, Error = Err> + Clone {
    spanned(ident_p())
        .then_ignore(just(Token::Colon))
        .then(type_expr)
        .map(|((name, name_span), ty)| Param {
            name,
            name_span,
            ty,
        })
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .delimited_by(just(Token::LParen), just(Token::RParen))
}

fn import_p() -> impl Parser<Token, Spanned<Import>, Error = Err> + Clone {
    let kind = choice((
        kw("conversation").to(ImportKind::Conversation),
        kw("judge").to(ImportKind::Judge),
        kw("validator").to(ImportKind::Validator),
        kw("dataset").to(ImportKind::Dataset),
        kw("type").to(ImportKind::Type),
        kw("provider").to(ImportKind::Provider),
    ));
    let str_lit = filter_map(|span, t: Token| match t {
        Token::Str(s) => Ok(s),
        other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
    });
    let named = kind
        .then(ident_p())
        .then_ignore(kw("from"))
        .then(str_lit)
        .map(|((kind, name), from)| Import::Named { kind, name, from });
    let module = str_lit
        .then_ignore(kw("as"))
        .then(ident_p())
        .map(|(path, alias)| Import::Module { path, alias });
    kw("import")
        .ignore_then(choice((named, module)))
        .map_with_span(|node, span| (node, span))
}

pub fn program_p() -> impl Parser<Token, Program, Error = Err> + Clone {
    let (expr, _stmt, block) = program_pieces();
    let type_expr = type_expr_p();

    let str_lit = filter_map(|span, t: Token| match t {
        Token::Str(s) => Ok(s),
        other => Err(Simple::expected_input_found(span, Vec::new(), Some(other))),
    });

    let field_assign = ident_p().then_ignore(just(Token::Colon)).then(expr.clone());

    let rubric_decl = |head: &'static str| {
        kw(head)
            .ignore_then(spanned(ident_p()))
            .then(param_list_p(type_expr.clone()))
            .then_ignore(just(Token::Arrow))
            .then(type_expr.clone())
            .then(
                field_assign
                    .clone()
                    .repeated()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map(|((((name, name_span), params), ret), fields)| RubricDecl {
                doc: None,
                name,
                name_span,
                params,
                ret,
                fields,
            })
    };

    let judge_decl = rubric_decl("judge").map(TopDecl::Judge);
    let validator_decl = rubric_decl("validator").map(TopDecl::Validator);

    let dataset_rows = {
        let record_lit = field_assign
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .delimited_by(just(Token::LBrace), just(Token::RBrace));
        record_lit
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
    };
    let dataset_source = kw("from")
        .ignore_then(str_lit)
        .map(DatasetSource::FromFile)
        .or(dataset_rows.map(DatasetSource::Rows));
    let dataset_decl = kw("dataset")
        .ignore_then(spanned(ident_p()))
        .then_ignore(just(Token::Colon))
        .then(type_expr.clone())
        .then(dataset_source.delimited_by(just(Token::LBrace), just(Token::RBrace)))
        .map(|(((name, name_span), ty), source)| {
            TopDecl::Dataset(DatasetDecl {
                doc: None,
                name,
                name_span,
                ty,
                source,
            })
        });

    let type_decl = kw("type")
        .ignore_then(spanned(ident_p()))
        .then_ignore(just(Token::Eq))
        .then(type_expr.clone())
        .map(|((name, name_span), ty)| {
            TopDecl::Type(TypeDecl {
                name,
                name_span,
                ty,
            })
        });

    let benchmark_stmt = choice((
        kw("dataset")
            .ignore_then(just(Token::Colon))
            .ignore_then(ident_p())
            .map(BenchmarkStmt::Dataset),
        kw("run")
            .ignore_then(just(Token::Colon))
            .ignore_then(expr.clone())
            .then_ignore(just(Token::Arrow))
            .then(ident_p())
            .map(|(expr, bind)| BenchmarkStmt::Run { expr, bind }),
        kw("expect")
            .ignore_then(expr.clone())
            .then_ignore(kw("satisfies"))
            .then(expr.clone())
            .then(
                kw("with")
                    .ignore_then(kw("threshold"))
                    .ignore_then(
                        filter_map(|span, t: Token| match t {
                            Token::Float(f) => Ok(f),
                            Token::Int(i) => Ok(i as f64),
                            other => {
                                Err(Simple::expected_input_found(span, Vec::new(), Some(other)))
                            }
                        })
                        .delimited_by(just(Token::LParen), just(Token::RParen)),
                    )
                    .or_not(),
            )
            .map(|((expr, judge), threshold)| BenchmarkStmt::Expect {
                expr,
                judge,
                threshold,
            }),
        kw("assert")
            .ignore_then(expr.clone())
            .map(BenchmarkStmt::Assert),
        kw("snapshot")
            .ignore_then(expr.clone())
            .then_ignore(kw("as"))
            .then(expr.clone())
            .map(|(expr, key)| BenchmarkStmt::Snapshot { expr, key }),
    ));
    let benchmark_decl = kw("benchmark")
        .ignore_then(spanned(ident_p()))
        .then(
            spanned(benchmark_stmt)
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|((name, name_span), stmts)| {
            TopDecl::Benchmark(BenchmarkDecl {
                doc: None,
                name,
                name_span,
                stmts,
            })
        });

    let provider_decl = kw("provider")
        .ignore_then(spanned(ident_p()))
        .then(kw("from").ignore_then(str_lit).or_not())
        .then(
            field_assign
                .clone()
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(((name, name_span), from), fields)| {
            TopDecl::Provider(ProviderDecl {
                doc: None,
                name,
                name_span,
                from,
                fields,
            })
        });

    let conversation_decl = kw("conversation")
        .ignore_then(spanned(ident_p()))
        .then(param_list_p(type_expr.clone()))
        .then(just(Token::Arrow).ignore_then(type_expr.clone()).or_not())
        .then(block)
        .map(|((((name, name_span), params), ret), body)| {
            TopDecl::Conversation(ConversationDecl {
                doc: None,
                name,
                name_span,
                params,
                ret,
                body,
            })
        });

    let top_decl = spanned(choice((
        conversation_decl,
        judge_decl,
        validator_decl,
        dataset_decl,
        type_decl,
        benchmark_decl,
        provider_decl,
    )));

    let import_or_decl = import_p().map(Item::Import).or(top_decl.map(Item::Decl));

    import_or_decl.repeated().then_ignore(end()).map(|items| {
        let mut program = Program {
            imports: Vec::new(),
            decls: Vec::new(),
        };
        for item in items {
            match item {
                Item::Import(i) => program.imports.push(i),
                Item::Decl(d) => program.decls.push(d),
            }
        }
        program
    })
}

enum Item {
    Import(Spanned<Import>),
    Decl(Spanned<TopDecl>),
}

const MAX_NESTING_DEPTH: usize = 1_000;

/// Scans a token stream for `(`/`{`/`[` nesting deeper than
/// `MAX_NESTING_DEPTH`, returning the span of the offending delimiter if
/// found. Depth tracks all three delimiter kinds together (rather than each
/// separately) since any one of them recurses through the same combinators
/// (`paren`, `block`, array types, ...) and can exhaust the stack.
fn find_excess_nesting(
    tokens: &[(Token, std::ops::Range<usize>)],
) -> Option<std::ops::Range<usize>> {
    let mut depth: usize = 0;
    for (tok, span) in tokens {
        match tok {
            Token::LParen | Token::LBrace | Token::LBracket => {
                depth += 1;
                if depth > MAX_NESTING_DEPTH {
                    return Some(span.clone());
                }
            }
            Token::RParen | Token::RBrace | Token::RBracket => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    None
}

/// Lex + parse a complete `.ulx` source file into a `Program`.
pub fn parse_source(src: &str) -> PResult<Program> {
    let tokens = lexer::lex(src)
        .map_err(|span| vec![Simple::custom(span.clone(), lex_error_message(src, &span))])?;
    if let Some(span) = find_excess_nesting(&tokens) {
        return Err(vec![Simple::custom(span, nesting_error_message())]);
    }
    let eoi = src.len()..src.len();
    let stream = chumsky::Stream::from_iter(eoi, tokens.into_iter());
    program_p().parse(stream)
}

/// §24.12: chumsky's recursive-descent parser recurses on the native call
/// stack (via `stacker`'s growable-stack allocator transitively), which can
/// exhaust the process's address space on pathological input — empirically,
/// tens of thousands of nested delimiters (`(`/`{`/`[`) reliably crash the
/// process with an `mmap`/`mprotect` failure well before any real `.ulx`
/// program would ever nest this deep. Rather than depend on exactly where
/// that crashes (which varies by platform/available memory), reject
/// excessive nesting up front with a normal parse error — `MAX_NESTING_DEPTH`
/// is a couple of orders of magnitude above any legitimate program and a
/// couple of orders of magnitude below the observed crash point.
fn nesting_error_message() -> String {
    format!(
        "nesting depth exceeds the {MAX_NESTING_DEPTH}-level limit — this looks like malformed \
         or adversarial input rather than an intentionally deep program"
    )
}

/// A lexer error's span always covers the full text of the token that
/// failed to lex (see `lexer::lex`'s doc comment). An all-ASCII-digit span
/// means `Token::Int`'s `parse::<i64>()` callback rejected it — the only
/// way that regex matches but the callback fails — so it gets a specific
/// "too large to fit" message instead of the generic "unrecognized
/// character" one (§24.12).
fn lex_error_message(src: &str, span: &std::ops::Range<usize>) -> String {
    let text = &src[span.clone()];
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        format!("integer literal `{text}` is too large to fit in a 64-bit integer")
    } else {
        "unrecognized character".to_string()
    }
}

/// Renders a parse error as one human-readable line — "found X but
/// expected one of: A, B" or "unexpected X" when there's nothing expected
/// worth listing. `Err`'s `Simple<Token>` carries enough structure for
/// `ariadne`'s span-aware reports (`ulx-cli`'s `ulx parse`/`ulx check`),
/// but every consumer that just needs a plain `String` — `ulx-lsp`'s
/// diagnostics, `ulx-wasm`'s browser playground — was independently
/// re-deriving this exact message, so it lives here once instead.
pub fn format_error(e: &Err) -> String {
    if let chumsky::error::SimpleReason::Custom(msg) = e.reason() {
        // A `Simple::custom(...)` error (lexer failures, artifact-type
        // errors, §24.12) carries its message here, not in `expected()`/
        // `found()` — those are empty/`None` for a custom error, so
        // falling through to the generic rendering below would silently
        // discard the real message.
        return msg.clone();
    }
    let expected: Vec<String> = e
        .expected()
        .map(|tok| match tok {
            Some(t) => format!("{t}"),
            None => "end of input".to_string(),
        })
        .collect();
    let found = e
        .found()
        .map(|t| format!("{t}"))
        .unwrap_or_else(|| "end of input".to_string());
    if expected.is_empty() {
        format!("unexpected {found}")
    } else {
        format!("found {found} but expected one of: {}", expected.join(", "))
    }
}
