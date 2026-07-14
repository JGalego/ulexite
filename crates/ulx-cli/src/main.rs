//! `ulx` — the Ulexite CLI (§20.12). Only `parse`/`check` exist so far; the
//! rest of §20's surface (`run`, `test`, `plan`, `debug`, ...) depends on
//! semantic analysis, the IR, and the runtime (§13, §12), none of which are
//! implemented yet — see docs/spec/25-future-directions.md.

use ariadne::{Color, Label, Report, ReportKind, Source};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ulx", about = "Ulexite language CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a .ulx file and report success or syntax errors.
    Parse { file: PathBuf },
    /// Alias for `parse` (§20.12's `ulx check`, ahead of semantic analysis).
    Check { file: PathBuf },
}

fn main() {
    let cli = Cli::parse();
    let file = match &cli.command {
        Command::Parse { file } | Command::Check { file } => file,
    };

    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read {}: {e}", file.display());
            std::process::exit(1);
        }
    };

    let name = file.display().to_string();

    match ulx_syntax::parse_source(&src) {
        Ok(program) => {
            println!(
                "OK: {} import(s), {} declaration(s)",
                program.imports.len(),
                program.decls.len()
            );
        }
        Err(errors) => {
            for e in &errors {
                report_error(&name, &src, e);
            }
            std::process::exit(1);
        }
    }
}

/// Renders a single parse error as an ariadne report pointing at the exact
/// source span (§20.1's static-analysis-friendliness starts here: even the
/// bare parser should point at *where*, not just *what*).
fn report_error(name: &str, src: &str, e: &ulx_syntax::Err) {
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

    let report = Report::build(ReportKind::Error, name, span.start)
        .with_message(message.clone())
        .with_label(
            Label::new((name, span))
                .with_message(message)
                .with_color(Color::Red),
        )
        .finish();

    let _ = report.print((name, Source::from(src)));
}
