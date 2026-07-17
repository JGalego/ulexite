//! `ulx eval calibrate` (§17.1's "a judge shouldn't be trusted by
//! default"): runs a judge against a human-labeled dataset and reports how
//! often the judge's own pass/fail verdict agrees with the human's.
//!
//! Scoped deliberately narrower than §17.1's `CalibrateFluencyJudge`
//! example, which types a dataset row as `{subject: text, human_verdict:
//! Verdict}` and compares full `Verdict` values. `dataset::load`'s JSONL
//! loader is untyped — it has no notion of a dataset's declared row type
//! at load time (see its `from_json`), so a `"Pass"` string in a JSONL
//! file loads as `Value::Text("Pass")`, never `Value::Verdict(Pass)` —
//! there is no dataset-level type coercion to lean on. Rather than bolt
//! one on just for this feature, calibration here works against a simpler,
//! honest row shape: `{subject: <any>, human_pass: bool}`, where
//! `human_pass` is the human's binary judgment (did this response pass
//! the bar) — comparable directly against the judge's own verdict reduced
//! to a boolean via the exact same pass/fail rule `expect` uses
//! (`evaluate_expect_verdict`): `Pass` -> true, `Fail` -> false, `Score(s)`
//! -> `s` against a threshold, `Escalate` -> false. This is calibration's
//! actual real-world question — "does the automated judge agree with the
//! human's yes/no" — without requiring a `Verdict`-typed dataset the
//! runtime can't currently load.

use ulx_ir::IrDataset;

use crate::error::RuntimeError;
use crate::interp::{evaluate_expect_verdict, invoke_judge_with_subject};
use crate::value::Value;
use crate::RunContext;

#[derive(Debug, Clone)]
pub struct CalibrationRow {
    pub row_index: usize,
    pub human_pass: bool,
    pub judge_verdict: Value,
    pub judge_pass: bool,
}

impl CalibrationRow {
    pub fn agrees(&self) -> bool {
        self.human_pass == self.judge_pass
    }
}

#[derive(Debug, Clone)]
pub struct CalibrationReport {
    pub judge_name: String,
    pub dataset_name: String,
    pub rows: Vec<CalibrationRow>,
}

impl CalibrationReport {
    /// `1.0` for an empty dataset (vacuously perfect agreement) rather
    /// than `NaN`/a divide-by-zero — an empty dataset is a configuration
    /// problem the caller should catch some other way (e.g. `rows.is_empty()`),
    /// not something this rate should silently encode as a failure.
    pub fn agreement_rate(&self) -> f64 {
        if self.rows.is_empty() {
            return 1.0;
        }
        let agreeing = self.rows.iter().filter(|r| r.agrees()).count();
        agreeing as f64 / self.rows.len() as f64
    }

    pub fn passes_threshold(&self, threshold: f64) -> bool {
        self.agreement_rate() >= threshold
    }

    pub fn disagreements(&self) -> impl Iterator<Item = &CalibrationRow> {
        self.rows.iter().filter(|r| !r.agrees())
    }
}

/// Runs `judge_name` against every row of `dataset_name`, reporting
/// per-row agreement with each row's `human_pass` label. `score_threshold`
/// is only consulted for a `Score(s)` verdict (mirrors `expect ...
/// satisfies judge ... with threshold(...)`) — `None` uses `evaluate_expect_verdict`'s
/// own default (`s > 0.0`).
pub fn run_calibration(
    ctx: &RunContext,
    dataset_name: &str,
    judge_name: &str,
    score_threshold: Option<f64>,
) -> Result<CalibrationReport, RuntimeError> {
    if !ctx.program.judges.iter().any(|j| j.name == judge_name) {
        return Err(RuntimeError::UnknownJudgeOrValidator(
            judge_name.to_string(),
        ));
    }
    let dataset: &IrDataset = ctx
        .program
        .datasets
        .iter()
        .find(|d| d.name == dataset_name)
        .ok_or_else(|| RuntimeError::UnknownDataset(dataset_name.to_string()))?;

    let rows = match crate::dataset::load(ctx, dataset)? {
        Value::List(rows) => rows,
        other => {
            return Err(RuntimeError::TypeError(format!(
                "dataset `{dataset_name}` did not load as a list of rows (got {other})"
            )))
        }
    };

    let mut report = CalibrationReport {
        judge_name: judge_name.to_string(),
        dataset_name: dataset_name.to_string(),
        rows: Vec::with_capacity(rows.len()),
    };

    for (row_index, row) in rows.into_iter().enumerate() {
        let Value::Record(fields) = &row else {
            return Err(RuntimeError::TypeError(format!(
                "dataset `{dataset_name}` row {row_index} is not a record (got {row})"
            )));
        };
        let subject = fields.get("subject").cloned().ok_or_else(|| {
            RuntimeError::TypeError(format!(
                "dataset `{dataset_name}` row {row_index} has no `subject` field"
            ))
        })?;
        let human_pass = match fields.get("human_pass") {
            Some(Value::Bool(b)) => *b,
            Some(other) => {
                return Err(RuntimeError::TypeError(format!(
                    "dataset `{dataset_name}` row {row_index}'s `human_pass` must be a bool, got {other}"
                )))
            }
            None => {
                return Err(RuntimeError::TypeError(format!(
                    "dataset `{dataset_name}` row {row_index} has no `human_pass` field"
                )))
            }
        };

        let judge_verdict = invoke_judge_with_subject(ctx, judge_name, subject)?;
        let (judge_pass, _) = evaluate_expect_verdict(&judge_verdict, score_threshold);

        report.rows.push(CalibrationRow {
            row_index,
            human_pass,
            judge_verdict,
            judge_pass,
        });
    }

    Ok(report)
}
