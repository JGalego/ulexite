//! Shared "judge via chat" helper (§12.4, §15.2): evaluating a rubric is,
//! mechanically, just another chat completion — build a rubric-evaluation
//! prompt from the judge call's `subject`/`rubric` args, hand it to the
//! vendor's already-implemented `chat` path, then parse the reply into a
//! `Verdict`. Every real vendor adapter's `"judge"` match arm delegates
//! here so the prompt format and parsing logic exist in exactly one place
//! rather than being reimplemented per vendor.

use crate::value::{Value, Verdict};

use super::{Invocation, Message, ProviderError};

const JUDGE_SYSTEM_PROMPT: &str = "You are an evaluator judging whether a subject satisfies a rubric. \
Respond with exactly one line and no other text:\n\
- `PASS` if the subject satisfies the rubric.\n\
- `FAIL: <reason>` if it does not, with a short reason.\n\
- `SCORE: <n>` where <n> is a number between 0.0 and 1.0, only if the rubric itself asks for a numeric score rather than a pass/fail judgment.\n\
- `ESCALATE` if you genuinely cannot tell from the rubric and subject given.";

fn build_prompt(request: &Invocation) -> Invocation {
    let rubric = request
        .args
        .get("rubric")
        .and_then(Value::as_text)
        .unwrap_or_default();
    let subject = request
        .args
        .get("subject")
        .and_then(Value::as_text)
        .unwrap_or_default();
    Invocation {
        messages: vec![
            Message {
                role: "system".to_string(),
                text: JUDGE_SYSTEM_PROMPT.to_string(),
            },
            Message {
                role: "user".to_string(),
                text: format!("Rubric: {rubric}\n\nSubject:\n{subject}"),
            },
        ],
        args: Default::default(),
    }
}

/// Parses a judge model's reply into a `Verdict`. Anything that doesn't
/// match the `PASS`/`FAIL: reason`/`SCORE: n`/`ESCALATE` shape asked for in
/// `JUDGE_SYSTEM_PROMPT` is treated as `Escalate` — the same "cannot tell"
/// semantics the rubric itself uses — rather than silently guessing.
fn parse_verdict(text: &str) -> Verdict {
    let trimmed = text.trim();

    if trimmed.eq_ignore_ascii_case("pass") {
        return Verdict::Pass;
    }
    if trimmed.eq_ignore_ascii_case("escalate") {
        return Verdict::Escalate;
    }
    if trimmed
        .get(..4)
        .is_some_and(|s| s.eq_ignore_ascii_case("fail"))
    {
        let reason = trimmed[4..].trim_start_matches(':').trim();
        return Verdict::Fail(if reason.is_empty() {
            "no reason given".to_string()
        } else {
            reason.to_string()
        });
    }
    if trimmed
        .get(..5)
        .is_some_and(|s| s.eq_ignore_ascii_case("score"))
    {
        let rest = trimmed[5..].trim_start_matches(':').trim();
        if let Ok(score) = rest.parse::<f64>() {
            return Verdict::Score(score);
        }
    }

    Verdict::Escalate
}

/// Builds the rubric-evaluation prompt, invokes `chat` (the caller's own
/// vendor-specific chat method — the `judge` capability shares the same
/// underlying chat request shape, just a different model per
/// `ulexite.toml`'s `[providers.*].judge` entry), and parses the reply.
pub(crate) fn judge_via_chat(
    request: &Invocation,
    chat: impl FnOnce(&Invocation) -> Result<Value, ProviderError>,
) -> Result<Value, ProviderError> {
    let prompt = build_prompt(request);
    let response = chat(&prompt)?;
    let text = response.as_text().unwrap_or_default();
    Ok(Value::Verdict(parse_verdict(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pass_case_insensitively() {
        assert_eq!(parse_verdict("pass"), Verdict::Pass);
        assert_eq!(parse_verdict("PASS"), Verdict::Pass);
        assert_eq!(parse_verdict("  Pass  "), Verdict::Pass);
    }

    #[test]
    fn parses_fail_with_reason() {
        assert_eq!(
            parse_verdict("FAIL: too literal a translation"),
            Verdict::Fail("too literal a translation".to_string())
        );
        assert_eq!(
            parse_verdict("fail:missing tone"),
            Verdict::Fail("missing tone".to_string())
        );
    }

    #[test]
    fn fail_with_no_reason_gets_a_placeholder() {
        assert_eq!(
            parse_verdict("FAIL"),
            Verdict::Fail("no reason given".to_string())
        );
    }

    #[test]
    fn parses_score() {
        assert_eq!(parse_verdict("SCORE: 0.75"), Verdict::Score(0.75));
        assert_eq!(parse_verdict("score:1"), Verdict::Score(1.0));
    }

    #[test]
    fn parses_escalate() {
        assert_eq!(parse_verdict("ESCALATE"), Verdict::Escalate);
    }

    #[test]
    fn unparseable_reply_escalates_rather_than_guessing() {
        assert_eq!(
            parse_verdict("I'm not sure, this is complicated"),
            Verdict::Escalate
        );
        assert_eq!(parse_verdict("SCORE: not-a-number"), Verdict::Escalate);
        assert_eq!(parse_verdict(""), Verdict::Escalate);
    }

    #[test]
    fn judge_via_chat_wraps_chat_response_into_a_verdict() {
        let request = Invocation {
            messages: vec![],
            args: std::collections::BTreeMap::from([
                ("subject".to_string(), Value::Text("le chat".to_string())),
                (
                    "rubric".to_string(),
                    Value::Text("Is this French for cat?".to_string()),
                ),
            ]),
        };
        let result = judge_via_chat(&request, |_req| Ok(Value::Text("PASS".to_string())));
        assert_eq!(result.unwrap(), Value::Verdict(Verdict::Pass));
    }
}
