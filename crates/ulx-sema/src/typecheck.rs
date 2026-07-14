use std::collections::HashSet;

use ulx_ast::*;

use crate::capability::CapabilitySpec;
use crate::diagnostic::Diagnostic;
use crate::scope::{InferredType, Scope};

/// Shared state threaded through one declaration's type-check pass.
pub struct Ctx<'a> {
    pub caps: &'a [CapabilitySpec],
    /// Names resolvable beyond local scope (sibling top-level decls, module
    /// aliases, imported names). `None` means "don't attempt undefined-name
    /// checking" — see `lib.rs`'s `analyze()` vs `load_and_analyze()` split.
    pub globals: Option<&'a HashSet<String>>,
    /// Names declared as a `judge` or `validator` (own file + imports of
    /// that kind), for checking `judge X(...)`/`validator X(...)` calls
    /// reference something that actually exists. Same `None` convention as
    /// `globals`.
    pub judges_and_validators: Option<&'a HashSet<String>>,
    /// Names declared as a `provider` (own file + imports), for checking
    /// `ask cap(provider: "X")`'s reserved arg references something real.
    /// Same `None` convention as `globals`.
    pub providers: Option<&'a HashSet<String>>,
    /// `ulexite.toml` `[providers.*]` entry names next to the file being
    /// checked, if `ulx-cli` found a manifest — `None` if it didn't (in
    /// which case `from`/`provider:` references naming a raw manifest entry
    /// can't be validated here at all, only at `ulx run`).
    pub known_manifest_providers: Option<&'a HashSet<String>>,
    pub diags: &'a mut Vec<Diagnostic>,
}

pub fn check_decl(decl: &TopDecl, caps: &[CapabilitySpec], diags: &mut Vec<Diagnostic>) {
    let mut ctx = Ctx {
        caps,
        globals: None,
        judges_and_validators: None,
        providers: None,
        known_manifest_providers: None,
        diags,
    };
    check_decl_with(decl, &mut ctx);
}

pub fn check_decl_with(decl: &TopDecl, ctx: &mut Ctx) {
    match decl {
        TopDecl::Conversation(c) => check_conversation(c, ctx),
        TopDecl::Judge(r) | TopDecl::Validator(r) => check_rubric(r, ctx),
        TopDecl::Dataset(_) => {}
        TopDecl::Type(_) => {}
        TopDecl::Benchmark(b) => check_benchmark(b, ctx),
        TopDecl::Provider(p) => check_provider(p, ctx),
    }
}

fn check_conversation(c: &ConversationDecl, ctx: &mut Ctx) {
    let mut scope = Scope::new();
    for p in &c.params {
        scope.declare(p.name.clone(), InferredType::Known(p.ty.0.clone()));
    }
    check_block(&c.body, &mut scope, ctx);
}

fn check_rubric(r: &RubricDecl, ctx: &mut Ctx) {
    let mut scope = Scope::new();
    for p in &r.params {
        scope.declare(p.name.clone(), InferredType::Known(p.ty.0.clone()));
    }
    for (_, (expr, span)) in &r.fields {
        check_expr(&(expr.clone(), span.clone()), &mut scope, ctx);
    }
}

fn check_benchmark(b: &BenchmarkDecl, ctx: &mut Ctx) {
    let mut scope = Scope::new();
    scope.declare("$", InferredType::Unknown);
    for (stmt, _) in &b.stmts {
        match stmt {
            BenchmarkStmt::Dataset(_) => {}
            BenchmarkStmt::Run { expr, bind } => {
                check_expr(expr, &mut scope, ctx);
                scope.declare(bind.clone(), InferredType::Unknown);
            }
            BenchmarkStmt::Expect { expr, judge, .. } => {
                check_expr(expr, &mut scope, ctx);
                check_expr(judge, &mut scope, ctx);
            }
            BenchmarkStmt::Assert(expr) => check_expr(expr, &mut scope, ctx),
            BenchmarkStmt::Snapshot { expr, key } => {
                check_expr(expr, &mut scope, ctx);
                check_expr(key, &mut scope, ctx);
            }
        }
    }
}

const PROVIDER_SCALAR_FIELDS: [&str; 4] = ["vendor", "api_key_env", "base_url", "api_version"];

