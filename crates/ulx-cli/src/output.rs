//! `--output` rendering. Not part of the language spec — a CLI-only
//! presentation layer over two things the runtime already produces: a
//! single `RunOutcome` (from `run`/`approve`/`deny`/`replay`) and a
//! `Vec<TraceRecord>` (from `trace`, and reused by the other commands for
//! the `jsonl`/`mermaid`/`html` formats, which need the whole trace rather
//! than just the final value).
//!
//! `Text` is the pre-existing default and is deliberately handled entirely
//! in `main.rs`, verbatim as it was before this module existed — every
//! format here is additive and never touches that path.

use serde_json::{json, Value as Json};

use ulx_runtime::value::{DraftOutcome, Verdict};
use ulx_runtime::{TraceRecord, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
    Jsonl,
    Mermaid,
    Html,
}

/// The result of one `run_conversation` call, format-agnostic. `Text`
/// rendering of this never goes through here — see the module docs. Every
/// variant carries `run_id` (even `Error`, whose trace up to the failure
/// point may still be worth inspecting) so a script consuming `Json` can
/// always chain into `ulx trace <run_id>` without having had to pass
/// `--run-id` explicitly up front.
pub enum RunOutcome<'a> {
    Value {
        run_id: &'a str,
        value: &'a Value,
    },
    Suspended {
        run_id: &'a str,
        reason: &'a str,
        target: &'a str,
    },
    Error {
        run_id: &'a str,
        message: String,
    },
}

/// `Json`-renders a `RunOutcome` as one line of structured output,
/// uniformly on stdout regardless of which of the three shapes it is —
/// unlike `Text` mode, which splits success/suspended (stdout) from error
/// (stderr). That split is a deliberate, documented difference: `Json`
/// mode is for scripts that want one parseable shape on one stream.
pub fn render_run_json(outcome: &RunOutcome) -> String {
    let doc = match outcome {
        RunOutcome::Value { run_id, value } => json!({
            "status": "ok",
            "run_id": run_id,
            "value": value_to_json(value),
        }),
        RunOutcome::Suspended {
            run_id,
            reason,
            target,
        } => json!({
            "status": "suspended",
            "run_id": run_id,
            "reason": reason,
            "target": target,
            "resume_hint": format!("ulx approve {run_id} --value <text>   (or: ulx deny {run_id})"),
        }),
        RunOutcome::Error { run_id, message } => json!({
            "status": "error",
            "run_id": run_id,
            "message": message,
        }),
    };
    serde_json::to_string(&doc).expect("json! output always serializes")
}

/// Untagged, "natural" JSON for a `Value` — deliberately not
/// `serde_json::to_value(v)`, which would surface `Value`'s internal
/// `{"kind": ..., "value": ...}` tagging (the shape used to round-trip
/// through the cache and trace files). `--output json`/`jsonl` is meant to
/// be pleasant to consume with `jq`, not a dump of the runtime's on-disk
/// representation.
fn value_to_json(v: &Value) -> Json {
    match v {
        Value::Text(s) => json!(s),
        Value::Int(i) => json!(i),
        Value::Float(x) => json!(x),
        Value::Bool(b) => json!(b),
        Value::List(items) => Json::Array(items.iter().map(value_to_json).collect()),
        Value::Record(fields) => Json::Object(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect(),
        ),
        Value::Verdict(verdict) => verdict_to_json(verdict),
        Value::Unsettled(outcome) => unsettled_to_json(outcome),
        Value::Unit => Json::Null,
    }
}

fn verdict_to_json(v: &Verdict) -> Json {
    match v {
        Verdict::Pass => json!({"verdict": "pass"}),
        Verdict::Fail(reason) => json!({"verdict": "fail", "reason": reason}),
        Verdict::Score(score) => json!({"verdict": "score", "score": score}),
        Verdict::Escalate => json!({"verdict": "escalate"}),
    }
}

