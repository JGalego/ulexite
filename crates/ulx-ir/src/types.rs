use ulx_ast::{ArtifactType, BinaryOp, MessageRole, TypeExpr, UnaryOp};

#[derive(Debug, Clone, PartialEq)]
pub struct IrProgram {
    pub conversations: Vec<IrConversation>,
    pub judges: Vec<IrRubric>,
    pub validators: Vec<IrRubric>,
    pub datasets: Vec<IrDataset>,
    pub benchmarks: Vec<IrBenchmark>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrConversation {
    pub name: String,
    pub params: Vec<(String, TypeExpr)>,
    pub ret: Option<TypeExpr>,
    pub body: IrBlock,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrRubric {
    pub name: String,
    pub params: Vec<(String, TypeExpr)>,
    pub ret: TypeExpr,
    pub fields: Vec<(String, IrExpr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrDataset {
    pub name: String,
    pub ty: TypeExpr,
    pub source: IrDatasetSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrDatasetSource {
    FromFile(String),
    Rows(Vec<Vec<(String, IrExpr)>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrBenchmark {
    pub name: String,
    pub steps: Vec<IrBenchmarkStep>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrBenchmarkStep {
    Dataset(String),
    Run {
        expr: IrExpr,
        bind: String,
    },
    Expect {
        expr: IrExpr,
        judge: IrExpr,
        threshold: Option<f64>,
    },
    Assert(IrExpr),
    Snapshot {
        expr: IrExpr,
        key: IrExpr,
    },
}

/// An ordered sequence of instructions plus an optional trailing value —
/// the IR analogue of `ulx_ast::Block` (§8's `block` production).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IrBlock {
    pub insts: Vec<IrInst>,
    pub tail: Option<Box<IrExpr>>,
}

/// One instruction: optionally binds its result to a name (a `with`-block
/// member, a plain `ident = expr` binding, an `ask`/`assistant ->` result),
/// or is evaluated for effect alone.
#[derive(Debug, Clone, PartialEq)]
pub struct IrInst {
    pub bind: Option<String>,
    pub expr: IrExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrArg {
    pub name: Option<String>,
    pub value: IrExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrTextPart {
    Literal(String),
    Interp(IrExpr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct IrMatchArm {
    pub pattern: IrPattern,
    pub body: IrArmBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrPattern {
    Variant { name: String, bindings: Vec<String> },
    Wildcard,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrArmBody {
    Expr(IrExpr),
    Block(IrBlock),
}

/// The one place non-determinism enters the IR (§9.3, §13.4): every effect
/// is tagged with the capability/name it invokes, never a vendor.
#[derive(Debug, Clone, PartialEq)]
pub enum IrEffect {
    Ask {
        capability: String,
        args: Vec<IrArg>,
        /// The outgoing turns (§7.3's `system:`/`user:` sugar and §7.5's
        /// explicit `ask` block both desugar to this) — a message's *text*
        /// is data, not something to execute, so it's a plain `IrExpr`
        /// (almost always `TextBlock`), not a nested `IrBlock`.
        messages: Vec<(MessageRole, IrExpr)>,
    },
    Judge {
        name: String,
        args: Vec<IrArg>,
    },
    Validator {
        name: String,
        args: Vec<IrArg>,
    },
    Escalate {
        target: String,
        args: Vec<(String, IrExpr)>,
    },
    /// A call to another `conversation` — effectful because it may itself
    /// contain effects, even though from the caller's view it looks like
    /// an ordinary function call.
    ConversationCall {
        name: String,
        args: Vec<IrArg>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum IrExpr {
    Int(i64),
    Float(f64),
    Str(String),
    TextBlock(Vec<IrTextPart>),
    Var(String),
    RowRef,
    Record(Vec<(String, IrExpr)>),
    FieldAccess {
        base: Box<IrExpr>,
        field: String,
    },
    /// A call whose callee isn't statically known to be a declared
    /// conversation (e.g. a stdlib module function like `pdf.extract_text`)
    /// — left as an opaque call for the runtime to resolve dynamically.
    OpaqueCall {
        callee: Box<IrExpr>,
        args: Vec<IrArg>,
    },
    Index {
        base: Box<IrExpr>,
        index: Box<IrExpr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<IrExpr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<IrExpr>,
        rhs: Box<IrExpr>,
    },
    If {
        cond: Box<IrExpr>,
        then_block: IrBlock,
        else_block: IrBlock,
    },
    GenericCall {
        name: String,
        ty_arg: TypeExpr,
        args: Vec<IrArg>,
    },
    Retry {
        count: u64,
        body: IrBlock,
        else_expr: Option<Box<IrExpr>>,
    },
    Match {
        scrutinee: Box<IrExpr>,
        arms: Vec<IrMatchArm>,
    },
    For {
        var: String,
        iter: Box<IrExpr>,
        body: IrBlock,
    },
    While {
        cond: Box<IrExpr>,
        body: IrBlock,
    },
    Break(Option<Box<IrExpr>>),
    /// A `with` block (§7.4): its bindings are independent by construction
    /// (§9.7, enforced in `ulx-sema`) and safe for the runtime to schedule
    /// concurrently (§10.2).
    Parallel(Vec<(String, IrExpr)>),
    Effect(Box<IrEffect>),
}

pub fn artifact_default() -> ArtifactType {
    ArtifactType::Text
}
