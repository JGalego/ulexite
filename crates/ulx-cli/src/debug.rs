//! `ulx debug <run_id>` (§19): an interactive stepper over an already-
//! recorded trace — real, but a deliberately narrower slice of §19's full
//! design. What's here: forward/backward stepping record-by-record
//! through a completed or suspended run's trace, breakpoints by record
//! `seq`, full (untruncated) input/output/error inspection at the current
//! record, and a call-stack view (§18.2/§19.4, built on `parent_run_id`).
//! What's deliberately not here, because it would need capabilities this
//! runtime doesn't have: `breakpoint()` as a language keyword suspending
//! *live* interpretation (§19.2 — no grammar/interpreter changes for a new
//! statement kind), `ulx attach` to a genuinely in-flight process (§19.6 —
//! there's no long-running execution engine to attach to, only a
//! suspended run's trace-so-far on disk), and `ulx fork`/re-running from a
//! step with edited inputs (§18.4/§19.3 — this stepper is read-only over
//! the recorded trace, it doesn't re-invoke the interpreter). A suspended
//! run is still resumed the existing way, `ulx approve`/`ulx deny`, which
//! this module points you at rather than reimplementing.

use ulx_runtime::TraceRecord;

/// One interactive command this stepper understands. `Unknown` carries the
/// original input back so the REPL loop can print a clear "unrecognized
/// command" message that echoes what was actually typed.
#[derive(Debug, Clone, PartialEq)]
pub enum DebugCommand {
    Next,
    Back,
    Continue,
    SetBreakpoint(u64),
    Inspect,
    Stack,
    List,
    Help,
    Quit,
    Unknown(String),
}

pub fn parse_command(line: &str) -> DebugCommand {
    let line = line.trim();
    let mut parts = line.split_whitespace();
    match parts.next().unwrap_or("") {
        "next" | "n" => DebugCommand::Next,
        "back" | "prev" | "p" => DebugCommand::Back,
        "continue" | "cont" | "c" => DebugCommand::Continue,
        "break" | "bp" => match parts.next().and_then(|s| s.parse::<u64>().ok()) {
            Some(seq) => DebugCommand::SetBreakpoint(seq),
            None => DebugCommand::Unknown(line.to_string()),
        },
        "inspect" | "i" | "show" => DebugCommand::Inspect,
        "stack" | "where" | "w" => DebugCommand::Stack,
        "list" | "l" => DebugCommand::List,
        "help" | "h" | "?" => DebugCommand::Help,
        "quit" | "q" | "exit" => DebugCommand::Quit,
        "" => DebugCommand::Unknown(String::new()),
        _ => DebugCommand::Unknown(line.to_string()),
    }
}

pub const HELP_TEXT: &str = "\
Commands:
  next, n            step to the next trace record
  back, prev, p       step to the previous trace record
  continue, cont, c   run forward to the next breakpoint (or the end)
  break, bp <seq>     set a breakpoint at record #<seq>
  inspect, i, show    print the current record's full input/output/error
  stack, where, w     print the call stack (§18.2) at the current record
  list, l             list every record with its seq, kind, and capability
  help, h, ?          show this text
  quit, q, exit       leave the debugger";

/// A stepper over one run's already-recorded trace — read-only, so `back`
/// is just re-displaying an earlier record already held in memory, not a
/// genuine "undo" of interpreter state. `cursor: None` means "before the
/// first record" (the session's initial position, matching how a real
/// debugger starts stopped before any code has run).
pub struct DebugSession {
    records: Vec<TraceRecord>,
    cursor: Option<usize>,
    breakpoints: std::collections::BTreeSet<u64>,
}

