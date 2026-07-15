use std::path::PathBuf;

use ulx_ast::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    /// `None` means "render against the module's own source" (the usual
    /// case). `Some(path)` means `span` is a byte range into `path`'s own
    /// content instead — used for diagnostics about `{var}` interpolations
    /// inside a `file("...")`/`@path`-loaded prompt file (§8 `file_expr`),
    /// whose spans come from re-lexing *that file's* content, not the
    /// importing module's.
    pub source_file: Option<PathBuf>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            span,
            source_file: None,
        }
    }

    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
            span,
            source_file: None,
        }
    }

    /// Marks this diagnostic's `span` as belonging to `path` rather than the
    /// enclosing module's own file.
    pub fn in_file(mut self, path: PathBuf) -> Self {
        self.source_file = Some(path);
        self
    }
}
