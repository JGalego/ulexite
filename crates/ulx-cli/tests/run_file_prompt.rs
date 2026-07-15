//! CLI-level end-to-end test for `file("...")`/`@path` prompt loading (§8
//! `file_expr`): drives the actual built `ulx` binary against
//! `examples/prompt_from_file.ulx`, and against a throwaway broken fixture,
//! with `--mock` so no real provider credentials are needed.

use std::path::PathBuf;
use std::process::Command;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

#[test]
fn check_accepts_the_file_prompt_example() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let file = examples_dir()
        .join("prompt_from_file.ulx")
        .canonicalize()
        .expect("examples/prompt_from_file.ulx must exist");
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
        "`ulx check` should accept file(...)/@path prompts\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn run_interpolates_prompt_text_loaded_from_disk() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let file = examples_dir()
        .join("prompt_from_file.ulx")
        .canonicalize()
        .expect("examples/prompt_from_file.ulx must exist");
    let tmp = tempfile::tempdir().expect("tempdir");

    let output = Command::new(exe)
        .arg("run")
        .arg(&file)
        .arg("Greet")
        .arg("--arg")
        .arg("name=Ana")
        .arg("--arg")
        .arg("occasion=Graduation")
        .arg("--mock")
        .current_dir(tmp.path())
        .output()
        .expect("failed to run `ulx run`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "`ulx run` should succeed against the mock provider\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Both the `file("...")`-loaded system prompt and the `@path`-loaded
    // user prompt must have been read, split, interpolated, and reached the
    // mock provider — the same guarantee an inline `"""..."""` block gives.
    assert!(
        stdout.contains("greeting writer"),
        "expected the file(\"...\")-loaded system prompt's content, got:\n{stdout}"
    );
    assert!(
        stdout.contains("Ana") && stdout.contains("Graduation"),
        "expected the @path-loaded user prompt's interpolated {{name}}/{{occasion}}, got:\n{stdout}"
    );
}

#[test]
fn check_reports_a_clear_error_for_a_missing_prompt_file() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("broken.ulx");
    std::fs::write(
        &file,
        r#"
        conversation Greet(name: text) -> text {
          system: file("does_not_exist.txt")
          user: """Hi {name}"""
          assistant -> reply: text
          reply
        }
        "#,
    )
    .unwrap();

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
        "`ulx check` should fail for a missing prompt file\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("could not read prompt file") || stderr.contains("could not read prompt file"),
        "expected a clear missing-prompt-file diagnostic, got:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
