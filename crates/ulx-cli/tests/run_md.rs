//! CLI-level end-to-end test for running a Markdown source (`ulx-cli::md`)
//! directly with `ulx run`/`ulx check`, instead of requiring a separate
//! `ulx from-md` compile step first: drives the actual built `ulx` binary
//! against `examples/write_haiku.md`, with `--mock` so no real provider
//! credentials are needed.

use std::path::PathBuf;
use std::process::Command;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

#[test]
fn check_accepts_a_markdown_source_directly() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let file = examples_dir()
        .join("write_haiku.md")
        .canonicalize()
        .expect("examples/write_haiku.md must exist");
    let tmp = tempfile::tempdir().expect("tempdir");

    let output = Command::new(exe)
        .arg("check")
        .arg(&file)
        .current_dir(tmp.path())
        .output()
        .expect("failed to run `ulx check`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "`ulx check` should accept a .md source directly\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn run_compiles_and_runs_a_markdown_source_directly() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let file = examples_dir()
        .join("write_haiku.md")
        .canonicalize()
        .expect("examples/write_haiku.md must exist");
    let tmp = tempfile::tempdir().expect("tempdir");

    let output = Command::new(exe)
        .arg("run")
        .arg(&file)
        .arg("WriteHaiku")
        .arg("--arg")
        .arg("theme=autumn")
        .arg("--mock")
        .current_dir(tmp.path())
        .output()
        .expect("failed to run `ulx run`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "`ulx run` should compile and run a .md source without a separate `ulx from-md` step\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("autumn"),
        "expected the {{theme}} placeholder to have been interpolated, got:\n{stdout}"
    );

    // Running it never leaves generated .ulx source outside `.ulexite/` —
    // the whole point is that a plain `.md` file behaves as if it were
    // `.ulx` all along, with nothing extra to see (or accidentally commit)
    // next to it.
    assert!(
        !file.with_extension("ulx").exists(),
        "run should not have written a sibling .ulx file next to the .md source"
    );
}

#[test]
fn check_reports_a_clear_error_for_invalid_markdown() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("broken.md");
    std::fs::write(&file, "just a paragraph, no heading\n").unwrap();

    let output = Command::new(exe)
        .arg("check")
        .arg(&file)
        .current_dir(tmp.path())
        .output()
        .expect("failed to run `ulx check`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "`ulx check` should fail for Markdown missing a title heading\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("Title")
            || stderr.contains("Title")
            || stdout.contains("heading")
            || stderr.contains("heading"),
        "expected a clear missing-title diagnostic, got:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
