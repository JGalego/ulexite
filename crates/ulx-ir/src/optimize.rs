//! §13.5's dead-artifact elimination: a binding never referenced downstream
//! is not scheduled at all, so a program doesn't spend tokens on output
//! nothing consumes. `Escalate` is the one exception — a human-approval
//! request is a real-world side effect that must happen even if its result
//! goes unused, so it is never eliminated.

use std::collections::HashSet;

use crate::types::*;

/// Recursively eliminates dead bindings from `block` (and every nested
/// block reachable from it). Returns the number of instructions removed.
pub fn eliminate_dead_bindings(block: &mut IrBlock) -> usize {
    let mut used = HashSet::new();
    if let Some(tail) = &block.tail {
        free_vars(tail, &mut used);
    }

    let mut removed = 0;
    let mut kept = Vec::with_capacity(block.insts.len());
    for mut inst in block.insts.drain(..).rev() {
        // `with`-block members carry their own names independently of the
        // enclosing instruction's `bind` (which is always `None` for a
        // `with` block) — prune unused members individually rather than
        // keep-or-drop the whole block.
        if inst.bind.is_none() {
            if let IrExpr::Parallel(members) = &mut inst.expr {
                let before = members.len();
                members.retain(|(name, _)| used.contains(name));
                removed += before - members.len();
                if members.is_empty() {
                    removed += 1;
                    continue;
                }
                for (_, e) in members.iter() {
                    free_vars(e, &mut used);
                }
                kept.push(inst);
                continue;
            }
        }

        let is_used = match &inst.bind {
            Some(name) => used.contains(name),
            None => true, // unbound instructions (matches, loops used for effect) are always kept
        };
        let eliminable = is_eliminable(&inst.expr);

        if !is_used && eliminable {
            removed += 1;
            continue;
        }

        removed += optimize_expr(&mut inst.expr);
        free_vars(&inst.expr, &mut used);
        kept.push(inst);
    }
    kept.reverse();
    block.insts = kept;
    removed
}

fn is_eliminable(expr: &IrExpr) -> bool {
    !matches!(expr, IrExpr::Effect(e) if matches!(**e, IrEffect::Escalate { .. }))
}

fn optimize_expr(expr: &mut IrExpr) -> usize {
    match expr {
        IrExpr::If {
            then_block,
            else_block,
            ..
        } => eliminate_dead_bindings(then_block) + eliminate_dead_bindings(else_block),
        IrExpr::Retry { body, .. } => eliminate_dead_bindings(body),
        IrExpr::For { body, .. } | IrExpr::While { body, .. } => eliminate_dead_bindings(body),
        IrExpr::Match { arms, .. } => arms
            .iter_mut()
            .map(|arm| match &mut arm.body {
                IrArmBody::Block(b) => eliminate_dead_bindings(b),
                IrArmBody::Expr(_) => 0,
            })
            .sum(),
        IrExpr::Effect(e) => {
            if let IrEffect::Ask { .. } = e.as_mut() {
                // ask bodies are flat message lists, nothing to recurse into
                0
            } else {
                0
            }
        }
        _ => 0,
    }
}

fn free_vars(expr: &IrExpr, out: &mut HashSet<String>) {
    match expr {
        IrExpr::Var(name) => {
            out.insert(name.clone());
        }
        IrExpr::FieldAccess { base, .. } => free_vars(base, out),
        IrExpr::OpaqueCall { callee, args } => {
            free_vars(callee, out);
            for a in args {
                free_vars(&a.value, out);
            }
        }
        IrExpr::Index { base, index } => {
            free_vars(base, out);
            free_vars(index, out);
        }
        IrExpr::Unary { expr, .. } => free_vars(expr, out),
        IrExpr::Binary { lhs, rhs, .. } => {
            free_vars(lhs, out);
            free_vars(rhs, out);
        }
        IrExpr::If {
            cond,
            then_block,
            else_block,
        } => {
            free_vars(cond, out);
            free_vars_block(then_block, out);
            free_vars_block(else_block, out);
        }
        IrExpr::GenericCall { args, .. } => {
            for a in args {
                free_vars(&a.value, out);
            }
        }
        IrExpr::Retry {
            body, else_expr, ..
        } => {
            free_vars_block(body, out);
            if let Some(e) = else_expr {
                free_vars(e, out);
            }
        }
        IrExpr::Match { scrutinee, arms } => {
            free_vars(scrutinee, out);
            for arm in arms {
                match &arm.body {
                    IrArmBody::Expr(e) => free_vars(e, out),
                    IrArmBody::Block(b) => free_vars_block(b, out),
                }
            }
        }
        IrExpr::For { iter, body, .. } => {
            free_vars(iter, out);
            free_vars_block(body, out);
        }
        IrExpr::While { cond, body } => {
            free_vars(cond, out);
            free_vars_block(body, out);
        }
        IrExpr::Break(Some(e)) => free_vars(e, out),
        IrExpr::Parallel(members) => {
            for (_, e) in members {
                free_vars(e, out);
            }
        }
        IrExpr::Record(fields) => {
            for (_, e) in fields {
                free_vars(e, out);
            }
        }
        IrExpr::TextBlock(parts) => {
            for p in parts {
                if let IrTextPart::Interp(e) = p {
                    free_vars(e, out);
                }
            }
        }
        IrExpr::Effect(eff) => match eff.as_ref() {
            IrEffect::Ask { args, messages, .. } => {
                for a in args {
                    free_vars(&a.value, out);
                }
                for (_, m) in messages {
                    free_vars(m, out);
                }
            }
            IrEffect::Judge { args, .. } | IrEffect::Validator { args, .. } => {
                for a in args {
                    free_vars(&a.value, out);
                }
            }
            IrEffect::Escalate { args, .. } => {
                for (_, e) in args {
                    free_vars(e, out);
                }
            }
            IrEffect::ConversationCall { args, .. } => {
                for a in args {
                    free_vars(&a.value, out);
                }
            }
        },
        IrExpr::Int(_)
        | IrExpr::Float(_)
        | IrExpr::Str(_)
        | IrExpr::RowRef
        | IrExpr::Break(None) => {}
    }
}

fn free_vars_block(block: &IrBlock, out: &mut HashSet<String>) {
    for inst in &block.insts {
        free_vars(&inst.expr, out);
    }
    if let Some(t) = &block.tail {
        free_vars(t, out);
    }
}
