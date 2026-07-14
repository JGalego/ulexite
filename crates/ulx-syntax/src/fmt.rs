//! AST-based pretty-printer for Ulexite (`ulx fmt`, §20.10).
//!
//! This is a canonical, opinionated formatter in the `gofmt`/`rustfmt` sense:
//! it re-emits a parsed [`Program`] with consistent 2-space indentation,
//! consistent spacing around `:`/`->`/`=>`/`{`/`}`, one statement per line,
//! and exactly one blank line between top-level declarations. It does
//! **not** attempt column alignment of `=`/`=>` the way some hand-written
//! examples in this repo use — that is a stylistic nicety, not a
//! correctness requirement, and skipping it keeps the printer simple.
//!
//! # Comments are NOT preserved (by design, for now)
//!
//! Ulexite's lexer (`lexer.rs`) discards comments and whitespace at lex
//! time (`#[logos(skip ...)]`) with no side-table capturing their text or
//! position, and the parser (`parser.rs`) builds directly into `ulx-ast`
//! types with no lossless/rowan-style CST layer in between — every AST
//! node's `doc: Option<String>` field is hardcoded to `None` at every
//! construction site. That means there is currently no data anywhere in
//! this compiler from which comment text could be recovered.
//!
//! Consequently: **`ulx fmt` silently drops every comment in the file.**
//! Adding comment/trivia preservation would require a real lexer + parser
//! change (a side-channel of skipped-trivia spans, threaded through parsing
//! and re-attached during printing) — a prerequisite project of its own,
//! deliberately out of scope here. Until that lands, `ulx fmt` is safe to
//! run only on files whose comments you're OK losing (or don't mind
//! re-adding by hand afterward).
//!
//! # Semantic vs. textual round-tripping
//!
//! Because there is no lossless CST, "round-trip" here means: parsing the
//! formatted output produces an AST equal (ignoring spans and `doc`
//! comments) to the original AST — not that the formatted bytes match the
//! original bytes. In particular, redundant source-level parentheses in
//! expressions are not preserved as such; the printer instead recomputes
//! exactly the parentheses needed to preserve operator-precedence
//! semantics (see `precedence`/`fmt_operand` below), since the AST has no
//! record of which parens were actually present in the source.

use ulx_ast::*;

/// Parses `src` and formats it, in one step. Returns the same parse errors
/// [`crate::parse_source`] would on invalid input.
pub fn format_source(src: &str) -> Result<String, Vec<crate::parser::Err>> {
    let program = crate::parser::parse_source(src)?;
    Ok(format_program(&program))
}

/// Pretty-prints an already-parsed [`Program`] into canonical `.ulx` source.
pub fn format_program(program: &Program) -> String {
    let mut out = String::new();
    for (imp, _) in &program.imports {
        out.push_str(&fmt_import(imp));
        out.push('\n');
    }
    if !program.imports.is_empty() && !program.decls.is_empty() {
        out.push('\n');
    }
    for (i, (decl, _)) in program.decls.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&fmt_top_decl(decl));
        out.push('\n');
    }
    out
}

fn ind(n: usize) -> String {
    "  ".repeat(n)
}

// ---------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------

fn kind_str(k: ImportKind) -> &'static str {
    match k {
        ImportKind::Conversation => "conversation",
        ImportKind::Judge => "judge",
        ImportKind::Validator => "validator",
        ImportKind::Dataset => "dataset",
        ImportKind::Type => "type",
        ImportKind::Provider => "provider",
    }
}

fn fmt_import(i: &Import) -> String {
    match i {
        Import::Named { kind, name, from } => {
            format!("import {} {name} from {}", kind_str(*kind), fmt_str_lit(from))
        }
        Import::Module { path, alias } => {
            format!("import {} as {alias}", fmt_str_lit(path))
        }
    }
}

// ---------------------------------------------------------------------
// Top-level declarations
// ---------------------------------------------------------------------

fn fmt_top_decl(d: &TopDecl) -> String {
    match d {
        TopDecl::Conversation(c) => fmt_conversation(c),
        TopDecl::Judge(r) => fmt_rubric("judge", r),
        TopDecl::Validator(r) => fmt_rubric("validator", r),
        TopDecl::Dataset(ds) => fmt_dataset(ds),
        TopDecl::Type(t) => format!("type {} = {}", t.name, fmt_type(&t.ty.0)),
        TopDecl::Benchmark(b) => fmt_benchmark(b),
        TopDecl::Provider(p) => fmt_provider(p),
    }
}

