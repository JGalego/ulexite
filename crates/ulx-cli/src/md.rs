//! A deliberately tiny Markdown dialect that compiles down to a `.ulx`
//! `conversation` — the "anyone can write this, no braces, no keywords"
//! authoring format `ulx from-md` reads. See `docs/simple-format.md` for
//! the full spec; this module is the reference implementation of it.
//!
//! The minimal case is just a title and a paragraph:
//!
//! ```text
//! # Greet
//!
//! Say hello to {name} and ask how their day is going.
//! ```
//!
//! `{name}` is auto-detected as a `text` parameter — no declaration needed.
//! `## System` and `## Judge` sections are opt-in. A judge turns on the same
//! judge-checked retry-with-escalation shape `examples/translate.ulx` hand-
//! writes (Pass/Fail/Escalate/Score, `retry(2)`, `escalate(human_approval,
//! ...)`) rather than exposing that machinery as something the Markdown
//! author has to assemble themselves. A fenced ` ```ulx-meta ` block is the
//! escape hatch for anything the defaults can't express: an explicit
//! conversation name, a non-`text` return type, or non-`text` param types.

use serde::Deserialize;

#[derive(Deserialize, Default)]
struct MetaToml {
    name: Option<String>,
    returns: Option<String>,
    #[serde(default)]
    params: Vec<ParamToml>,
}

#[derive(Deserialize)]
struct ParamToml {
    name: String,
    #[serde(rename = "type")]
    ty: String,
}

pub struct MdConversation {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub returns: String,
    pub system: Option<String>,
    pub ask: String,
    pub judge: Option<String>,
}

enum Section {
    Body,
    System,
    Ask,
    Judge,
    Ignored,
}

/// Parses the Markdown dialect described in the module docs. Errors are
/// plain, human-readable strings (there's no source-span machinery here —
/// the format is small enough that "what's wrong" is always self-evident
/// from the message alone).
pub fn parse_md(src: &str) -> Result<MdConversation, String> {
    let mut in_fence = false;
    let mut fence_is_meta = false;
    let mut meta_buf: Vec<&str> = Vec::new();
    let mut meta_toml: Option<String> = None;

    let mut title: Option<&str> = None;
    let mut current = Section::Body;
    let mut body: Vec<&str> = Vec::new();
    let mut system: Vec<&str> = Vec::new();
    let mut ask: Vec<&str> = Vec::new();
    let mut judge: Vec<&str> = Vec::new();

    for line in src.lines() {
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") {
            if in_fence {
                if fence_is_meta {
                    meta_toml = Some(meta_buf.join("\n"));
                }
                in_fence = false;
                fence_is_meta = false;
                meta_buf.clear();
            } else {
                in_fence = true;
                fence_is_meta = trimmed
                    .trim_start_matches('`')
                    .trim()
                    .eq_ignore_ascii_case("ulx-meta");
            }
            continue;
        }
        if in_fence {
            if fence_is_meta {
                meta_buf.push(line);
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("## ") {
            current = match rest.trim().to_ascii_lowercase().as_str() {
                "system" => Section::System,
                "ask" | "user" => Section::Ask,
                "judge" => Section::Judge,
                _ => Section::Ignored,
            };
            continue;
        }
        if title.is_none() {
            if let Some(rest) = trimmed.strip_prefix("# ") {
                title = Some(rest.trim());
                continue;
            }
        }

        match current {
            Section::Body => body.push(line),
            Section::System => system.push(line),
            Section::Ask => ask.push(line),
            Section::Judge => judge.push(line),
            Section::Ignored => {}
        }
    }

    let title = title.ok_or_else(|| {
        "the Markdown file needs a top-level `# Title` heading naming the conversation".to_string()
    })?;

    let meta: MetaToml = match meta_toml {
        Some(s) => toml::from_str(&s).map_err(|e| format!("invalid ```ulx-meta block: {e}"))?,
        None => MetaToml::default(),
    };

    let join = |lines: Vec<&str>| lines.join("\n").trim().to_string();

    let ask_text = {
        let explicit = join(ask);
        if explicit.is_empty() {
            join(body)
        } else {
            explicit
        }
    };
    if ask_text.is_empty() {
        return Err(
            "write a paragraph under the title (or add a `## Ask` section) describing what to \
             ask the model"
                .to_string(),
        );
    }

    let system_text = {
        let s = join(system);
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    };
    let judge_text = {
        let j = join(judge);
        if j.is_empty() {
            None
        } else {
            Some(j)
        }
    };

    let name = match meta.name {
        Some(n) => n,
        None => to_identifier(title),
    };
    if name.is_empty() {
        return Err(format!(
            "could not derive a valid conversation name from title {title:?} — set `name` in a \
             ```ulx-meta block instead"
        ));
    }
    let returns = meta.returns.unwrap_or_else(|| "text".to_string());

    let params: Vec<(String, String)> = if !meta.params.is_empty() {
        meta.params.into_iter().map(|p| (p.name, p.ty)).collect()
    } else {
        let mut names = find_placeholders(system_text.as_deref().unwrap_or(""));
        for n in find_placeholders(&ask_text) {
            if !names.contains(&n) {
                names.push(n);
            }
        }
        names.into_iter().map(|n| (n, "text".to_string())).collect()
    };

    Ok(MdConversation {
        name,
        params,
        returns,
        system: system_text,
        ask: ask_text,
        judge: judge_text,
    })
}

/// Renders the parsed conversation as `.ulx` source. Without a judge this is
/// a plain `ask`; with one, it's the same judge-checked retry-with-
/// escalation shape as `examples/translate.ulx` §21.1 (Pass/Fail/Escalate/
/// Score arms, `retry(2)`, `escalate(human_approval, ...)`), just generated
/// instead of hand-written.
pub fn render_ulx(conv: &MdConversation) -> String {
    let params = conv
        .params
        .iter()
        .map(|(n, t)| format!("{n}: {t}"))
        .collect::<Vec<_>>()
        .join(", ");

    let mut out = String::new();
    let judge_name = format!("{}Judge", conv.name);

    if let Some(rubric) = &conv.judge {
        out.push_str(&format!(
            "judge {judge_name}(subject: text) -> Verdict {{\n  rubric: \"\"\"{rubric}\"\"\"\n}}\n\n"
        ));
    }

    out.push_str(&format!(
        "conversation {}({params}) -> {} {{\n",
        conv.name, conv.returns
    ));
    if let Some(system) = &conv.system {
        out.push_str(&format!("  system: \"\"\"{system}\"\"\"\n"));
    }
    out.push_str(&format!("  user: \"\"\"{}\"\"\"\n", conv.ask));
    out.push_str(&format!("  assistant -> answer: {}\n", conv.returns));

    if conv.judge.is_some() {
        out.push('\n');
        out.push_str(&format!("  match judge {judge_name}(answer) {{\n"));
        out.push_str("    Pass          => answer\n");
        out.push_str("    Fail(reason)  => retry(2) {\n");
        out.push_str(
            "                        user: \"\"\"The previous answer was rejected: {reason}. Try again.\"\"\"\n",
        );
        out.push_str("                        assistant -> answer\n");
        out.push_str("                      } else escalate(human_approval, reason: reason)\n");
        out.push_str(
            "    Escalate      => escalate(human_approval, reason: \"judge could not decide\")\n",
        );
        out.push_str("    Score(_)      => answer\n");
        out.push_str("  }\n");
    } else {
        out.push_str("  answer\n");
    }
    out.push_str("}\n");
    out
}

fn to_identifier(title: &str) -> String {
    title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Finds every `{identifier}` placeholder in `text`, in first-seen order,
/// deduplicated — how a param list is inferred when no ```ulx-meta` block
/// overrides it. Byte-slices only ever on `find`'s return (always a char
/// boundary) or right after a `{`/`}` (both single-byte ASCII), so this
/// never panics on non-ASCII prose around the placeholders.
fn find_placeholders(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            break;
        };
        let candidate = &after[..end];
        if is_valid_ident(candidate) && !names.iter().any(|n| n == candidate) {
            names.push(candidate.to_string());
        }
        rest = &after[end + 1..];
    }
    names
}

fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_semantically(src: &str) {
        let dir = std::env::temp_dir().join(format!(
            "ulexite-cli-md-test-{}-{}",
            std::process::id(),
            src.len()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("main.ulx");
        std::fs::write(&file, src).unwrap();

        let ws =
            ulx_sema::analyze_file_with_deps(&file, None, &ulx_sema::DependencyPaths::default())
                .unwrap_or_else(|e| panic!("analysis failed: {e}\n--- generated .ulx ---\n{src}"));
        for module in ws.modules.values() {
            for d in &module.diagnostics {
                assert_ne!(
                    d.severity,
                    ulx_sema::Severity::Error,
                    "semantic error in generated .ulx: {:?}\n--- generated .ulx ---\n{src}",
                    d
                );
            }
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn minimal_greet_with_auto_inferred_param() {
        let md = "# Greet\n\nSay hello to {name} and ask how their day is going.\n";
        let conv = parse_md(md).unwrap();
        assert_eq!(conv.name, "Greet");
        assert_eq!(conv.params, vec![("name".to_string(), "text".to_string())]);
        assert_eq!(conv.returns, "text");
        assert!(conv.judge.is_none());

        let ulx = render_ulx(&conv);
        assert!(ulx_syntax::parse_source(&ulx).is_ok(), "{ulx}");
        check_semantically(&ulx);
    }

    #[test]
    fn system_and_judge_sections_generate_a_retry_escalation_conversation() {
        let md = "\
# Translate

## System
You are a professional translator.

## Ask
Translate to {target_lang}: {source}

## Judge
Is this an accurate, fluent translation? Answer Pass or Fail(reason).
";
        let conv = parse_md(md).unwrap();
        assert_eq!(conv.name, "Translate");
        assert_eq!(
            conv.params,
            vec![
                ("target_lang".to_string(), "text".to_string()),
                ("source".to_string(), "text".to_string()),
            ]
        );
        assert!(conv.judge.is_some());

        let ulx = render_ulx(&conv);
        assert!(ulx_syntax::parse_source(&ulx).is_ok(), "{ulx}");
        check_semantically(&ulx);
    }

    #[test]
    fn ulx_meta_block_overrides_name_returns_and_param_types() {
        let md = "\
# score it

Rate this review from 1 to 5: {review}

```ulx-meta
name = \"ScoreReview\"
returns = \"number\"

[[params]]
name = \"review\"
type = \"text\"
```
";
        let conv = parse_md(md).unwrap();
        assert_eq!(conv.name, "ScoreReview");
        assert_eq!(conv.returns, "number");
        assert_eq!(
            conv.params,
            vec![("review".to_string(), "text".to_string())]
        );

        let ulx = render_ulx(&conv);
        assert!(ulx_syntax::parse_source(&ulx).is_ok(), "{ulx}");
        check_semantically(&ulx);
    }

    #[test]
    fn missing_title_is_an_error() {
        assert!(parse_md("just a paragraph, no heading\n").is_err());
    }

    #[test]
    fn missing_body_is_an_error() {
        assert!(parse_md("# Empty\n").is_err());
    }
}