/// Validates a `provider Name [from "entry"] { field: expr ... }` decl
/// (§12.4's "declare a provider from `.ulx` source" extension). Fields are
/// plain config, never executable code, so this checks *shape* (known
/// scalar fields are strings, capability fields are a bare model string or
/// a `{ model: ..., ... }` record of plain literals), not scope/type
/// inference the way `check_conversation`/`check_rubric` do.
fn check_provider(p: &ProviderDecl, ctx: &mut Ctx) {
    let mut seen_fields: HashSet<&str> = HashSet::new();
    let mut has_vendor = false;
    for (name, value) in &p.fields {
        if !seen_fields.insert(name.as_str()) {
            ctx.diags.push(Diagnostic::error(
                format!("provider `{}` has more than one `{name}` field", p.name),
                value.1.clone(),
            ));
            continue;
        }
        if PROVIDER_SCALAR_FIELDS.contains(&name.as_str()) {
            if name == "vendor" {
                has_vendor = true;
            }
            if !matches!(value.0, Expr::Str(_)) {
                ctx.diags.push(Diagnostic::error(
                    format!("provider `{}` field `{name}` must be a string", p.name),
                    value.1.clone(),
                ));
            }
        } else {
            check_provider_capability_value(&p.name, name, value, ctx);
        }
    }

    if let Some(from) = &p.from {
        if has_vendor {
            ctx.diags.push(Diagnostic::error(
                format!(
                    "provider `{}` declares both `from` and `vendor` — `vendor` is inherited from \
                     the `from` entry; remove one",
                    p.name
                ),
                // Field-less span fallback: the decl has no single "from"
                // span tracked yet, so anchor on the first field if any,
                // else this is still useful without one.
                p.fields.first().map(|(_, v)| v.1.clone()).unwrap_or(0..0),
            ));
        }
        if let Some(known) = ctx.known_manifest_providers {
            if !known.contains(from) {
                ctx.diags.push(Diagnostic::error(
                    format!(
                        "provider `{}` has `from \"{from}\"`, but ulexite.toml has no `[providers.{from}]` entry",
                        p.name
                    ),
                    p.fields.first().map(|(_, v)| v.1.clone()).unwrap_or(0..0),
                ));
            }
        }
    } else if !has_vendor {
        ctx.diags.push(Diagnostic::error(
            format!(
                "provider `{}` has no `from` and no `vendor` — a standalone provider block must \
                 declare one",
                p.name
            ),
            p.fields.first().map(|(_, v)| v.1.clone()).unwrap_or(0..0),
        ));
    }
}

fn check_provider_capability_value(
    provider_name: &str,
    field: &str,
    value: &Spanned<Expr>,
    ctx: &mut Ctx,
) {
    match &value.0 {
        Expr::Str(_) => {}
        Expr::RecordLit(inner_fields) => {
            for (inner_name, inner_value) in inner_fields {
                if !matches!(inner_value.0, Expr::Str(_) | Expr::Int(_) | Expr::Float(_)) {
                    ctx.diags.push(Diagnostic::error(
                        format!(
                            "provider `{provider_name}` capability `{field}` field `{inner_name}` \
                             must be a plain string, int, or float — provider config isn't \
                             executable code"
                        ),
                        inner_value.1.clone(),
                    ));
                }
            }
        }
        _ => {
            ctx.diags.push(Diagnostic::error(
                format!(
                    "provider `{provider_name}` capability `{field}` must be a bare model-name \
                     string or a `{{ model: \"...\", ... }}` record"
                ),
                value.1.clone(),
            ));
        }
    }
}

fn check_block(block: &Block, scope: &mut Scope, ctx: &mut Ctx) {
    scope.push();
    check_with_block_independence(block, ctx);
    for (stmt, span) in &block.stmts {
        check_stmt(stmt, span, scope, ctx);
    }
    if let Some(tail) = &block.tail {
        check_expr(tail, scope, ctx);
    }
    scope.pop();
}