fn fmt_conversation(c: &ConversationDecl) -> String {
    let ret = c
        .ret
        .as_ref()
        .map(|t| format!(" -> {}", fmt_type(&t.0)))
        .unwrap_or_default();
    format!(
        "conversation {}({}){ret} {}",
        c.name,
        fmt_params(&c.params),
        fmt_block(&c.body, 0)
    )
}

fn fmt_rubric(head: &str, r: &RubricDecl) -> String {
    let ret = fmt_type(&r.ret.0);
    if r.fields.is_empty() {
        return format!("{head} {}({}) -> {ret} {{}}", r.name, fmt_params(&r.params));
    }
    let mut s = format!("{head} {}({}) -> {ret} {{\n", r.name, fmt_params(&r.params));
    for (k, v) in &r.fields {
        s.push_str(&ind(1));
        s.push_str(&format!("{k}: {}\n", fmt_operand(v, 0, 1)));
    }
    s.push('}');
    s
}

fn fmt_dataset(d: &DatasetDecl) -> String {
    let mut s = format!("dataset {}: {} {{\n", d.name, fmt_type(&d.ty.0));
    match &d.source {
        DatasetSource::FromFile(path) => {
            s.push_str(&ind(1));
            s.push_str(&format!("from {}\n", fmt_str_lit(path)));
        }
        DatasetSource::Rows(rows) => {
            s.push_str(&ind(1));
            s.push_str("[\n");
            for (i, row) in rows.iter().enumerate() {
                s.push_str(&ind(2));
                s.push_str(&fmt_record_fields(row, 2));
                if i + 1 < rows.len() {
                    s.push(',');
                }
                s.push('\n');
            }
            s.push_str(&ind(1));
            s.push_str("]\n");
        }
    }
    s.push('}');
    s
}

fn fmt_benchmark(b: &BenchmarkDecl) -> String {
    if b.stmts.is_empty() {
        return format!("benchmark {} {{}}", b.name);
    }
    let mut s = format!("benchmark {} {{\n", b.name);
    for (stmt, _) in &b.stmts {
        s.push_str(&ind(1));
        s.push_str(&fmt_benchmark_stmt(stmt, 1));
        s.push('\n');
    }
    s.push('}');
    s
}

fn fmt_benchmark_stmt(s: &BenchmarkStmt, indent: usize) -> String {
    match s {
        BenchmarkStmt::Dataset(name) => format!("dataset: {name}"),
        BenchmarkStmt::Run { expr, bind } => {
            format!("run: {} -> {bind}", fmt_operand(expr, 0, indent))
        }
        BenchmarkStmt::Expect {
            expr,
            judge,
            threshold,
        } => {
            let t = threshold
                .map(|t| format!(" with threshold({})", fmt_float(t)))
                .unwrap_or_default();
            format!(
                "expect {} satisfies {}{t}",
                fmt_operand(expr, 0, indent),
                fmt_operand(judge, 0, indent)
            )
        }
        BenchmarkStmt::Assert(e) => format!("assert {}", fmt_operand(e, 0, indent)),
        BenchmarkStmt::Snapshot { expr, key } => format!(
            "snapshot {} as {}",
            fmt_operand(expr, 0, indent),
            fmt_operand(key, 0, indent)
        ),
    }
}

fn fmt_provider(p: &ProviderDecl) -> String {
    let from = p
        .from
        .as_ref()
        .map(|f| format!(" from {}", fmt_str_lit(f)))
        .unwrap_or_default();
    if p.fields.is_empty() {
        return format!("provider {}{from} {{}}", p.name);
    }
    let mut s = format!("provider {}{from} {{\n", p.name);
    for (k, v) in &p.fields {
        s.push_str(&ind(1));
        s.push_str(&format!("{k}: {}\n", fmt_operand(v, 0, 1)));
    }
    s.push('}');
    s
}

