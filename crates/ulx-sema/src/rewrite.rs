//! Replaces every `file("...")`/`@path` node (§8 `file_expr`) with a plain
//! `Expr::TextBlock` once its content has been loaded and validated during
//! typechecking (`typecheck::load_file_text`'s cache). Runs once per module,
//! after typecheck succeeds, so that `ulx-ir`/`ulx-runtime` never see
//! `Expr::FileText` at all — they only ever get ordinary text blocks, the
//! same as an inline `"""..."""`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ulx_ast::*;

type PromptCache = HashMap<PathBuf, Result<Vec<TextPart>, String>>;

pub(crate) fn rewrite_program(program: &mut Program, cache: &PromptCache, base_dir: Option<&Path>) {
    for (decl, _) in &mut program.decls {
        rewrite_decl(decl, cache, base_dir);
    }
}

fn rewrite_decl(decl: &mut TopDecl, cache: &PromptCache, base_dir: Option<&Path>) {
    match decl {
        TopDecl::Conversation(c) => rewrite_block(&mut c.body, cache, base_dir),
        TopDecl::Judge(r) | TopDecl::Validator(r) => {
            for (_, e) in &mut r.fields {
                rewrite_expr(&mut e.0, cache, base_dir);
            }
        }
        TopDecl::Dataset(d) => {
            if let DatasetSource::Rows(rows) = &mut d.source {
                for row in rows {
                    for (_, e) in row {
                        rewrite_expr(&mut e.0, cache, base_dir);
                    }
                }
            }
        }
        TopDecl::Type(_) => {}
        TopDecl::Benchmark(b) => {
            for (stmt, _) in &mut b.stmts {
                match stmt {
                    BenchmarkStmt::Dataset(_) => {}
                    BenchmarkStmt::Run { expr, .. } => rewrite_expr(&mut expr.0, cache, base_dir),
                    BenchmarkStmt::Expect { expr, judge, .. } => {
                        rewrite_expr(&mut expr.0, cache, base_dir);
                        rewrite_expr(&mut judge.0, cache, base_dir);
                    }
                    BenchmarkStmt::Assert(e) => rewrite_expr(&mut e.0, cache, base_dir),
                    BenchmarkStmt::Snapshot { expr, key } => {
                        rewrite_expr(&mut expr.0, cache, base_dir);
                        rewrite_expr(&mut key.0, cache, base_dir);
                    }
                }
            }
        }
        TopDecl::Provider(p) => {
            for (_, e) in &mut p.fields {
                rewrite_expr(&mut e.0, cache, base_dir);
            }
        }
    }
}

fn rewrite_block(block: &mut Block, cache: &PromptCache, base_dir: Option<&Path>) {
    for (stmt, _) in &mut block.stmts {
        rewrite_stmt(stmt, cache, base_dir);
    }
    if let Some(tail) = &mut block.tail {
        rewrite_expr(&mut tail.0, cache, base_dir);
    }
}

fn rewrite_stmt(stmt: &mut Stmt, cache: &PromptCache, base_dir: Option<&Path>) {
    match stmt {
        Stmt::Message { text, .. } => rewrite_expr(&mut text.0, cache, base_dir),
        Stmt::AssistantBind { .. } => {}
        Stmt::With(bindings) => {
            for b in bindings {
                rewrite_expr(&mut b.value.0, cache, base_dir);
            }
        }
        Stmt::Ask { args, body, .. } => {
            for a in args {
                rewrite_expr(&mut a.value.0, cache, base_dir);
            }
            rewrite_block(body, cache, base_dir);
        }
        Stmt::Binding(b) => rewrite_expr(&mut b.value.0, cache, base_dir),
        Stmt::Match(m) => {
            rewrite_expr(&mut m.scrutinee.0, cache, base_dir);
            for arm in &mut m.arms {
                match &mut arm.body {
                    MatchArmBody::Expr(e) => rewrite_expr(&mut e.0, cache, base_dir),
                    MatchArmBody::Block(b) => rewrite_block(b, cache, base_dir),
                }
            }
        }
        Stmt::For { iter, body, .. } => {
            rewrite_expr(&mut iter.0, cache, base_dir);
            rewrite_block(body, cache, base_dir);
        }
        Stmt::While { cond, body } => {
            rewrite_expr(&mut cond.0, cache, base_dir);
            rewrite_block(body, cache, base_dir);
        }
        Stmt::Break(e) => {
            if let Some(e) = e {
                rewrite_expr(&mut e.0, cache, base_dir);
            }
        }
        Stmt::Expr(e) => rewrite_expr(&mut e.0, cache, base_dir),
    }
}

fn rewrite_expr(expr: &mut Expr, cache: &PromptCache, base_dir: Option<&Path>) {
    match expr {
        Expr::FileText { path, .. } => {
            let path = path.clone();
            let parts = base_dir
                .and_then(|d| cache.get(&d.join(&path)))
                .and_then(|r| r.as_ref().ok())
                .cloned()
                .unwrap_or_default();
            *expr = Expr::TextBlock(parts);
        }
        Expr::TextBlock(parts) => {
            for p in parts {
                if let TextPart::Interp(e) = p {
                    rewrite_expr(&mut e.0, cache, base_dir);
                }
            }
        }
        Expr::FieldAccess { base, .. } => rewrite_expr(&mut base.0, cache, base_dir),
        Expr::Call { callee, args } => {
            rewrite_expr(&mut callee.0, cache, base_dir);
            for a in args {
                rewrite_expr(&mut a.value.0, cache, base_dir);
            }
        }
        Expr::Index { base, index } => {
            rewrite_expr(&mut base.0, cache, base_dir);
            rewrite_expr(&mut index.0, cache, base_dir);
        }
        Expr::Unary { expr, .. } => rewrite_expr(&mut expr.0, cache, base_dir),
        Expr::Binary { lhs, rhs, .. } => {
            rewrite_expr(&mut lhs.0, cache, base_dir);
            rewrite_expr(&mut rhs.0, cache, base_dir);
        }
        Expr::If {
            cond,
            then_block,
            else_block,
        } => {
            rewrite_expr(&mut cond.0, cache, base_dir);
            rewrite_block(then_block, cache, base_dir);
            rewrite_block(else_block, cache, base_dir);
        }
        Expr::GenericCall { args, .. } => {
            for a in args {
                rewrite_expr(&mut a.value.0, cache, base_dir);
            }
        }
        Expr::Retry {
            body, else_expr, ..
        } => {
            rewrite_block(body, cache, base_dir);
            if let Some(e) = else_expr {
                rewrite_expr(&mut e.0, cache, base_dir);
            }
        }
        Expr::Escalate { args, .. } => {
            for (_, e) in args {
                rewrite_expr(&mut e.0, cache, base_dir);
            }
        }
        Expr::JudgeCall { args, .. } | Expr::ValidatorCall { args, .. } => {
            for a in args {
                rewrite_expr(&mut a.value.0, cache, base_dir);
            }
        }
        Expr::AskExpr { args, body, .. } => {
            for a in args {
                rewrite_expr(&mut a.value.0, cache, base_dir);
            }
            rewrite_block(body, cache, base_dir);
        }
        Expr::RecordLit(fields) => {
            for (_, e) in fields {
                rewrite_expr(&mut e.0, cache, base_dir);
            }
        }
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::Ident(_) | Expr::RowRef => {}
    }
}