fn check_stmt(stmt: &Stmt, span: &Span, scope: &mut Scope, ctx: &mut Ctx) {
    match stmt {
        Stmt::Message { text, .. } => check_expr(text, scope, ctx),
        Stmt::AssistantBind { name, ty } => {
            scope.declare_typed(name.clone(), ty);
        }
        Stmt::With(bindings) => {
            for b in bindings {
                check_expr(&b.value, scope, ctx);
            }
            let mut seen = HashSet::new();
            for b in bindings {
                if !seen.insert(b.name.clone()) {
                    ctx.diags.push(Diagnostic::error(
                        format!(
                            "duplicate binding `{}` in a `with` block — concurrent writers to the \
                             same name require an explicit merge function (§9.5), which isn't \
                             expressible yet; rename one of the bindings",
                            b.name
                        ),
                        span.clone(),
                    ));
                }
                scope.declare(b.name.clone(), infer_expr_type(&b.value, scope));
            }
        }
        Stmt::Ask {
            capability,
            args,
            body,
            bind_name,
            bind_ty,
        } => {
            check_ask_call(capability, args, span, scope, ctx);
            check_block(body, scope, ctx);
            scope.declare_typed(bind_name.clone(), bind_ty);
        }
        Stmt::Binding(b) => {
            check_expr(&b.value, scope, ctx);
            scope.declare(b.name.clone(), infer_expr_type(&b.value, scope));
        }
        Stmt::Match(m) => check_match(m, scope, ctx),
        Stmt::For { var, iter, body } => {
            check_expr(iter, scope, ctx);
            scope.push();
            scope.declare(var.clone(), InferredType::Unknown);
            check_block(body, scope, ctx);
            scope.pop();
        }
        Stmt::While { cond, body } => {
            check_expr(cond, scope, ctx);
            check_block(body, scope, ctx);
        }
        Stmt::Break(e) => {
            if let Some(e) = e {
                check_expr(e, scope, ctx);
            }
        }
        Stmt::Expr(e) => check_expr(e, scope, ctx),
    }
}

/// §9.7: a `with` block's bindings must be independent — no binding's
/// value expression may reference a sibling binding from the *same* block.
/// (The parser doesn't enforce this structurally the way the grammar note
/// in §8.1 aspires to; this pass is the actual enforcement point for now.)
fn check_with_block_independence(block: &Block, ctx: &mut Ctx) {
    for (stmt, span) in &block.stmts {
        if let Stmt::With(bindings) = stmt {
            let names: HashSet<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
            for b in bindings {
                let mut refs = HashSet::new();
                collect_idents(&b.value.0, &mut refs);
                for r in &refs {
                    if names.contains(r.as_str()) && r != &b.name {
                        ctx.diags.push(Diagnostic::error(
                            format!(
                                "`with` block binding `{}` references sibling binding `{r}` — \
                                 bindings in the same `with` block must be independent (§9.7); \
                                 move this into a sequential statement outside the block",
                                b.name
                            ),
                            span.clone(),
                        ));
                    }
                }
            }
        }
    }
}

fn collect_idents(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Ident(s) => {
            out.insert(s.clone());
        }
        Expr::FieldAccess { base, .. } => collect_idents(&base.0, out),
        Expr::Call { callee, args } => {
            collect_idents(&callee.0, out);
            for a in args {
                collect_idents(&a.value.0, out);
            }
        }
        Expr::Index { base, index } => {
            collect_idents(&base.0, out);
            collect_idents(&index.0, out);
        }
        Expr::Unary { expr, .. } => collect_idents(&expr.0, out),
        Expr::Binary { lhs, rhs, .. } => {
            collect_idents(&lhs.0, out);
            collect_idents(&rhs.0, out);
        }
        Expr::If { cond, .. } => collect_idents(&cond.0, out),
        Expr::GenericCall { args, .. } => {
            for a in args {
                collect_idents(&a.value.0, out);
            }
        }
        Expr::Retry { else_expr, .. } => {
            if let Some(e) = else_expr {
                collect_idents(&e.0, out);
            }
        }
        Expr::Escalate { args, .. } => {
            for (_, e) in args {
                collect_idents(&e.0, out);
            }
        }
        Expr::JudgeCall { args, .. } | Expr::ValidatorCall { args, .. } => {
            for a in args {
                collect_idents(&a.value.0, out);
            }
        }
        Expr::AskExpr { args, .. } => {
            for a in args {
                collect_idents(&a.value.0, out);
            }
        }
        Expr::RecordLit(fields) => {
            for (_, e) in fields {
                collect_idents(&e.0, out);
            }
        }
        Expr::TextBlock(parts) => {
            for p in parts {
                if let TextPart::Interp((e, _)) = p {
                    collect_idents(e, out);
                }
            }
        }
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::RowRef => {}
    }
}