fn unsettled_to_json(o: &DraftOutcome) -> Json {
    match o {
        DraftOutcome::Refused(reason) => json!({"unsettled": "refused", "reason": reason}),
        DraftOutcome::RateLimited => json!({"unsettled": "rate_limited"}),
        DraftOutcome::Timeout => json!({"unsettled": "timeout"}),
    }
}

/// Renders a whole trace as `Json`, `Jsonl`, `Mermaid`, or `Html`. Never
/// called with `Text` — `cmd_trace`'s existing per-record table stays
/// inline in `main.rs`, unchanged, and `run`/`approve`/`deny`/`replay`'s
/// `Text` output never involves a trace at all.
pub fn render_trace(format: OutputFormat, records: &[TraceRecord]) -> String {
    match format {
        OutputFormat::Text => unreachable!("Text trace rendering stays inline in main.rs"),
        OutputFormat::Json => {
            let values: Vec<Json> = records.iter().map(trace_record_to_json).collect();
            serde_json::to_string(&values).expect("json! output always serializes")
        }
        OutputFormat::Jsonl => records
            .iter()
            .map(|r| {
                serde_json::to_string(&trace_record_to_json(r))
                    .expect("json! output always serializes")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        OutputFormat::Mermaid => render_trace_mermaid(records),
        OutputFormat::Html => render_trace_html(records),
    }
}

fn trace_record_to_json(r: &TraceRecord) -> Json {
    json!({
        "seq": r.seq,
        "kind": r.kind,
        "capability": r.capability,
        "cache_hit": r.cache_hit,
        "output": r.output.as_ref().map(value_to_json),
        "error": r.error,
        "timestamp_ms": r.timestamp_ms,
    })
}

fn status_of(r: &TraceRecord) -> &'static str {
    if r.cache_hit {
        "hit"
    } else if r.error.is_some() {
        "err"
    } else {
        "miss"
    }
}

fn output_text_of(r: &TraceRecord) -> String {
    r.output
        .as_ref()
        .map(|v| v.to_string())
        .or_else(|| r.error.clone())
        .unwrap_or_default()
}

/// Char-count-based truncation (not byte-based — the runtime's values are
/// arbitrary LLM output, so multi-byte UTF-8 is the common case, not the
/// exception, and slicing on a byte offset can land inside a codepoint and
/// panic).
pub(crate) fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars).collect();
        format!("{head}...")
    }
}

fn render_trace_mermaid(records: &[TraceRecord]) -> String {
    let mut participants: Vec<&str> = Vec::new();
    for r in records {
        let label = r.capability.as_deref().unwrap_or(r.kind.as_str());
        if !participants.contains(&label) {
            participants.push(label);
        }
    }

    let mut out = String::from("sequenceDiagram\n");
    out.push_str("    participant Program\n");
    for label in &participants {
        out.push_str(&format!(
            "    participant {} as {}\n",
            mermaid_id(label),
            mermaid_escape(label)
        ));
    }

    for r in records {
        let label = r.capability.as_deref().unwrap_or(r.kind.as_str());
        let id = mermaid_id(label);
        out.push_str(&format!("    Program->>+{id}: #{} {label}\n", r.seq));
        let status = status_of(r);
        let text = mermaid_escape(&truncate(&output_text_of(r), 80));
        out.push_str(&format!("    {id}-->>-Program: [{status}] {text}\n"));
    }
    out.trim_end().to_string()
}

/// Sanitizes a capability/kind label into a valid bare Mermaid participant
/// id (letters/digits/underscore only, not digit-initial) — labels here can
/// contain `:` (e.g. `validator:regex`), which Mermaid's parser doesn't
/// accept unescaped. The human-readable label is preserved separately via
/// `participant <id> as <label>`.
fn mermaid_id(label: &str) -> String {
    let sanitized: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    match sanitized.chars().next() {
        None => "p".to_string(),
        Some(c) if c.is_ascii_digit() => format!("p_{sanitized}"),
        Some(_) => sanitized,
    }
}

