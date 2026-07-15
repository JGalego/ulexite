//! Shared diagnostic rendering for both parse errors (`ulx_syntax::Err`)
//! and semantic diagnostics (`ulx_sema::Diagnostic`) — one `ariadne`-backed
//! renderer so `ulx parse`/`ulx check` look and feel consistent (§20.1).

use ariadne::{Color, Label, Report, ReportKind, Source};

pub fn report_parse_error(name: &str, src: &str, e: &ulx_syntax::Err) {
    let span = e.span();
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

    let message = if expected.is_empty() {
        format!("unexpected {found}")
    } else {
        format!("found {found} but expected one of: {}", expected.join(", "))
    };

    print_report(name, src, span, message, Color::Red);
}

pub fn report_diagnostic(name: &str, src: &str, d: &ulx_sema::Diagnostic) {
    let color = match d.severity {
        ulx_sema::Severity::Error => Color::Red,
        ulx_sema::Severity::Warning => Color::Yellow,
    };
    print_report(name, src, d.span.clone(), d.message.clone(), color);
}

/// Like `report_diagnostic`, but honors `d.source_file`: a diagnostic about
/// a `{var}` interpolation inside a `file("...")`/`@path`-loaded prompt
/// file (§8 `file_expr`) carries a span into *that* file's own content, not
/// `module_name`/`module_src` — render it against the right source instead
/// of the enclosing module's.
pub fn report_module_diagnostic(module_name: &str, module_src: &str, d: &ulx_sema::Diagnostic) {
    let Some(path) = &d.source_file else {
        report_diagnostic(module_name, module_src, d);
        return;
    };
    match std::fs::read_to_string(path) {
        Ok(src) => report_diagnostic(&path.display().to_string(), &src, d),
        Err(_) => eprintln!("{}: {}", path.display(), d.message),
    }
}

fn print_report(
    name: &str,
    src: &str,
    span: std::ops::Range<usize>,
    message: String,
    color: Color,
) {
    let span = if span.end > src.len() || span.start > span.end {
        0..src.len().min(1)
    } else {
        span
    };
    let report = Report::build(ReportKind::Error, name, span.start)
        .with_message(message.clone())
        .with_label(
            Label::new((name, span))
                .with_message(message)
                .with_color(color),
        )
        .finish();
    let _ = report.print((name, Source::from(src)));
}