fn check_match(m: &MatchStmt, scope: &mut Scope, ctx: &mut Ctx) {
    check_expr(&m.scrutinee, scope, ctx);
    let is_verdict = matches!(
        &m.scrutinee.0,
        Expr::JudgeCall { .. } | Expr::ValidatorCall { .. }
    ) || matches!(
        infer_expr_type(&m.scrutinee, scope),
        InferredType::Known(TypeExpr::Named(n)) if n == "Verdict"
    );

    if is_verdict {
        const REQUIRED: [&str; 4] = ["Pass", "Fail", "Score", "Escalate"];
        let mut covered: HashSet<&str> = HashSet::new();
        let mut has_wildcard = false;
        for arm in &m.arms {
            match &arm.pattern {
                Pattern::Variant { name, .. } => {
                    covered.insert(name.as_str());
                }
                Pattern::Wildcard => has_wildcard = true,
            }
        }
        if !has_wildcard {
            let missing: Vec<&str> = REQUIRED
                .iter()
                .filter(|v| !covered.contains(*v))
                .copied()
                .collect();
            if !missing.is_empty() {
                ctx.diags.push(Diagnostic::error(
                    format!(
                        "non-exhaustive match over `Verdict`: missing variant(s) {} (§9.4) — \
                         add the missing arm(s) or a wildcard `_` arm",
                        missing.join(", ")
                    ),
                    m.scrutinee.1.clone(),
                ));
            }
        }
    }

    for arm in &m.arms {
        scope.push();
        if let Pattern::Variant { bindings, .. } = &arm.pattern {
            for b in bindings {
                scope.declare(b.clone(), InferredType::Unknown);
            }
        }
        match &arm.body {
            MatchArmBody::Expr(e) => check_expr(e, scope, ctx),
            MatchArmBody::Block(b) => check_block(b, scope, ctx),
        }
        scope.pop();
    }
}

/// §9.2/§11.5: check each positional argument's best-effort-inferred
/// artifact type against the capability's declared `accepts` set. Silent
/// (no diagnostic) whenever inference can't determine a type — see the
/// module docs' honesty principle.
fn check_ask_call(capability: &str, args: &[Arg], span: &Span, scope: &mut Scope, ctx: &mut Ctx) {
    for a in args {
        check_expr(&a.value, scope, ctx);
    }
    check_provider_arg_reference(args, ctx);
    let Some(spec) = ctx.caps.iter().find(|c| c.name == capability) else {
        ctx.diags.push(Diagnostic::warning(
            format!("`{capability}` is not a known stdlib capability (§15.1); skipping artifact-type checks for this call"),
            span.clone(),
        ));
        return;
    };
    for a in args {
        if let InferredType::Known(TypeExpr::Artifact(at)) = infer_expr_type(&a.value, scope) {
            if !spec.accepts.contains(&at) {
                ctx.diags.push(Diagnostic::error(
                    format!(
                        "capability `{capability}` accepts {:?} but this argument is `{:?}` (§9.2, §11.5)",
                        spec.accepts, at
                    ),
                    a.value.1.clone(),
                ));
            }
        }
    }
}

/// Checks `ask cap(provider: "X", ...)`'s reserved `provider` arg (§12.4 —
/// no new grammar, just a string-valued named arg the interpreter treats
/// specially) references a name that's either an in-scope `provider` decl
/// or a raw `ulexite.toml` entry. Silent whenever neither set is fully
/// known (mirrors `check_rubric_reference`'s "don't hard-error over a name
/// this pass genuinely can't see" convention) — hard error only when both
/// sets are available and the name is in neither.
fn check_provider_arg_reference(args: &[Arg], ctx: &mut Ctx) {
    let Some(arg) = args.iter().find(|a| a.name.as_deref() == Some("provider")) else {
        return;
    };
    let Expr::Str(name) = &arg.value.0 else {
        ctx.diags.push(Diagnostic::error(
            "`provider` must be a string naming a declared provider or ulexite.toml entry"
                .to_string(),
            arg.value.1.clone(),
        ));
        return;
    };
    if ctx.providers.is_none() && ctx.known_manifest_providers.is_none() {
        return;
    }
    let found = ctx.providers.is_some_and(|s| s.contains(name))
        || ctx
            .known_manifest_providers
            .is_some_and(|s| s.contains(name));
    if !found {
        ctx.diags.push(Diagnostic::error(
            format!(
                "`{name}` is not declared as a `provider` in this file or its imports, and no \
                 `ulexite.toml` entry named `{name}` exists"
            ),
            arg.value.1.clone(),
        ));
    }
}