/// Mermaid sequence-diagram messages run to the end of the source line, so
/// an embedded literal newline would silently corrupt the diagram — fold
/// whitespace instead.
fn mermaid_escape(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

const TRACE_HTML_CSS: &str = r#"
:root { color-scheme: light dark; }
body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
  max-width: 860px;
  margin: 2rem auto;
  padding: 0 1rem 3rem;
  line-height: 1.45;
}
h1 { font-size: 1.15rem; font-weight: 600; }
h1 code { font-weight: 400; opacity: .7; }
.record {
  border: 1px solid rgba(127, 127, 127, .35);
  border-left-width: 4px;
  border-radius: 8px;
  padding: .6rem .9rem;
  margin-bottom: .6rem;
}
.record.hit  { border-left-color: #2e9e44; }
.record.miss { border-left-color: #2f6fdb; }
.record.err  { border-left-color: #d3402f; }
.meta { font-size: .78rem; opacity: .65; margin-bottom: .35rem; }
.badge {
  display: inline-block;
  padding: .05rem .55rem;
  border-radius: 999px;
  font-size: .7rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: .02em;
  color: #fff;
}
.badge.hit  { background: #2e9e44; }
.badge.miss { background: #2f6fdb; }
.badge.err  { background: #d3402f; }
pre {
  white-space: pre-wrap;
  word-break: break-word;
  margin: 0;
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: .85rem;
}
@media (prefers-color-scheme: dark) {
  .record { border-color: rgba(255, 255, 255, .25); }
}
"#;

/// Self-contained (no JS, no external assets) HTML page. Every dynamic
/// piece of text embedded here ultimately originates from LLM output or
/// user args — untrusted as far as this page is concerned — so it's run
/// through `html_escape` before being spliced in; skipping that would let
/// e.g. a response containing `<script>` execute in whoever's browser
/// opens the file.
fn render_trace_html(records: &[TraceRecord]) -> String {
    let run_id = records
        .first()
        .map(|r| r.run_id.as_str())
        .unwrap_or("unknown");

    let mut body = String::new();
    for r in records {
        let status = status_of(r);
        let label = r.capability.as_deref().unwrap_or(r.kind.as_str());
        let text = output_text_of(r);
        body.push_str(&format!(
            "<div class=\"record {status}\">\n  <div class=\"meta\"><span class=\"badge {status}\">{status}</span> #{seq} &middot; {label}</div>\n  <pre>{text}</pre>\n</div>\n",
            status = status,
            seq = r.seq,
            label = html_escape(label),
            text = html_escape(&text),
        ));
    }

    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Ulexite trace — {run_id}</title>\n<style>{css}</style>\n</head>\n<body>\n<h1>Trace <code>{run_id}</code></h1>\n{body}</body>\n</html>\n",
        run_id = html_escape(run_id),
        css = TRACE_HTML_CSS,
        body = body,
    )
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn record(
        seq: u64,
        capability: &str,
        cache_hit: bool,
        output: Option<Value>,
        error: Option<&str>,
    ) -> TraceRecord {
        TraceRecord {
            run_id: "run123".to_string(),
            seq,
            kind: "effect".to_string(),
            capability: Some(capability.to_string()),
            cache_key: None,
            cache_hit,
            output,
            error: error.map(str::to_string),
            timestamp_ms: 0,
        }
    }

    #[test]
    fn value_to_json_covers_every_variant() {
        assert_eq!(value_to_json(&Value::Text("hi".into())), json!("hi"));
        assert_eq!(value_to_json(&Value::Int(42)), json!(42));
        assert_eq!(value_to_json(&Value::Float(1.5)), json!(1.5));
        assert_eq!(value_to_json(&Value::Bool(true)), json!(true));
        assert_eq!(value_to_json(&Value::Unit), Json::Null);
        assert_eq!(
            value_to_json(&Value::List(vec![Value::Int(1), Value::Int(2)])),
            json!([1, 2])
        );
        let mut fields = BTreeMap::new();
        fields.insert("a".to_string(), Value::Int(1));
        assert_eq!(value_to_json(&Value::Record(fields)), json!({"a": 1}));
        assert_eq!(
            value_to_json(&Value::Verdict(Verdict::Fail("nope".into()))),
            json!({"verdict": "fail", "reason": "nope"})
        );
        assert_eq!(
            value_to_json(&Value::Unsettled(DraftOutcome::RateLimited)),
            json!({"unsettled": "rate_limited"})
        );
    }

    #[test]
    fn render_run_json_shapes() {
        let v = Value::Text("hello".into());
        let s = render_run_json(&RunOutcome::Value {
            run_id: "abc",
            value: &v,
        });
        assert_eq!(s, r#"{"run_id":"abc","status":"ok","value":"hello"}"#);

        let s = render_run_json(&RunOutcome::Suspended {
            run_id: "abc",
            reason: "need approval",
            target: "human",
        });
        assert!(s.contains(r#""status":"suspended""#));
        assert!(s.contains(r#""run_id":"abc""#));
        assert!(s.contains("ulx approve abc"));

        let s = render_run_json(&RunOutcome::Error {
            run_id: "abc",
            message: "boom".into(),
        });
        assert_eq!(s, r#"{"message":"boom","run_id":"abc","status":"error"}"#);
    }

    #[test]
    fn render_trace_json_and_jsonl_round_trip_all_records() {
        let records = vec![
            record(0, "chat", false, Some(Value::Text("hi".into())), None),
            record(1, "chat", true, Some(Value::Text("hi".into())), None),
            record(2, "chat", false, None, Some("boom")),
        ];
        let jsonl = render_trace(OutputFormat::Jsonl, &records);
        assert_eq!(jsonl.lines().count(), 3);
        for line in jsonl.lines() {
            serde_json::from_str::<Json>(line).expect("each jsonl line is valid JSON");
        }

        let json_all = render_trace(OutputFormat::Json, &records);
        let parsed: Vec<Json> = serde_json::from_str(&json_all).expect("valid JSON array");
        assert_eq!(parsed.len(), 3);
    }

    #[test]
    fn render_trace_mermaid_sanitizes_labels_and_folds_newlines() {
        let records = vec![record(
            0,
            "validator:regex",
            false,
            Some(Value::Text("line one\nline two".into())),
            None,
        )];
        let out = render_trace(OutputFormat::Mermaid, &records);
        assert!(out.starts_with("sequenceDiagram\n"));
        assert!(out.contains("participant Program"));
        // `:` isn't valid in a bare participant id — must be sanitized.
        assert!(out.contains("participant validator_regex as validator:regex"));
        assert!(out.contains("Program->>+validator_regex:"));
        // the embedded literal `\n` in the value text must be folded to a
        // space, not left raw (a raw newline would break the diagram).
        assert!(out.contains("line one line two"));
    }

    #[test]
    fn mermaid_id_prefixes_digit_initial_labels() {
        // a bare Mermaid participant id can't start with a digit.
        assert_eq!(mermaid_id("123abc"), "p_123abc");
        assert_eq!(mermaid_id("chat"), "chat");
        assert_eq!(mermaid_id(""), "p");
    }

    #[test]
    fn render_trace_html_escapes_untrusted_output() {
        let records = vec![record(
            0,
            "chat",
            false,
            Some(Value::Text("<script>alert(1)</script>".into())),
            None,
        )];
        let out = render_trace(OutputFormat::Html, &records);
        assert!(!out.contains("<script>alert"));
        assert!(out.contains("&lt;script&gt;"));
        assert!(out.contains("class=\"record miss\""));
    }

    #[test]
    fn truncate_is_char_boundary_safe_on_multibyte_utf8() {
        let s = "é".repeat(200); // 2 bytes/char — a byte-based slice at 100 would panic
        let t = truncate(&s, 100);
        assert_eq!(t.chars().count(), 103); // 100 chars + "..."
    }

    #[test]
    fn truncate_leaves_short_strings_untouched() {
        assert_eq!(truncate("short", 100), "short");
    }
}
