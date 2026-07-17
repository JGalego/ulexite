//! CLI-level end-to-end test for `ulx debug` (§19): drives the actual
//! built `ulx` binary, first producing a real trace via `ulx run` against
//! nested conversations (so the call-stack navigation this stepper is for
//! has something real to show), then piping a script of debugger commands
//! into `ulx debug <run_id>`'s stdin and asserting on its stdout.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn write_nested_fixture(tmp: &std::path::Path) -> PathBuf {
    let file = tmp.join("nested.ulx");
    std::fs::write(
        &file,
        r#"
        conversation Leaf(x: text) -> text {
          x
        }

        conversation Middle(x: text) -> text {
          Leaf(x: x)
        }

        conversation Root(x: text) -> text {
          Middle(x: x)
        }
        "#,
    )
    .unwrap();
    file
}

fn run_debug_script(exe: &str, tmp: &std::path::Path, run_id: &str, script: &str) -> String {
    let mut child = Command::new(exe)
        .arg("debug")
        .arg(run_id)
        .current_dir(tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn `ulx debug`");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("ulx debug did not exit");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "`ulx debug` should exit cleanly on `quit`\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

#[test]
fn debug_steps_through_a_real_nested_conversation_trace() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = write_nested_fixture(tmp.path());

    let run = Command::new(exe)
        .arg("run")
        .arg(&file)
        .arg("Root")
        .arg("--arg")
        .arg("x=hi")
        .arg("--mock")
        .arg("--run-id")
        .arg("nested_debug")
        .current_dir(tmp.path())
        .output()
        .expect("failed to run `ulx run`");
    assert!(
        run.status.success(),
        "`ulx run` should succeed against the mock provider"
    );

    let stdout = run_debug_script(
        exe,
        tmp.path(),
        "nested_debug",
        "next\nstack\nnext\nstack\nnext\nlist\nquit\n",
    );

    assert!(
        stdout.contains("2 record(s)"),
        "expected the startup banner to report both `call` records, got:\n{stdout}"
    );
    // First `next` lands on Middle's call record; its stack is just itself.
    assert!(stdout.contains("Middle (current)"), "got:\n{stdout}");
    // Second `next` lands on Leaf's call record, nested one level under Middle.
    assert!(stdout.contains("Middle > Leaf (current)"), "got:\n{stdout}");
    // Third `next` is past the end of a 2-record trace.
    assert!(stdout.contains("(end of trace)"), "got:\n{stdout}");
    assert!(
        stdout.contains("#0"),
        "list should show record #0, got:\n{stdout}"
    );
    assert!(
        stdout.contains("#1"),
        "list should show record #1, got:\n{stdout}"
    );
}

#[test]
fn debug_reports_a_suspended_run_with_a_resume_hint() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("escalate.ulx");
    std::fs::write(
        &file,
        r#"
        conversation NeedsApproval(x: text) -> text {
          escalate(human_approval, reason: "needs a human decision")
        }
        "#,
    )
    .unwrap();

    let run = Command::new(exe)
        .arg("run")
        .arg(&file)
        .arg("NeedsApproval")
        .arg("--arg")
        .arg("x=hi")
        .arg("--mock")
        .arg("--run-id")
        .arg("suspended_debug")
        .current_dir(tmp.path())
        .output()
        .expect("failed to run `ulx run`");
    // A suspended run is a nonzero exit by design (see main.rs's execute()).
    assert!(!run.status.success());

    let stdout = run_debug_script(exe, tmp.path(), "suspended_debug", "inspect\nquit\n");
    assert!(
        stdout.contains("SUSPENDED") && stdout.contains("human_approval"),
        "expected a suspend banner naming the target, got:\n{stdout}"
    );
    assert!(
        stdout.contains("ulx approve suspended_debug")
            && stdout.contains("ulx deny suspended_debug"),
        "expected a concrete resume hint, got:\n{stdout}"
    );
    assert!(
        stdout.contains("needs a human decision"),
        "expected `inspect` to show the escalate reason, got:\n{stdout}"
    );
}

#[test]
fn debug_reports_a_clear_error_for_an_unknown_run_id() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let tmp = tempfile::tempdir().expect("tempdir");

    let output = Command::new(exe)
        .arg("debug")
        .arg("does-not-exist")
        .current_dir(tmp.path())
        .stdin(Stdio::piped())
        .output()
        .expect("failed to run `ulx debug`");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not read trace"),
        "expected a clear error, got:\n{stderr}"
    );
}
