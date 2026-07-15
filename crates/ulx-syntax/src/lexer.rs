//! Lexer for Ulexite (§13.2 — logos, per the compiler-architecture RFC).
//!
//! Keywords are deliberately *not* distinct token variants: they lex as
//! plain `Ident`s and are recognized contextually in the parser. This keeps
//! the lexer free of keyword/identifier ambiguity (no reserved-word list to
//! maintain here) at the cost of a handful of string comparisons in the
//! parser — a standard, low-risk trade for a young grammar that is still
//! gaining vocabulary (§25 may promote some of these to real keywords once
//! the grammar is stable).

use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*")]
#[logos(skip r"/\*([^*]|\*[^*/])*\*+/")]
pub enum Token {
    #[regex(r"[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    Float(f64),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Int(i64),

    #[regex(r#""([^"\\]|\\.)*""#, lex_string)]
    Str(String),

    /// Triple-quoted text block (§7.1, §8 `text_block`). Content has
    /// backslash escapes (`\n`, `\t`, `\"`, `\\`) resolved the same as a
    /// plain `"..."` string (`unescape`, below) — but `{`/`}` interpolation
    /// splitting is deliberately *not* done here; that happens in the
    /// parser (`parser.rs`'s `split_text_block`), which re-invokes this
    /// lexer on each `{expr}` span it finds. Doing the backslash-unescape
    /// first rather than after splitting is safe: it only ever turns a
    /// literal `\n`/`\t`/etc. into the real character, never into `{`/`}`,
    /// so it can't change where an interpolation starts or ends, and a
    /// nested string literal inside `{...}` (e.g. `{judge X(r: "a\nb")}`)
    /// ends up with the same real newline either way — the nested
    /// re-lex's own `unescape` call is a no-op on text that no longer has
    /// a backslash in it.
    #[token("\"\"\"", lex_text_block)]
    TextBlock(String),

    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    /// `@prompts/system.txt` — bare shorthand for `file("prompts/system.txt")`
    /// (§8 `file_expr`). The leading `@` is stripped by the callback; the
    /// character class excludes whitespace and every grammar delimiter
    /// (`{`, `}`, `(`, `)`, `,`, `:`), so it never swallows past the path.
    #[regex(r"@[A-Za-z0-9_./-]+", |lex| lex.slice()[1..].to_string())]
    AtPath(String),

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token(".")]
    Dot,
    #[token("|")]
    Pipe,
    #[token("==")]
    EqEq,
    #[token("=")]
    Eq,
    #[token("!=")]
    Ne,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("$")]
    Dollar,
}

// `Simple<Token>` (chumsky's built-in error type) requires `Token: Eq + Hash`.
// `f64` has no total `Eq`, so we derive only `PartialEq` above and provide
// these manually; our grammar's float literals can never lex to `NaN`, so
// bitwise comparison/hashing is sound for every value this lexer produces.
impl Eq for Token {}

impl std::hash::Hash for Token {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Token::Float(f) => f.to_bits().hash(state),
            Token::Int(i) => i.hash(state),
            Token::Str(s) | Token::TextBlock(s) | Token::Ident(s) | Token::AtPath(s) => {
                s.hash(state)
            }
            _ => {}
        }
    }
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Float(v) => write!(f, "{v}"),
            Token::Int(v) => write!(f, "{v}"),
            Token::Str(s) => write!(f, "{s:?}"),
            Token::TextBlock(_) => write!(f, "<text block>"),
            Token::Ident(s) => write!(f, "{s}"),
            Token::AtPath(s) => write!(f, "@{s}"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::Comma => write!(f, ","),
            Token::Colon => write!(f, ":"),
            Token::Arrow => write!(f, "->"),
            Token::FatArrow => write!(f, "=>"),
            Token::Dot => write!(f, "."),
            Token::Pipe => write!(f, "|"),
            Token::EqEq => write!(f, "=="),
            Token::Eq => write!(f, "="),
            Token::Ne => write!(f, "!="),
            Token::Le => write!(f, "<="),
            Token::Ge => write!(f, ">="),
            Token::Lt => write!(f, "<"),
            Token::Gt => write!(f, ">"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Dollar => write!(f, "$"),
        }
    }
}

fn lex_string(lex: &mut logos::Lexer<Token>) -> Option<String> {
    let raw = lex.slice();
    let inner = &raw[1..raw.len() - 1];
    Some(unescape(inner))
}

/// Called immediately after the opening `"""` token has matched; consumes
/// everything up to (and including) the closing `"""` from the remainder.
fn lex_text_block(lex: &mut logos::Lexer<Token>) -> Option<String> {
    let remainder = lex.remainder();
    let bytes = remainder.as_bytes();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 3] == b"\"\"\"" {
            let content = &remainder[..i];
            lex.bump(i + 3);
            return Some(unescape(content));
        }
        i += 1;
    }
    None
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Lex `src` into a `(Token, Span)` stream, suitable for feeding to chumsky.
/// Returns `Err` with the byte offset of the first unrecognized character.
pub fn lex(src: &str) -> Result<Vec<(Token, std::ops::Range<usize>)>, usize> {
    let mut out = Vec::new();
    let mut lexer = Token::lexer(src);
    while let Some(tok) = lexer.next() {
        match tok {
            Ok(t) => out.push((t, lexer.span())),
            Err(_) => return Err(lexer.span().start),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only_text_block(src: &str) -> String {
        let tokens = lex(src).expect("must lex");
        match tokens.as_slice() {
            [(Token::TextBlock(s), _)] => s.clone(),
            other => panic!("expected exactly one TextBlock token, got {other:?}"),
        }
    }

    #[test]
    fn text_block_resolves_backslash_escapes_same_as_a_plain_string() {
        assert_eq!(
            only_text_block(r#""""line one\nline two""""#),
            "line one\nline two"
        );
        assert_eq!(only_text_block(r#""""a\tb""""#), "a\tb");
        assert_eq!(only_text_block(r#""""C:\\path""""#), "C:\\path");

        // Built with `push_str`/`push` rather than a hand-escaped literal —
        // easier to get right by eye than counting nested quote characters.
        // Note this deliberately keeps the escaped quote away from the
        // closing `"""`: `lex_text_block`'s own closing-delimiter scan
        // (separate from the `unescape` this test targets) has no
        // backslash-escape awareness of its own, so `\"` immediately
        // followed by two more literal quote characters can still confuse
        // it into closing early — a real, narrower, pre-existing gap in
        // the same family as the interpolation-splitting one
        // docs/spec/24-limitations.md already records, not something this
        // fix (`\n`/`\t`/`\\` resolution) touches.
        let mut src = String::from("\"\"\"before ");
        src.push('\\');
        src.push('"');
        src.push_str("quoted");
        src.push('\\');
        src.push('"');
        src.push_str(" after\"\"\"");
        assert_eq!(only_text_block(&src), "before \"quoted\" after");
    }

    #[test]
    fn text_block_escape_resolves_before_interpolation_braces_are_untouched() {
        // The unescape pass must never itself introduce or consume a `{`/`}`
        // — interpolation splitting happens later, in the parser, on
        // whatever this lexer hands back.
        assert_eq!(
            only_text_block(r#""""Hello {name}\nGoodbye""""#),
            "Hello {name}\nGoodbye"
        );
    }
}
