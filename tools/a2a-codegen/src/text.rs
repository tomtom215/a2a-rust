// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Text for the generated source: doc comments from the proto's comments,
//! and the names serde and prost give proto identifiers.

use std::fmt::Write as _;

/// The doc comment: `doc`, or the proto comment, then `append`; refused when
/// both are empty, since every public item is documented.
pub fn doc(
    what: &str,
    comment: &str,
    doc: Option<&str>,
    append: Option<&str>,
    indent: usize,
) -> Result<String, String> {
    let mut text = match doc {
        Some(doc) => doc.trim().to_owned(),
        None if indent == 0 => short_first_paragraph(&escape(comment)),
        None => escape(comment),
    };
    if let Some(append) = append {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(append.trim());
    }
    if text.is_empty() {
        return Err(format!("{what} has no proto comment; give it a `doc`"));
    }
    let pad = "    ".repeat(indent);
    let mut out = String::new();
    for line in text.lines() {
        if line.is_empty() {
            let _ = writeln!(out, "{pad}///");
        } else {
            let _ = writeln!(out, "{pad}/// {line}");
        }
    }
    Ok(out)
}

/// The longest first paragraph clippy's `too_long_first_doc_paragraph`
/// accepts on an item (197 characters passed and 210 failed, measured here).
const FIRST_PARAGRAPH_MAX: usize = 200;

/// `text` with its first paragraph ended after the first sentence when that
/// paragraph is longer than [`FIRST_PARAGRAPH_MAX`]: rustdoc shows the first
/// paragraph as the item's summary.
pub fn short_first_paragraph(text: &str) -> String {
    let end = text.find("\n\n").unwrap_or(text.len());
    let first = &text[..end];
    if first.chars().count() <= FIRST_PARAGRAPH_MAX {
        return text.to_owned();
    }
    let sentence = first
        .match_indices(". ")
        .map(|(i, _)| i)
        .find(|&i| !first[..i].ends_with("e.g") && !first[..i].ends_with("i.e"));
    let Some(i) = sentence else {
        return text.to_owned();
    };
    let rest = first[i + 2..].trim_start();
    format!("{}.\n\n{rest}{}", &first[..i], &text[end..])
}

/// A proto comment as Markdown that reads as it was written: outside code
/// spans, brackets and angle brackets are escaped, so rustdoc takes them for
/// neither links nor HTML, and a URL (with any quotes around it) becomes a
/// code span, since the proto's URLs are examples rather than links.
pub fn escape(comment: &str) -> String {
    comment
        .lines()
        .map(escape_line)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn escape_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for (i, word) in line.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let ticks = word.matches('`').count();
        if !in_code && ticks == 0 && word.contains("://") {
            let end = word.trim_end_matches([',', '.', ';', ')']);
            let _ = write!(out, "`{end}`{}", &word[end.len()..]);
            continue;
        }
        if !in_code && ticks == 0 {
            let start = word.len() - word.trim_start_matches(['(', '"', '\'']).len();
            let end = word
                .trim_end_matches([',', '.', ';', ':', ')', '"', '\''])
                .len();
            if start < end && is_identifier(&word[start..end]) {
                let _ = write!(
                    out,
                    "{}`{}`{}",
                    &word[..start],
                    &word[start..end],
                    &word[end..]
                );
                continue;
            }
        }
        for c in word.chars() {
            if c == '`' {
                in_code = !in_code;
            }
            if !in_code && matches!(c, '[' | ']' | '<' | '>') {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out
}

/// A word rustdoc readers take for code and clippy's `doc_markdown` wants in
/// a code span: letters and digits with a capital after a lower-case letter
/// (`AgentCard`, `ProtoJSON`).
pub fn is_identifier(word: &str) -> bool {
    word.chars().all(|c| c.is_ascii_alphanumeric())
        && word
            .as_bytes()
            .windows(2)
            .any(|w| w[0].is_ascii_lowercase() && w[1].is_ascii_uppercase())
}

/// `TaskState` as `TASK_STATE`.
pub fn screaming(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// `INPUT_REQUIRED` as `InputRequired`.
pub fn pascal(screaming: &str) -> String {
    screaming
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| {
                f.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase()
            })
        })
        .collect()
}

/// `protocol_binding` as `protocolBinding`, as serde's `camelCase` does.
pub fn camel(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in snake.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_translate_as_serde_and_prost_do() {
        assert_eq!(screaming("TaskState"), "TASK_STATE");
        assert_eq!(pascal("INPUT_REQUIRED"), "InputRequired");
        assert_eq!(camel("protocol_binding"), "protocolBinding");
        assert_eq!(camel("url"), "url");
    }

    #[test]
    fn escape_leaves_code_spans_alone() {
        assert_eq!(escape("a [b] `<c>` <d>"), "a \\[b\\] `<c>` \\<d\\>");
    }

    #[test]
    fn a_long_first_paragraph_ends_after_its_first_sentence() {
        let long = format!("One (e.g. this). Two {}\nthree.\n\nNext.", "x".repeat(200));
        assert_eq!(
            short_first_paragraph(&long),
            format!(
                "One (e.g. this).\n\nTwo {}\nthree.\n\nNext.",
                "x".repeat(200)
            )
        );
        assert_eq!(short_first_paragraph("Short. Text."), "Short. Text.");
    }

    #[test]
    fn identifiers_become_code_spans() {
        assert_eq!(
            escape("AgentCardSignature signs an AgentCard."),
            "`AgentCardSignature` signs an `AgentCard`."
        );
        assert_eq!(
            escape("(see ProtoJSON) A2A JSON"),
            "(see `ProtoJSON`) A2A JSON"
        );
        assert_eq!(escape("`AgentCard` stays"), "`AgentCard` stays");
    }

    #[test]
    fn urls_become_code_spans_with_their_quotes() {
        assert_eq!(
            escape("Example: \"https://a.example/v1\", \"https://b.example\""),
            "Example: `\"https://a.example/v1\"`, `\"https://b.example\"`"
        );
        assert_eq!(escape("see `https://x` too"), "see `https://x` too");
    }
}
