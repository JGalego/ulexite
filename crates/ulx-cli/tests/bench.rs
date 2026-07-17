//! CLI-level end-to-end test for `ulx bench` (§16): drives the actual
//! built `ulx` binary (via `CARGO_BIN_EXE_ulx`, the standard way to
//! integration-test a `[[bin]]` target) against `examples/eval_translate.ulx`
//! — a real cross-file `benchmark` declaration (its `run:` calls
//! `Translate`, imported from `translate.ulx`) — with `--mock` so it needs
//! no real provider credentials. The child process's cwd is a throwaway
//! temp dir so its `.ulexite/` cache/trace state (see `manifest.rs`) never
//! touches the repo — but `snapshot`'s golden-baseline files live beside
//! the source file itself (`examples/snapshots/TranslateQuality/`), not
//! under cwd, so this test exercises a real comparison against the
//! baseline already committed there (produced by this exact mock run),
//! not just a first-ever "recorded" pass.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn bench_command_runs_eval_translate_example_end_to_end() {
    let exe = env!("CARGO_BIN_EXE_ulx");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let file = manifest_dir
        .join("../../examples/eval_translate.ulx")
        .canonicalize()
        .expect("examples/eval_translate.ulx must exist");

    let tmp = tempfile::tempdir().expect("tempdir");

    let output = Command::new(exe)
        .arg("bench")
        .arg(&file)
        .arg("TranslateQuality")
        .arg("--mock")
        .current_dir(tmp.path())
        .output()
        .expect("failed to run `ulx bench`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "`ulx bench` should succeed against the mock provider\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("row 0:"),
        "expected a per-row report line, got:\n{stdout}"
    );
    assert!(
        stdout.contains("TranslateQuality:"),
        "expected a named summary line, got:\n{stdout}"
    );
    // `examples/fixtures/translations.jsonl` has 3 rows, none of which
    // trigger the mock provider's `MOCK_JUDGE_FAIL`/`MOCK_JUDGE_ESCALATE`
    // markers, so every row's `expect`/`assert` should pass.
    assert!(
        stdout.contains("3/3 row(s) passed"),
        "expected all 3 fixture rows to pass against the mock provider, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("FAIL"),
        "did not expect any failing row, got:\n{stdout}"
    );
}
