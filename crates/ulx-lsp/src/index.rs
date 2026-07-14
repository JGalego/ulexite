//! One AST walk over a parsed `Program` producing everything hover,
//! go-to-definition, and completion need: a map of top-level declarations
//! plus every reference to a name found inside declaration bodies.
//!
//! `ulx-ast` spans every *declaration* (`Spanned<TopDecl>`) but not
//! individual name tokens — `ConversationDecl.name` is a plain `String`,
//! not `Spanned<String>` (same for every other decl/param name). Widening
//! that would ripple through `ulx-sema`'s and `ulx-ir`'s pattern matches
//! and every test that destructures a decl name, which is out of scope for
//! adding a language server. So this index deliberately mixes coarse spans
//! (a whole declaration, a whole `ask` block) with tight spans
//! (`Expr::Ident`, `TypeExpr::Named`, judge/validator call names, which
//! *do* carry their own `Spanned<_>` wrapper) and `lookup` always prefers
//! whichever containing span is smallest — that one rule is what makes a
//! precise reference nested inside a coarse declaration resolve to the
//! tight reference rather than falling back to "the whole enclosing decl."

use std::collections::HashMap;

use ulx_ast::{
    ArtifactType, BenchmarkStmt, Block, ConversationDecl, DatasetSource, Expr, GenericArg, Import,
    MatchArmBody, Param, Program, ProviderDecl, RubricDecl, Span, Spanned, Stmt, TextPart, TopDecl,
    TypeExpr,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Conversation,
    Judge,
    Validator,
    Dataset,
    Type,
    Benchmark,
    Provider,
}

