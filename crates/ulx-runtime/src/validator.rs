//! Deterministic validators (§5.7, §15.6): unlike a `judge`, these never
//! call a provider. `regex` is fully real (backed by the `regex` crate);
//! `json_schema` is checked for well-formedness only in v0.1 (real schema
//! conformance is future work — see `docs/spec/24-limitations.md`), which
//! is an honest, documented narrowing rather than a silent shortcut.

use crate::value::Verdict;

pub fn run_regex(pattern: &str, subject: &str) -> Verdict {
    match regex::Regex::new(pattern) {
        Ok(re) => {
            if re.is_match(subject) {
                Verdict::Pass
            } else {
                Verdict::Fail(format!("`{subject}` does not match /{pattern}/"))
            }
        }
        Err(e) => Verdict::Fail(format!("invalid regex `{pattern}`: {e}")),
    }
}

/// v0.1: checks the subject parses as JSON at all. A real `json_schema`
/// validator would check structural conformance against a named schema
/// (§9.2) — that's future work, not implemented here.
pub fn run_json_wellformed(subject: &str) -> Verdict {
    match serde_json::from_str::<serde_json::Value>(subject) {
        Ok(_) => Verdict::Pass,
        Err(e) => Verdict::Fail(format!("not valid JSON: {e}")),
    }
}
