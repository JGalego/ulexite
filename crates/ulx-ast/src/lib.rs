//! AST node definitions for Ulexite, mirroring the grammar in
//! `docs/spec/08-grammar.md`. Every recursive node carries a source span so
//! downstream stages (semantic analysis, §9; incremental recompilation,
//! §13.7) can report and diff against exact source locations.

pub type Span = std::ops::Range<usize>;

/// A node paired with the source span it was parsed from.
pub type Spanned<T> = (T, Span);

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub imports: Vec<Spanned<Import>>,
    pub decls: Vec<Spanned<TopDecl>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Import {
    /// `import judge Fluency from "translate.ulx"` (§7.7)
    Named {
        kind: ImportKind,
        name: String,
        from: String,
    },
    /// `import "vector" as vector` (§15)
    Module { path: String, alias: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Conversation,
    Judge,
    Validator,
    Dataset,
    Type,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TopDecl {
    Conversation(ConversationDecl),
    Judge(RubricDecl),
    Validator(RubricDecl),
    Dataset(DatasetDecl),
    Type(TypeDecl),
    Benchmark(BenchmarkDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationDecl {
    pub doc: Option<String>,
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<Spanned<TypeExpr>>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
}

/// Shared shape of `judge` and `validator` declarations (§7.2): both are
/// `name(params) -> ret { field: expr ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct RubricDecl {
    pub doc: Option<String>,
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Spanned<TypeExpr>,
    pub fields: Vec<(String, Spanned<Expr>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DatasetDecl {
    pub doc: Option<String>,
    pub name: String,
    pub ty: Spanned<TypeExpr>,
    pub source: DatasetSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DatasetSource {
    FromFile(String),
    Rows(Vec<Vec<(String, Spanned<Expr>)>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkDecl {
    pub doc: Option<String>,
    pub name: String,
    pub stmts: Vec<Spanned<BenchmarkStmt>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BenchmarkStmt {
    Dataset(String),
    Run {
        expr: Spanned<Expr>,
        bind: String,
    },
    Expect {
        expr: Spanned<Expr>,
        judge: Spanned<Expr>,
        threshold: Option<f64>,
    },
    Assert(Spanned<Expr>),
    Snapshot {
        expr: Spanned<Expr>,
        key: Spanned<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    pub stmts: Vec<Spanned<Stmt>>,
    /// The optional trailing expression that is the block's value (§8, `block`).
    pub tail: Option<Box<Spanned<Expr>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `system: """..."""` / `user: """..."""` (§7.3)
    Message {
        role: MessageRole,
        text: Spanned<Expr>,
    },
    /// `assistant -> name: Type` (§7.3)
    AssistantBind {
        name: String,
        ty: Option<Spanned<TypeExpr>>,
    },
    With(Vec<Binding>),
    /// `ask capability(...) { ... } -> name: Type` (§7.5)
    Ask {
        capability: String,
        args: Vec<Arg>,
        body: Block,
        bind_name: String,
        bind_ty: Option<Spanned<TypeExpr>>,
    },
    Binding(Binding),
    Match(MatchStmt),
    For {
        var: String,
        iter: Spanned<Expr>,
        body: Block,
    },
    While {
        cond: Spanned<Expr>,
        body: Block,
    },
    Break(Option<Spanned<Expr>>),
    Expr(Spanned<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub name: String,
    pub value: Spanned<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchStmt {
    pub scrutinee: Box<Spanned<Expr>>,
    pub arms: Vec<MatchArm>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: MatchArmBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchArmBody {
    Expr(Spanned<Expr>),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// `Pass`, `Fail(reason)`, `Score(_)` (§8)
    Variant { name: String, bindings: Vec<String> },
    Wildcard,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Spanned<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextPart {
    Literal(String),
    Interp(Spanned<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Str(String),
    TextBlock(Vec<TextPart>),
    Ident(String),
    RecordLit(Vec<(String, Spanned<Expr>)>),
    /// `if cond { .. } else { .. }` (§8 `if_expr`)
    If {
        cond: Box<Spanned<Expr>>,
        then_block: Block,
        else_block: Block,
    },
    /// `list<text>()` (§8 `generic_call`)
    GenericCall {
        name: String,
        ty_arg: Spanned<TypeExpr>,
        args: Vec<Arg>,
    },
    /// `retry(n) { .. } else expr` (§8 `retry_expr`)
    Retry {
        count: u64,
        body: Block,
        else_expr: Option<Box<Spanned<Expr>>>,
    },
    /// `escalate(target, k: v, ...)` (§8 `escalate_expr`)
    Escalate {
        target: String,
        args: Vec<(String, Spanned<Expr>)>,
    },
    JudgeCall {
        name: String,
        args: Vec<Arg>,
    },
    ValidatorCall {
        name: String,
        args: Vec<Arg>,
    },
    AskExpr {
        capability: String,
        args: Vec<Arg>,
        body: Block,
    },
    /// `$` — current dataset row inside a benchmark (§8 `row_ref`, §16.2)
    RowRef,
    FieldAccess {
        base: Box<Spanned<Expr>>,
        field: String,
    },
    Call {
        callee: Box<Spanned<Expr>>,
        args: Vec<Arg>,
    },
    Index {
        base: Box<Spanned<Expr>>,
        index: Box<Spanned<Expr>>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Spanned<Expr>>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Spanned<Expr>>,
        rhs: Box<Spanned<Expr>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Artifact(ArtifactType),
    Record(Vec<(String, Spanned<TypeExpr>)>),
    Union(Vec<Variant>),
    /// `Draft<text>`, `dataset<Row>`, or a const-generic dimension like
    /// `embedding<1536>` (§11.4, §8 `generic_type`/`generic_arg`).
    Generic {
        name: String,
        arg: GenericArg,
    },
    Array(Box<Spanned<TypeExpr>>),
    Named(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum GenericArg {
    Type(Box<Spanned<TypeExpr>>),
    Const(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub name: String,
    pub payload: Option<Box<Spanned<TypeExpr>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactType {
    Text,
    Markdown,
    Image,
    Audio,
    Video,
    Pdf,
    Json,
    Xml,
    Html,
    Csv,
    Embedding,
    Vector,
    ToolOutput,
}

impl ArtifactType {
    /// The fourteen closed artifact types of §9.2 (as keywords, thirteen are
    /// distinct identifiers here since `embedding`/`vector` are listed once
    /// each per §8's `artifact_type` production).
    pub fn from_keyword(s: &str) -> Option<Self> {
        Some(match s {
            "text" => Self::Text,
            "markdown" => Self::Markdown,
            "image" => Self::Image,
            "audio" => Self::Audio,
            "video" => Self::Video,
            "pdf" => Self::Pdf,
            "json" => Self::Json,
            "xml" => Self::Xml,
            "html" => Self::Html,
            "csv" => Self::Csv,
            "embedding" => Self::Embedding,
            "vector" => Self::Vector,
            "tool_output" => Self::ToolOutput,
            _ => return None,
        })
    }
}