impl DeclKind {
    pub fn label(&self) -> &'static str {
        match self {
            DeclKind::Conversation => "conversation",
            DeclKind::Judge => "judge",
            DeclKind::Validator => "validator",
            DeclKind::Dataset => "dataset",
            DeclKind::Type => "type",
            DeclKind::Benchmark => "benchmark",
            DeclKind::Provider => "provider",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeclEntry {
    pub kind: DeclKind,
    pub name: String,
    pub span: Span,
    pub doc: Option<String>,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RefTarget {
    /// References a top-level decl by name — resolved against this file's
    /// `decls` first, then (via `import_sources`) an imported file.
    Name(String),
    /// References a stdlib capability (`ask <capability>(...)`).
    Capability(String),
    /// A `TypeExpr::Artifact` occurrence — hover-only, nothing to jump to.
    ArtifactType(ArtifactType),
}

pub struct Index {
    pub decls: HashMap<String, DeclEntry>,
    /// Name -> the `from "..."` path it was imported from (`import judge
    /// Fluency from "translate.ulx"`), for cross-file go-to-definition.
    pub import_sources: HashMap<String, String>,
    refs: Vec<(Span, RefTarget)>,
}

impl Index {
    pub fn build(program: &Program) -> Self {
        let mut decls = HashMap::new();
        for (decl, span) in &program.decls {
            let entry = decl_entry(decl, span.clone());
            decls.insert(entry.name.clone(), entry);
        }

        let mut refs = Vec::new();
        // Register every decl's own (coarse) span so hovering anywhere in
        // a declaration that isn't covered by a tighter reference below
        // still resolves to that declaration.
        for (name, entry) in &decls {
            refs.push((entry.span.clone(), RefTarget::Name(name.clone())));
        }

        let mut import_sources = HashMap::new();
        for (import, span) in &program.imports {
            if let Import::Named { name, from, .. } = import {
                import_sources.insert(name.clone(), from.clone());
                refs.push((span.clone(), RefTarget::Name(name.clone())));
            }
        }

        for (decl, _) in &program.decls {
            walk_decl(decl, &decls, &mut refs);
        }

        Index {
            decls,
            import_sources,
            refs,
        }
    }

    /// The reference/declaration whose span most tightly contains `offset`
    /// (smallest span wins), or `None` if nothing at all covers it.
    pub fn lookup(&self, offset: usize) -> Option<&RefTarget> {
        self.refs
            .iter()
            .filter(|(span, _)| span.start <= offset && offset <= span.end)
            .min_by_key(|(span, _)| span.end.saturating_sub(span.start))
            .map(|(_, target)| target)
    }
}

fn decl_entry(decl: &TopDecl, span: Span) -> DeclEntry {
    match decl {
        TopDecl::Conversation(c) => DeclEntry {
            kind: DeclKind::Conversation,
            name: c.name.clone(),
            span,
            doc: c.doc.clone(),
            signature: conversation_signature(c),
        },
        TopDecl::Judge(r) => DeclEntry {
            kind: DeclKind::Judge,
            name: r.name.clone(),
            span,
            doc: r.doc.clone(),
            signature: rubric_signature("judge", r),
        },
        TopDecl::Validator(r) => DeclEntry {
            kind: DeclKind::Validator,
            name: r.name.clone(),
            span,
            doc: r.doc.clone(),
            signature: rubric_signature("validator", r),
        },
        TopDecl::Dataset(d) => DeclEntry {
            kind: DeclKind::Dataset,
            name: d.name.clone(),
            span,
            doc: d.doc.clone(),
            signature: format!("dataset {}: {}", d.name, type_expr_str(&d.ty.0)),
        },
        TopDecl::Type(t) => DeclEntry {
            kind: DeclKind::Type,
            name: t.name.clone(),
            span,
            doc: None,
            signature: format!("type {} = {}", t.name, type_expr_str(&t.ty.0)),
        },
        TopDecl::Benchmark(b) => DeclEntry {
            kind: DeclKind::Benchmark,
            name: b.name.clone(),
            span,
            doc: b.doc.clone(),
            signature: format!("benchmark {}", b.name),
        },
        TopDecl::Provider(p) => DeclEntry {
            kind: DeclKind::Provider,
            name: p.name.clone(),
            span,
            doc: p.doc.clone(),
            signature: provider_signature(p),
        },
    }
}

fn params_str(params: &[Param]) -> String {
    params
        .iter()
        .map(|p| format!("{}: {}", p.name, type_expr_str(&p.ty.0)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn conversation_signature(c: &ConversationDecl) -> String {
    let ret = c
        .ret
        .as_ref()
        .map(|(t, _)| format!(" -> {}", type_expr_str(t)))
        .unwrap_or_default();
    format!("conversation {}({}){ret}", c.name, params_str(&c.params))
}

fn rubric_signature(keyword: &str, r: &RubricDecl) -> String {
    format!(
        "{keyword} {}({}) -> {}",
        r.name,
        params_str(&r.params),
        type_expr_str(&r.ret.0)
    )
}

fn provider_signature(p: &ProviderDecl) -> String {
    match &p.from {
        Some(from) => format!("provider {} from \"{from}\"", p.name),
        None => format!("provider {}", p.name),
    }
}

fn artifact_type_str(a: ArtifactType) -> &'static str {
    ArtifactType::ALL
        .iter()
        .find(|(_, ty)| *ty == a)
        .map(|(kw, _)| *kw)
        .unwrap_or("text")
}

fn type_expr_str(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Artifact(a) => artifact_type_str(*a).to_string(),
        TypeExpr::Named(n) => n.clone(),
        TypeExpr::Array(inner) => format!("[{}]", type_expr_str(&inner.0)),
        TypeExpr::Generic { name, arg } => format!("{name}<{}>", generic_arg_str(arg)),
        TypeExpr::Record(fields) => {
            let rendered: Vec<String> = fields
                .iter()
                .map(|(n, t)| format!("{n}: {}", type_expr_str(&t.0)))
                .collect();
            format!("{{ {} }}", rendered.join(", "))
        }
        TypeExpr::Union(variants) => variants
            .iter()
            .map(|v| match &v.payload {
                Some(p) => format!("{}({})", v.name, type_expr_str(&p.0)),
                None => v.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn generic_arg_str(arg: &GenericArg) -> String {
    match arg {
        GenericArg::Type(t) => type_expr_str(&t.0),
        GenericArg::Const(n) => n.to_string(),
    }
}

fn walk_type_expr(t: &Spanned<TypeExpr>, refs: &mut Vec<(Span, RefTarget)>) {
    let (ty, span) = t;
    match ty {
        TypeExpr::Artifact(a) => refs.push((span.clone(), RefTarget::ArtifactType(*a))),
        TypeExpr::Named(name) => refs.push((span.clone(), RefTarget::Name(name.clone()))),
        TypeExpr::Array(inner) => walk_type_expr(inner, refs),
        TypeExpr::Generic { arg, .. } => {
            if let GenericArg::Type(inner) = arg {
                walk_type_expr(inner, refs);
            }
        }
        TypeExpr::Record(fields) => {
            for (_, field_ty) in fields {
                walk_type_expr(field_ty, refs);
            }
        }
        TypeExpr::Union(variants) => {
            for v in variants {
                if let Some(p) = &v.payload {
                    walk_type_expr(p, refs);
                }
            }
        }
    }
}

fn walk_decl(
    decl: &TopDecl,
    decls: &HashMap<String, DeclEntry>,
    refs: &mut Vec<(Span, RefTarget)>,
) {
    match decl {
        TopDecl::Conversation(c) => {
            for p in &c.params {
                walk_type_expr(&p.ty, refs);
            }
            if let Some(ret) = &c.ret {
                walk_type_expr(ret, refs);
            }
            walk_block(&c.body, decls, refs);
        }
        TopDecl::Judge(r) | TopDecl::Validator(r) => {
            for p in &r.params {
                walk_type_expr(&p.ty, refs);
            }
            walk_type_expr(&r.ret, refs);
            for (_, expr) in &r.fields {
                walk_expr(expr, decls, refs);
            }
        }
        TopDecl::Dataset(d) => {
            walk_type_expr(&d.ty, refs);
            if let DatasetSource::Rows(rows) = &d.source {
                for row in rows {
                    for (_, expr) in row {
                        walk_expr(expr, decls, refs);
                    }
                }
            }
        }
        TopDecl::Type(t) => walk_type_expr(&t.ty, refs),
        TopDecl::Benchmark(b) => {
            for (stmt, _) in &b.stmts {
                walk_benchmark_stmt(stmt, decls, refs);
            }
        }
        TopDecl::Provider(p) => {
            for (_, expr) in &p.fields {
                walk_expr(expr, decls, refs);
            }
        }
    }
}

fn walk_benchmark_stmt(
    stmt: &BenchmarkStmt,
    decls: &HashMap<String, DeclEntry>,
    refs: &mut Vec<(Span, RefTarget)>,
) {
    match stmt {
        // `dataset` names a top-level `dataset` decl but the grammar
        // stores it as a bare `String` with no span of its own — nothing
        // tight to register here (same AST limitation as decl names).
        BenchmarkStmt::Dataset(_) => {}
        BenchmarkStmt::Run { expr, .. } => walk_expr(expr, decls, refs),
        BenchmarkStmt::Expect { expr, judge, .. } => {
            walk_expr(expr, decls, refs);
            walk_expr(judge, decls, refs);
        }
        BenchmarkStmt::Assert(e) => walk_expr(e, decls, refs),
        BenchmarkStmt::Snapshot { expr, key } => {
            walk_expr(expr, decls, refs);
            walk_expr(key, decls, refs);
        }
    }
}

fn walk_block(
    block: &Block,
    decls: &HashMap<String, DeclEntry>,
    refs: &mut Vec<(Span, RefTarget)>,
) {
    for (stmt, span) in &block.stmts {
        walk_stmt(stmt, span.clone(), decls, refs);
    }
    if let Some(tail) = &block.tail {
        walk_expr(tail, decls, refs);
    }
}

fn walk_stmt(
    stmt: &Stmt,
    stmt_span: Span,
    decls: &HashMap<String, DeclEntry>,
    refs: &mut Vec<(Span, RefTarget)>,
) {
    match stmt {
        Stmt::Message { text, .. } => walk_expr(text, decls, refs),
        Stmt::AssistantBind { ty, .. } => {
            if let Some(t) = ty {
                walk_type_expr(t, refs);
            }
        }
        Stmt::With(bindings) => {
            for b in bindings {
                walk_expr(&b.value, decls, refs);
            }
        }
        Stmt::Ask {
            capability,
            args,
            body,
            bind_ty,
            ..
        } => {
            refs.push((stmt_span, RefTarget::Capability(capability.clone())));
            for arg in args {
                walk_expr(&arg.value, decls, refs);
            }
            walk_block(body, decls, refs);
            if let Some(t) = bind_ty {
                walk_type_expr(t, refs);
            }
        }
        Stmt::Binding(b) => walk_expr(&b.value, decls, refs),
        Stmt::Match(m) => {
            walk_expr(&m.scrutinee, decls, refs);
            for arm in &m.arms {
                match &arm.body {
                    MatchArmBody::Expr(e) => walk_expr(e, decls, refs),
                    MatchArmBody::Block(b) => walk_block(b, decls, refs),
                }
            }
        }
        Stmt::For { iter, body, .. } => {
            walk_expr(iter, decls, refs);
            walk_block(body, decls, refs);
        }
        Stmt::While { cond, body } => {
            walk_expr(cond, decls, refs);
            walk_block(body, decls, refs);
        }
        Stmt::Break(e) => {
            if let Some(e) = e {
                walk_expr(e, decls, refs);
            }
        }
        Stmt::Expr(e) => walk_expr(e, decls, refs),
    }
}

fn walk_expr(
    expr: &Spanned<Expr>,
    decls: &HashMap<String, DeclEntry>,
    refs: &mut Vec<(Span, RefTarget)>,
) {
    let (e, span) = expr;
    match e {
        Expr::Ident(name) => {
            if decls.contains_key(name) {
                refs.push((span.clone(), RefTarget::Name(name.clone())));
            }
        }
        Expr::TextBlock(parts) => {
            for part in parts {
                if let TextPart::Interp(inner) = part {
                    walk_expr(inner, decls, refs);
                }
            }
        }
        Expr::RecordLit(fields) => {
            for (_, v) in fields {
                walk_expr(v, decls, refs);
            }
        }
        Expr::If {
            cond,
            then_block,
            else_block,
        } => {
            walk_expr(cond, decls, refs);
            walk_block(then_block, decls, refs);
            walk_block(else_block, decls, refs);
        }
        Expr::GenericCall { ty_arg, args, .. } => {
            walk_type_expr(ty_arg, refs);
            for a in args {
                walk_expr(&a.value, decls, refs);
            }
        }
        Expr::Retry {
            body, else_expr, ..
        } => {
            walk_block(body, decls, refs);
            if let Some(e) = else_expr {
                walk_expr(e, decls, refs);
            }
        }
        Expr::Escalate { args, .. } => {
            for (_, v) in args {
                walk_expr(v, decls, refs);
            }
        }
        Expr::JudgeCall { name, args } | Expr::ValidatorCall { name, args } => {
            if decls.contains_key(name) {
                refs.push((span.clone(), RefTarget::Name(name.clone())));
            }
            for a in args {
                walk_expr(&a.value, decls, refs);
            }
        }
        Expr::AskExpr {
            capability,
            args,
            body,
        } => {
            refs.push((span.clone(), RefTarget::Capability(capability.clone())));
            for a in args {
                walk_expr(&a.value, decls, refs);
            }
            walk_block(body, decls, refs);
        }
        Expr::RowRef => {}
        Expr::FieldAccess { base, .. } => walk_expr(base, decls, refs),
        Expr::Call { callee, args } => {
            walk_expr(callee, decls, refs);
            for a in args {
                walk_expr(&a.value, decls, refs);
            }
        }
        Expr::Index { base, index } => {
            walk_expr(base, decls, refs);
            walk_expr(index, decls, refs);
        }
        Expr::Unary { expr, .. } => walk_expr(expr, decls, refs),
        Expr::Binary { lhs, rhs, .. } => {
            walk_expr(lhs, decls, refs);
            walk_expr(rhs, decls, refs);
        }
        Expr::Int(_) | Expr::Float(_) | Expr::Str(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Program {
        ulx_syntax::parse_source(src).expect("fixture should parse")
    }

    #[test]
    fn finds_top_level_decl_with_signature() {
        let src = "conversation Greet(name: text) -> text {\n  \"hi\"\n}\n";
        let index = Index::build(&parse(src));
        let entry = index.decls.get("Greet").expect("Greet should be indexed");
        assert_eq!(entry.kind, DeclKind::Conversation);
        assert_eq!(entry.signature, "conversation Greet(name: text) -> text");
    }

    #[test]
    fn call_reference_resolves_tighter_than_enclosing_decl() {
        let src = "conversation A() -> text { \"x\" }\nconversation B() -> text { A() }\n";
        let index = Index::build(&parse(src));
        let call_offset = src.rfind("A()").unwrap();
        match index.lookup(call_offset) {
            Some(RefTarget::Name(name)) => assert_eq!(name, "A"),
            other => panic!("expected Name(\"A\"), got {other:?}"),
        }
    }

    #[test]
    fn hovering_inside_decl_but_outside_any_ref_falls_back_to_decl() {
        let src = "conversation A() -> text { \"x\" }\n";
        let index = Index::build(&parse(src));
        // offset on the keyword "conversation" itself — not a ref, only
        // the coarse whole-decl span covers it.
        let offset = 2;
        match index.lookup(offset) {
            Some(RefTarget::Name(name)) => assert_eq!(name, "A"),
            other => panic!("expected fallback to Name(\"A\"), got {other:?}"),
        }
    }

    #[test]
    fn capability_reference_resolves_inside_ask_block() {
        let src = "conversation C() -> text {\n  ask chat() {\n    user: \"\"\"hi\"\"\"\n  } -> out: text\n  out\n}\n";
        let index = Index::build(&parse(src));
        let offset = src.find("chat").unwrap();
        match index.lookup(offset) {
            Some(RefTarget::Capability(name)) => assert_eq!(name, "chat"),
            other => panic!("expected Capability(\"chat\"), got {other:?}"),
        }
    }

    #[test]
    fn named_type_reference_is_tight() {
        let src = "type Thing = { x: text }\nconversation UseThing(t: Thing) -> text { \"x\" }\n";
        let index = Index::build(&parse(src));
        let offset = src.rfind("Thing").unwrap();
        match index.lookup(offset) {
            Some(RefTarget::Name(name)) => assert_eq!(name, "Thing"),
            other => panic!("expected Name(\"Thing\"), got {other:?}"),
        }
    }

    #[test]
    fn import_records_source_path() {
        let src =
            "import judge Fluency from \"translate.ulx\"\nconversation C() -> text { \"x\" }\n";
        let index = Index::build(&parse(src));
        assert_eq!(
            index.import_sources.get("Fluency").map(String::as_str),
            Some("translate.ulx")
        );
    }
}