/// Checks that a `judge X(...)`/`validator X(...)` call references a name
/// actually declared with that kind. Silent when `judges_and_validators` is
/// `None` (the standalone `analyze()` path, same convention as `globals`).
fn check_rubric_reference(name: &str, span: &Span, ctx: &mut Ctx) {
    if let Some(known) = ctx.judges_and_validators {
        if !known.contains(name) {
            ctx.diags.push(Diagnostic::warning(
                format!("`{name}` is not declared as a `judge` or `validator` in this file or its imports"),
                span.clone(),
            ));
        }
    }
}

fn check_expr(expr: &Spanned<Expr>, scope: &mut Scope, ctx: &mut Ctx) {
    let (node, span) = expr;
    match node {
        Expr::Ident(name) => {
            if let Some(globals) = ctx.globals {
                if !scope.contains(name) && !globals.contains(name) {
                    ctx.diags.push(Diagnostic::warning(
                        format!("`{name}` is not defined in this scope"),
                        span.clone(),
                    ));
                }
            }
        }
        Expr::FieldAccess { base, .. } => check_expr(base, scope, ctx),
        Expr::Call { callee, args } => {
            // `capability(embed)` (§5.5, §7.5, §12.4): the argument is a
            // symbolic capability-kind identifier, not a variable reference
            // — skip it the same way the runtime's `eval_opaque_call` does,
            // rather than flagging it as an undefined name.
            let is_capability_hint = matches!(&callee.0, Expr::Ident(n) if n == "capability");
            if !is_capability_hint {
                check_expr(callee, scope, ctx);
            }
            for a in args {
                if is_capability_hint && matches!(a.value.0, Expr::Ident(_)) {
                    continue;
                }
                check_expr(&a.value, scope, ctx);
            }
        }
        Expr::Index { base, index } => {
            check_expr(base, scope, ctx);
            check_expr(index, scope, ctx);
        }
        Expr::Unary { expr, .. } => check_expr(expr, scope, ctx),
        Expr::Binary { lhs, rhs, .. } => {
            check_expr(lhs, scope, ctx);
            check_expr(rhs, scope, ctx);
        }
        Expr::If {
            cond,
            then_block,
            else_block,
        } => {
            check_expr(cond, scope, ctx);
            check_block(then_block, scope, ctx);
            check_block(else_block, scope, ctx);
        }
        Expr::GenericCall { args, .. } => {
            for a in args {
                check_expr(&a.value, scope, ctx);
            }
        }
        Expr::Retry {
            body, else_expr, ..
        } => {
            check_block(body, scope, ctx);
            if let Some(e) = else_expr {
                check_expr(e, scope, ctx);
            }
        }
        Expr::Escalate { args, .. } => {
            for (_, e) in args {
                check_expr(e, scope, ctx);
            }
        }
        Expr::JudgeCall { name, args } | Expr::ValidatorCall { name, args } => {
            check_rubric_reference(name, span, ctx);
            for a in args {
                check_expr(&a.value, scope, ctx);
            }
        }
        Expr::AskExpr {
            capability,
            args,
            body,
        } => {
            check_ask_call(capability, args, span, scope, ctx);
            check_block(body, scope, ctx);
        }
        Expr::RecordLit(fields) => {
            for (_, e) in fields {
                check_expr(e, scope, ctx);
            }
        }
        Expr::TextBlock(parts) => {
            for p in parts {
                if let TextPart::Interp(e) = p {
                    check_expr(e, scope, ctx);
                }
            }
        }
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) | Expr::RowRef => {}
    }
}

fn infer_expr_type(expr: &Spanned<Expr>, scope: &Scope) -> InferredType {
    match &expr.0 {
        Expr::Ident(name) => scope
            .type_of(name)
            .cloned()
            .unwrap_or(InferredType::Unknown),
        Expr::Str(_) | Expr::TextBlock(_) => {
            InferredType::Known(TypeExpr::Artifact(ArtifactType::Text))
        }
        Expr::JudgeCall { .. } | Expr::ValidatorCall { .. } => {
            InferredType::Known(TypeExpr::Named("Verdict".to_string()))
        }
        _ => InferredType::Unknown,
    }
}