fn fmt_params(params: &[Param]) -> String {
    params
        .iter()
        .map(|p| format!("{}: {}", p.name, fmt_type(&p.ty.0)))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------

fn artifact_keyword(a: ArtifactType) -> &'static str {
    match a {
        ArtifactType::Text => "text",
        ArtifactType::Markdown => "markdown",
        ArtifactType::Image => "image",
        ArtifactType::Audio => "audio",
        ArtifactType::Video => "video",
        ArtifactType::Pdf => "pdf",
        ArtifactType::Json => "json",
        ArtifactType::Xml => "xml",
        ArtifactType::Html => "html",
        ArtifactType::Csv => "csv",
        ArtifactType::Embedding => "embedding",
        ArtifactType::Vector => "vector",
        ArtifactType::ToolOutput => "tool_output",
    }
}

fn fmt_type(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Artifact(a) => artifact_keyword(*a).to_string(),
        TypeExpr::Record(fields) => {
            let inner = fields
                .iter()
                .map(|(n, t)| format!("{n}: {}", fmt_type(&t.0)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
        TypeExpr::Union(variants) => variants
            .iter()
            .map(fmt_variant)
            .collect::<Vec<_>>()
            .join(" | "),
        TypeExpr::Generic { name, arg } => format!("{name}<{}>", fmt_generic_arg(arg)),
        TypeExpr::Array(inner) => format!("[{}]", fmt_type(&inner.0)),
        TypeExpr::Named(n) => n.clone(),
    }
}

fn fmt_variant(v: &Variant) -> String {
    match &v.payload {
        Some(p) => format!("{}({})", v.name, fmt_type(&p.0)),
        None => v.name.clone(),
    }
}

fn fmt_generic_arg(a: &GenericArg) -> String {
    match a {
        GenericArg::Type(t) => fmt_type(&t.0),
        GenericArg::Const(i) => i.to_string(),
    }
}

// ---------------------------------------------------------------------
// Blocks, statements, patterns
// ---------------------------------------------------------------------

/// Prints a block. `indent` is the indentation level of the line the
/// opening `{` appears on; contents print at `indent + 1` and the closing
/// `}` prints at `indent` (the caller places the returned string directly
/// after whatever precedes the block on the same line, e.g. `ask f() `).
fn fmt_block(block: &Block, indent: usize) -> String {
    if block.stmts.is_empty() && block.tail.is_none() {
        return "{}".to_string();
    }
    let mut out = String::from("{\n");
    for (stmt, _) in &block.stmts {
        out.push_str(&fmt_stmt(stmt, indent + 1));
        out.push('\n');
    }
    if let Some(tail) = &block.tail {
        out.push_str(&ind(indent + 1));
        out.push_str(&fmt_operand(tail, 0, indent + 1));
        out.push('\n');
    }
    out.push_str(&ind(indent));
    out.push('}');
    out
}

fn role_str(r: MessageRole) -> &'static str {
    match r {
        MessageRole::System => "system",
        MessageRole::User => "user",
    }
}

fn fmt_stmt(stmt: &Stmt, indent: usize) -> String {
    let pad = ind(indent);
    match stmt {
        Stmt::Message { role, text } => {
            format!("{pad}{}: {}", role_str(*role), fmt_operand(text, 0, indent))
        }
        Stmt::AssistantBind { name, ty } => {
            let ty_s = ty
                .as_ref()
                .map(|t| format!(": {}", fmt_type(&t.0)))
                .unwrap_or_default();
            format!("{pad}assistant -> {name}{ty_s}")
        }
        Stmt::With(bindings) => {
            if bindings.is_empty() {
                format!("{pad}with {{}}")
            } else {
                let mut s = format!("{pad}with {{\n");
                for b in bindings {
                    s.push_str(&ind(indent + 1));
                    s.push_str(&format!(
                        "{} = {}\n",
                        b.name,
                        fmt_operand(&b.value, 0, indent + 1)
                    ));
                }
                s.push_str(&pad);
                s.push('}');
                s
            }
        }
        Stmt::Ask {
            capability,
            args,
            body,
            bind_name,
            bind_ty,
        } => {
            let ty_s = bind_ty
                .as_ref()
                .map(|t| format!(": {}", fmt_type(&t.0)))
                .unwrap_or_default();
            format!(
                "{pad}ask {capability}({}) {} -> {bind_name}{ty_s}",
                fmt_args(args, indent),
                fmt_block(body, indent)
            )
        }
        Stmt::Binding(b) => format!("{pad}{} = {}", b.name, fmt_operand(&b.value, 0, indent)),
        Stmt::Match(m) => fmt_match(m, indent),
        Stmt::For { var, iter, body } => format!(
            "{pad}for {var} in {} {}",
            fmt_operand(iter, 0, indent),
            fmt_block(body, indent)
        ),
        Stmt::While { cond, body } => format!(
            "{pad}while {} {}",
            fmt_operand(cond, 0, indent),
            fmt_block(body, indent)
        ),
        Stmt::Break(opt) => match opt {
            Some(e) => format!("{pad}break {}", fmt_operand(e, 0, indent)),
            None => format!("{pad}break"),
        },
        Stmt::Expr(e) => format!("{pad}{}", fmt_operand(e, 0, indent)),
    }
}

fn fmt_match(m: &MatchStmt, indent: usize) -> String {
    let pad = ind(indent);
    let mut s = format!("{pad}match {} {{\n", fmt_operand(&m.scrutinee, 0, indent));
    for arm in &m.arms {
        s.push_str(&ind(indent + 1));
        s.push_str(&fmt_pattern(&arm.pattern));
        s.push_str(" => ");
        match &arm.body {
            MatchArmBody::Expr(e) => s.push_str(&fmt_operand(e, 0, indent + 1)),
            MatchArmBody::Block(b) => s.push_str(&fmt_block(b, indent + 1)),
        }
        s.push('\n');
    }
    s.push_str(&pad);
    s.push('}');
    s
}

fn fmt_pattern(p: &Pattern) -> String {
    match p {
        Pattern::Wildcard => "_".to_string(),
        Pattern::Variant { name, bindings } => {
            if bindings.is_empty() {
                name.clone()
            } else {
                format!("{name}({})", bindings.join(", "))
            }
        }
    }
}

// ---------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------

/// Binary-operator precedence, tightest-binds-highest — mirrors the
/// left-to-right precedence chain built in `parser.rs`'s `program_pieces`
/// (`or` < `and` < comparison < `+`/`-` < `*`/`/`).
fn precedence_of_binop(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Or => 1,
        BinaryOp::And => 2,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            3
        }
        BinaryOp::Add | BinaryOp::Sub => 4,
        BinaryOp::Mul | BinaryOp::Div => 5,
    }
}

