//! CLI-level end-to-end test for `ulx eval calibrate` (§17.1): drives the
//! actual built `ulx` binary against a real judge and labeled dataset,
//! with `--mock` so it needs no real provider credentials.

use std::path::PathBuf;
use std::process::Command;

fn write_fixture(tmp: &std::path::Path) -> PathBuf {
    let file = tmp.join("calibrate.ulx");
    std::fs::write(
        &file,
        r#"
        judge Fluency(subject: text) -> Verdict {
          rubric: """Is this fluent?"""
        }

        dataset Labels: [{subject: text, human_pass: bool}] {
          from "labels.jsonl"
        }
        "#,
    )
    .unwrap();
    file
}

#[test]
fn eval_calibrate_reports_real_agreement_and_fails_below_threshold() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = write_fixture(tmp.path());
    // 2 of 3 rows agree: the mock judge fails only the MOCK_JUDGE_FAIL row,
    // but the human labeled every row `true` — a genuine, known-in-advance
    // 66.7% agreement rate, below the default 80% threshold.
    std::fs::write(
        tmp.path().join("labels.jsonl"),
        "{\"subject\": \"Good morning, how are you?\", \"human_pass\": true}\n\
         {\"subject\": \"MOCK_JUDGE_FAIL this one\", \"human_pass\": true}\n\
         {\"subject\": \"another normal sentence\", \"human_pass\": true}\n",
    )
    .unwrap();

    let output = Command::new(exe)
        .arg("eval")
        .arg("calibrate")
        .arg(&file)
        .arg("Labels")
        .arg("Fluency")
        .arg("--mock")
        .current_dir(tmp.path())
        .output()
        .expect("failed to run `ulx eval calibrate`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected a nonzero exit for below-threshold agreement\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("2/3 agree"),
        "expected the real 2/3 agreement count, got:\n{stdout}"
    );
    assert!(
        stdout.contains("DISAGREE"),
        "expected the MOCK_JUDGE_FAIL row flagged as a disagreement, got:\n{stdout}"
    );
    assert!(
        stdout.contains("FAIL threshold"),
        "expected an explicit threshold failure line, got:\n{stdout}"
    );
}

#[test]
fn eval_calibrate_passes_when_every_row_agrees() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = write_fixture(tmp.path());
    std::fs::write(
        tmp.path().join("labels.jsonl"),
        "{\"subject\": \"Good morning, how are you?\", \"human_pass\": true}\n\
         {\"subject\": \"another normal sentence\", \"human_pass\": true}\n",
    )
    .unwrap();

    let output = Command::new(exe)
        .arg("eval")
        .arg("calibrate")
        .arg(&file)
        .arg("Labels")
        .arg("Fluency")
        .arg("--mock")
        .current_dir(tmp.path())
        .output()
        .expect("failed to run `ulx eval calibrate`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected a zero exit for 100% agreement\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("2/2 agree"), "got:\n{stdout}");
    assert!(stdout.contains("PASS threshold"), "got:\n{stdout}");
}

#[test]
fn eval_calibrate_reports_a_clear_error_for_an_unknown_judge() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = write_fixture(tmp.path());
    std::fs::write(tmp.path().join("labels.jsonl"), "").unwrap();

    let output = Command::new(exe)
        .arg("eval")
        .arg("calibrate")
        .arg(&file)
        .arg("Labels")
        .arg("NotAJudge")
        .arg("--mock")
        .current_dir(tmp.path())
        .output()
        .expect("failed to run `ulx eval calibrate`");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("NotAJudge"),
        "expected the unknown judge name in the error, got:\n{stderr}"
    );
}