impl DebugSession {
    pub fn new(records: Vec<TraceRecord>) -> Self {
        DebugSession {
            records,
            cursor: None,
            breakpoints: std::collections::BTreeSet::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn current(&self) -> Option<&TraceRecord> {
        self.cursor.and_then(|i| self.records.get(i))
    }

    /// Advances one record; `None` (with the cursor left at the last
    /// record) once there's nothing further to step into.
    pub fn step_next(&mut self) -> Option<&TraceRecord> {
        let next = match self.cursor {
            None if !self.records.is_empty() => 0,
            Some(i) if i + 1 < self.records.len() => i + 1,
            _ => return None,
        };
        self.cursor = Some(next);
        self.records.get(next)
    }

    /// Steps back one record; `None` (cursor left at the first record, or
    /// at "before the first record" if already there) when there's nowhere
    /// further back to go.
    pub fn step_back(&mut self) -> Option<&TraceRecord> {
        match self.cursor {
            Some(0) | None => None,
            Some(i) => {
                self.cursor = Some(i - 1);
                self.records.get(i - 1)
            }
        }
    }

    pub fn set_breakpoint(&mut self, seq: u64) {
        self.breakpoints.insert(seq);
    }

    /// Steps forward until a record whose `seq` is a set breakpoint, or
    /// until the trace ends — whichever comes first. Always advances by at
    /// least one record if there is one, mirroring an ordinary debugger's
    /// `continue` never re-stopping on the record you're already sitting
    /// on.
    pub fn continue_to_breakpoint(&mut self) -> Option<&TraceRecord> {
        loop {
            let next = match self.cursor {
                None if !self.records.is_empty() => 0,
                Some(i) if i + 1 < self.records.len() => i + 1,
                _ => return None,
            };
            self.cursor = Some(next);
            if self.breakpoints.contains(&self.records[next].seq) {
                break;
            }
            if next + 1 >= self.records.len() {
                break;
            }
        }
        self.current()
    }

    /// The chain of enclosing conversation names, root to immediate
    /// parent, for the record at `cursor` — reconstructed by walking
    /// `parent_run_id` back through the same synthetic `"{run_id}:{seq}"`
    /// links `crate::output::call_depths` reads, but returning the actual
    /// names (a "call" record's own `capability` is the conversation it
    /// invoked) rather than just a depth count.
    pub fn call_chain(&self) -> Vec<String> {
        let Some(current) = self.current() else {
            return Vec::new();
        };
        let by_seq: std::collections::HashMap<u64, &TraceRecord> =
            self.records.iter().map(|r| (r.seq, r)).collect();
        let mut chain = Vec::new();
        let mut parent_seq = current
            .parent_run_id
            .as_deref()
            .and_then(|p| p.rsplit(':').next())
            .and_then(|s| s.parse::<u64>().ok());
        while let Some(seq) = parent_seq {
            let Some(r) = by_seq.get(&seq) else { break };
            chain.push(r.capability.clone().unwrap_or_else(|| "?".to_string()));
            parent_seq = r
                .parent_run_id
                .as_deref()
                .and_then(|p| p.rsplit(':').next())
                .and_then(|s| s.parse::<u64>().ok());
        }
        chain.reverse();
        chain
    }

    /// `(target, reason)` if this run's trace ends on a real `escalate(...)`
    /// suspend point (`eval_escalate`'s "suspended" record — `capability:
    /// Some("escalate")`, `error: Some("suspended")`) — `cmd_debug` uses
    /// this for a startup banner pointing at `ulx approve`/`ulx deny`
    /// rather than reimplementing the resume flow itself.
    pub fn suspend_info(&self) -> Option<(String, String)> {
        let last = self.records.last()?;
        if last.capability.as_deref() != Some("escalate")
            || last.error.as_deref() != Some("suspended")
        {
            return None;
        }
        let target = last.input.first()?.role.clone();
        let reason = last.input.first()?.text.clone();
        Some((target, reason))
    }

    pub fn render_list(&self) -> String {
        self.records
            .iter()
            .map(|r| {
                let marker = if self.cursor == self.records.iter().position(|x| x.seq == r.seq) {
                    "*"
                } else {
                    " "
                };
                let bp = if self.breakpoints.contains(&r.seq) {
                    "b"
                } else {
                    " "
                };
                format!(
                    "{marker}{bp} #{:<3} [{}] {}",
                    r.seq,
                    r.kind,
                    r.capability.as_deref().unwrap_or("-")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn render_stack(&self) -> String {
        let Some(current) = self.current() else {
            return "(no current record — step with `next` first)".to_string();
        };
        let mut chain = self.call_chain();
        chain.push(format!(
            "{} (current)",
            current
                .capability
                .as_deref()
                .unwrap_or(current.kind.as_str())
        ));
        chain.join(" > ")
    }

    /// Full, untruncated dump of the current record — unlike `ulx trace`'s
    /// default table, which truncates output to keep one line per record.
    pub fn render_inspect(&self) -> String {
        let Some(r) = self.current() else {
            return "(no current record — step with `next` first)".to_string();
        };
        let mut out = format!(
            "#{} [{}] kind={} cache={}\n",
            r.seq,
            r.capability.as_deref().unwrap_or("-"),
            r.kind,
            if r.cache_hit { "hit" } else { "miss" }
        );
        if let Some(p) = &r.provider {
            out.push_str(&format!(
                "provider: {p}{}\n",
                r.model
                    .as_deref()
                    .map(|m| format!(" ({m})"))
                    .unwrap_or_default()
            ));
        }
        if r.input.is_empty() {
            out.push_str("input: (none)\n");
        } else {
            out.push_str("input:\n");
            for m in &r.input {
                out.push_str(&format!("  {}: {}\n", m.role, m.text));
            }
        }
        match (&r.output, &r.error) {
            (Some(v), _) => out.push_str(&format!("output: {v}\n")),
            (None, Some(e)) => out.push_str(&format!("error: {e}\n")),
            (None, None) => out.push_str("output: (none)\n"),
        }
        out.push_str(&format!("timestamp_ms: {}", r.timestamp_ms));
        out
    }

    /// A one-line summary for the record just stepped to — what `next`/
    /// `back`/`continue` print, distinct from `render_inspect`'s full dump
    /// so ordinary stepping stays scannable and `inspect` is the deliberate
    /// "show me everything" action.
    pub fn render_current_summary(&self) -> String {
        let Some(r) = self.current() else {
            return "(end of trace)".to_string();
        };
        let status = if r.cache_hit {
            "hit "
        } else if r.error.is_some() {
            "err "
        } else {
            "miss"
        };
        let out = r
            .output
            .as_ref()
            .map(|v| v.to_string())
            .or_else(|| r.error.clone())
            .unwrap_or_default();
        format!(
            "#{:<3} [{status}] {:<10} {}",
            r.seq,
            r.capability.as_deref().unwrap_or("-"),
            crate::output::truncate(&out, 100)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ulx_runtime::provider::Message;
    use ulx_runtime::Value;

    fn rec(seq: u64, capability: &str, parent: Option<&str>) -> TraceRecord {
        TraceRecord {
            run_id: "r".to_string(),
            seq,
            kind: "call".to_string(),
            capability: Some(capability.to_string()),
            cache_key: None,
            cache_hit: false,
            input: vec![Message {
                role: "x".to_string(),
                text: "hi".to_string(),
            }],
            provider: None,
            model: None,
            output: Some(Value::Text("ok".to_string())),
            error: None,
            timestamp_ms: 0,
            parent_run_id: parent.map(str::to_string),
        }
    }

    #[test]
    fn parse_command_recognizes_every_alias() {
        assert_eq!(parse_command("next"), DebugCommand::Next);
        assert_eq!(parse_command("n"), DebugCommand::Next);
        assert_eq!(parse_command("back"), DebugCommand::Back);
        assert_eq!(parse_command("p"), DebugCommand::Back);
        assert_eq!(parse_command("continue"), DebugCommand::Continue);
        assert_eq!(parse_command("c"), DebugCommand::Continue);
        assert_eq!(parse_command("break 3"), DebugCommand::SetBreakpoint(3));
        assert_eq!(parse_command("bp 7"), DebugCommand::SetBreakpoint(7));
        assert_eq!(parse_command("inspect"), DebugCommand::Inspect);
        assert_eq!(parse_command("stack"), DebugCommand::Stack);
        assert_eq!(parse_command("list"), DebugCommand::List);
        assert_eq!(parse_command("help"), DebugCommand::Help);
        assert_eq!(parse_command("quit"), DebugCommand::Quit);
        assert_eq!(
            parse_command("break notanumber"),
            DebugCommand::Unknown("break notanumber".to_string())
        );
        assert_eq!(
            parse_command("gibberish"),
            DebugCommand::Unknown("gibberish".to_string())
        );
    }

    #[test]
    fn step_next_and_back_move_the_cursor_one_record_at_a_time() {
        let mut s = DebugSession::new(vec![rec(0, "A", None), rec(1, "B", None)]);
        assert!(s.current().is_none());
        assert_eq!(s.step_next().unwrap().seq, 0);
        assert_eq!(s.step_next().unwrap().seq, 1);
        assert!(s.step_next().is_none(), "no more records past the end");
        assert_eq!(
            s.current().unwrap().seq,
            1,
            "cursor stays at the last record"
        );

        assert_eq!(s.step_back().unwrap().seq, 0);
        assert!(s.step_back().is_none(), "no more records before the start");
    }

    #[test]
    fn continue_to_breakpoint_stops_exactly_at_a_set_breakpoint() {
        let mut s = DebugSession::new(vec![
            rec(0, "A", None),
            rec(1, "B", None),
            rec(2, "C", None),
            rec(3, "D", None),
        ]);
        s.set_breakpoint(2);
        assert_eq!(s.continue_to_breakpoint().unwrap().seq, 2);
        // Calling continue again should advance past the breakpoint it's
        // already sitting on, not stay stuck reporting the same record.
        assert_eq!(s.continue_to_breakpoint().unwrap().seq, 3);
        assert!(s.continue_to_breakpoint().is_none());
    }

    #[test]
    fn continue_with_no_breakpoint_runs_to_the_end() {
        let mut s = DebugSession::new(vec![rec(0, "A", None), rec(1, "B", None)]);
        assert_eq!(s.continue_to_breakpoint().unwrap().seq, 1);
    }

    #[test]
    fn call_chain_reconstructs_nesting_from_parent_run_id() {
        let records = vec![rec(0, "Middle", None), rec(1, "Leaf", Some("r:0"))];
        let mut s = DebugSession::new(records);
        s.step_next(); // Middle
        assert_eq!(s.call_chain(), Vec::<String>::new());
        assert_eq!(s.render_stack(), "Middle (current)");

        s.step_next(); // Leaf
        assert_eq!(s.call_chain(), vec!["Middle".to_string()]);
        assert_eq!(s.render_stack(), "Middle > Leaf (current)");
    }

    #[test]
    fn inspect_shows_full_input_and_output() {
        let mut s = DebugSession::new(vec![rec(0, "A", None)]);
        s.step_next();
        let out = s.render_inspect();
        assert!(out.contains("x: hi"));
        assert!(out.contains("output: ok"));
    }

    #[test]
    fn suspend_info_detects_a_real_escalate_suspend_record() {
        let mut suspended = rec(0, "escalate", None);
        suspended.error = Some("suspended".to_string());
        suspended.input = vec![Message {
            role: "human_approval".to_string(),
            text: "needs a human decision".to_string(),
        }];
        let s = DebugSession::new(vec![suspended]);
        assert_eq!(
            s.suspend_info(),
            Some((
                "human_approval".to_string(),
                "needs a human decision".to_string()
            ))
        );

        let completed = DebugSession::new(vec![rec(0, "A", None)]);
        assert_eq!(completed.suspend_info(), None);
    }

    #[test]
    fn empty_trace_reports_no_current_record_rather_than_panicking() {
        let mut s = DebugSession::new(vec![]);
        assert!(s.step_next().is_none());
        assert!(s.step_back().is_none());
        assert_eq!(
            s.render_inspect(),
            "(no current record — step with `next` first)"
        );
        assert_eq!(
            s.render_stack(),
            "(no current record — step with `next` first)"
        );
    }
}