fn binop_str(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Or => "or",
        BinaryOp::And => "and",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
    }
}

/// The precedence level of an already-built expression, for the sole
/// purpose of deciding whether `fmt_operand` needs to wrap it in parens
/// when it appears as an operand of something tighter-binding. The AST
/// does not record whether the original source actually had parens there
/// (`parser.rs`'s `paren` production returns the inner expression as-is),
/// so parens are *recomputed* from precedence rather than preserved
/// verbatim — this is what keeps reformatting semantically round-trip
/// safe despite that information loss.
fn precedence(e: &Expr) -> u8 {
    match e {
        Expr::Binary { op, .. } => precedence_of_binop(*op),
        Expr::Unary { .. } => 6,
        Expr::FieldAccess { .. } | Expr::Call { .. } | Expr::Index { .. } => 7,
        _ => 9,
    }
}

/// Prints `se`, wrapping it in parens if its precedence is lower than
/// `required` (the minimum precedence the surrounding context needs to
/// stay unambiguous).
fn fmt_operand(se: &Spanned<Expr>, required: u8, indent: usize) -> String {
    let s = fmt_expr(&se.0, indent);
    if precedence(&se.0) < required {
        format!("({s})")
    } else {
        s
    }
}

fn fmt_args(args: &[Arg], indent: usize) -> String {
    args.iter()
        .map(|a| {
            let v = fmt_operand(&a.value, 0, indent);
            match &a.name {
                Some(n) => format!("{n}: {v}"),
                None => v,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn fmt_record_fields(fields: &[(String, Spanned<Expr>)], indent: usize) -> String {
    let inner = fields
        .iter()
        .map(|(k, v)| format!("{k}: {}", fmt_operand(v, 0, indent)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{inner}}}")
}

/// Ulexite float literals lex from `[0-9]+\.[0-9]+` (`lexer.rs`) — they
/// always need a fractional digit. Rust's `f64` `Display` drops a trailing
/// `.0` (`2.0` prints as `2`), which would re-lex as `Int`, so it's added
/// back explicitly when absent.
fn fmt_float(f: f64) -> String {
    let s = format!("{f}");
    if s.contains('.') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Re-escapes a string value back into a quoted `Str` token. `ulx-ast`
/// stores the *unescaped* value (`lexer.rs`'s `unescape`), so this does not
/// need to reproduce the original escaping choice byte-for-byte — only
/// produce *some* valid `Str` token that re-lexes back to the same value.
fn fmt_str_lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `TextBlock` content (triple-quoted) is captured raw by the lexer with no
/// unescaping (`lex_text_block`), so literal parts are re-emitted verbatim.
fn fmt_text_block(parts: &[TextPart], indent: usize) -> String {
    let mut s = String::from("\"\"\"");
    for part in parts {
        match part {
            TextPart::Literal(l) => s.push_str(l),
            TextPart::Interp(e) => {
                s.push('{');
                s.push_str(&fmt_operand(e, 0, indent));
                s.push('}');
            }
        }
    }
    s.push_str("\"\"\"");
    s
}

fn fmt_expr(e: &Expr, indent: usize) -> String {
    match e {
        Expr::Int(i) => i.to_string(),
        Expr::Float(f) => fmt_float(*f),
        Expr::Str(s) => fmt_str_lit(s),
        Expr::TextBlock(parts) => fmt_text_block(parts, indent),
        Expr::Ident(s) => s.clone(),
        Expr::RecordLit(fields) => fmt_record_fields(fields, indent),
        Expr::If {
            cond,
            then_block,
            else_block,
        } => format!(
            "if {} {} else {}",
            fmt_operand(cond, 0, indent),
            fmt_block(then_block, indent),
            fmt_block(else_block, indent),
        ),
        Expr::GenericCall { name, ty_arg, args } => {
            format!("{name}<{}>({})", fmt_type(&ty_arg.0), fmt_args(args, indent))
        }
        Expr::Retry {
            count,
            body,
            else_expr,
        } => {
            let mut s = format!("retry({count}) {}", fmt_block(body, indent));
            if let Some(e) = else_expr {
                s.push_str(" else ");
                s.push_str(&fmt_operand(e, 0, indent));
            }
            s
        }
        Expr::Escalate { target, args } => {
            if args.is_empty() {
                format!("escalate({target})")
            } else {
                let inner = args
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", fmt_operand(v, 0, indent)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("escalate({target}, {inner})")
            }
        }
        Expr::JudgeCall { name, args } => format!("judge {name}({})", fmt_args(args, indent)),
        Expr::ValidatorCall { name, args } => {
            format!("validator {name}({})", fmt_args(args, indent))
        }
        Expr::AskExpr {
            capability,
            args,
            body,
        } => format!(
            "ask {capability}({}) {}",
            fmt_args(args, indent),
            fmt_block(body, indent)
        ),
        Expr::RowRef => "$".to_string(),
        Expr::FieldAccess { base, field } => {
            format!("{}.{field}", fmt_operand(base, 7, indent))
        }
        Expr::Call { callee, args } => {
            format!("{}({})", fmt_operand(callee, 7, indent), fmt_args(args, indent))
        }
        Expr::Index { base, index } => format!(
            "{}[{}]",
            fmt_operand(base, 7, indent),
            fmt_operand(index, 0, indent)
        ),
        Expr::Unary { op, expr } => {
            let operand = fmt_operand(expr, 6, indent);
            match op {
                UnaryOp::Not => format!("not {operand}"),
                UnaryOp::Neg => format!("-{operand}"),
            }
        }
        Expr::Binary { op, lhs, rhs } => {
            let l = precedence_of_binop(*op);
            let lhs_s = fmt_operand(lhs, l, indent);
            let rhs_s = fmt_operand(rhs, l + 1, indent);
            format!("{lhs_s} {} {rhs_s}", binop_str(*op))
        }
    }
}
