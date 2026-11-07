// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The small subset of YAML an Agent Skills frontmatter is written in.
//!
//! The reference parser of the specification reads strictyaml, a deliberately
//! restricted YAML: every scalar is a string and there are no anchors,
//! aliases, tags or flow collections. This reader does the same, and no more:
//! a top-level block mapping whose values are scalars (plain, single-quoted,
//! double-quoted, or a `|` or `>` block), one level of nested mapping (for
//! `metadata`), and a name for anything else. It has no recursion, so input
//! cannot nest it deep; no alias, so input cannot expand; and it is given a
//! block already bounded in size by the caller.
//!
//! What it does not read, it reports as [`Value::Unsupported`] against the
//! key it belongs to, and skips by indentation without looking inside, so an
//! unknown field in a style it does not read costs nothing. A scalar that
//! is malformed (a quote never closed, an unknown escape) is
//! [`Value::Malformed`], and again belongs to its key. Only a block whose
//! shape is lost (an indented first line, a line with no `key:`) is an error
//! for the whole of it.
//!
//! One leniency, which the specification's own client guide recommends: a
//! plain value that contains `: ` (`description: Use when: the user asks`)
//! is not valid YAML and is read as text, with [`Entry::lenient`] set.

#[path = "yaml_quoted.rs"]
mod quoted;

use quoted::scan_quoted;

/// What a top-level key holds, as far as this reader goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Value {
    /// A scalar of any style, read as a string.
    Text(String),
    /// A block mapping of scalars, one level deep.
    Map(Vec<(String, String)>),
    /// A construct this reader does not interpret, named.
    Unsupported(&'static str),
    /// A scalar that does not parse, with why.
    Malformed(&'static str),
}

/// One top-level `key: value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) key: String,
    pub(super) value: Value,
    /// The key's line within the block, counted from 1.
    pub(super) line: usize,
    /// Whether a plain value held `: ` and was read as text all the same.
    pub(super) lenient: bool,
}

/// A block whose shape is lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct YamlError {
    /// The line, counted from 1 within the block.
    pub(super) line: usize,
    pub(super) reason: &'static str,
}

/// Reads `block`, the text between the two `---` lines.
pub(super) fn parse(block: &str) -> Result<Vec<Entry>, YamlError> {
    let lines: Vec<&str> = block.lines().collect();
    let mut entries = Vec::new();
    let mut at = 0;
    while let Some(&line) = lines.get(at) {
        if is_blank_or_comment(line) {
            at += 1;
            continue;
        }
        let number = at + 1;
        if line.starts_with([' ', '\t']) {
            return Err(YamlError {
                line: number,
                reason: "an entry is indented",
            });
        }
        let Some((key, rest)) = split_entry(line) else {
            return Err(YamlError {
                line: number,
                reason: "a line is not a `key: value` entry",
            });
        };
        let end = entry_end(&lines, at + 1);
        let (value, lenient) = decode(rest, &lines[at + 1..end]);
        entries.push(Entry {
            key,
            value,
            line: number,
            lenient,
        });
        at = end;
    }
    Ok(entries)
}

fn is_blank_or_comment(line: &str) -> bool {
    let line = line.trim_start();
    line.is_empty() || line.starts_with('#')
}

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// The index after the lines that belong to the entry whose key line came
/// just before `from`: every blank or indented line.
fn entry_end(lines: &[&str], from: usize) -> usize {
    let mut end = from;
    while let Some(line) = lines.get(end) {
        if line.trim().is_empty() || line.starts_with([' ', '\t']) {
            end += 1;
        } else {
            break;
        }
    }
    end
}

/// `line` split at its key's colon: the key, and what follows the colon.
/// The colon must end the line or be followed by a space or tab, so a URL in
/// a plain key does not split it.
fn split_entry(line: &str) -> Option<(String, &str)> {
    if let Some(quote) = line.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        let (key, after) = scan_quoted(line, quote).ok()?;
        let rest = after.trim_start().strip_prefix(':')?;
        if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
            return None;
        }
        return Some((key, rest));
    }
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == ':'
            && chars
                .peek()
                .is_none_or(|&(_, next)| next == ' ' || next == '\t')
        {
            let key = line[..i].trim_end();
            if key.is_empty() || key == "-" || key.starts_with("- ") || key.starts_with("? ") {
                return None;
            }
            return Some((key.to_owned(), &line[i + 1..]));
        }
    }
    None
}

/// The value after a key, from the rest of its line and the lines under it.
fn decode(rest: &str, children: &[&str]) -> (Value, bool) {
    let rest = rest.trim();
    match rest.chars().next() {
        None | Some('#') => (nested(children), false),
        Some('|' | '>') => (block_scalar(rest, children), false),
        Some(quote @ ('"' | '\'')) => (quoted(rest, children, quote), false),
        Some('[' | '{') => (Value::Unsupported("a flow collection"), false),
        Some('&' | '*' | '!') => (Value::Unsupported("an anchor, an alias or a tag"), false),
        Some(_) => plain(rest, children),
    }
}

/// A value with nothing on its key's line: empty, a sequence (not read), a
/// block mapping of one-line scalars, or a scalar that starts on the line
/// below its key.
fn nested(children: &[&str]) -> Value {
    let Some(start) = children.iter().position(|line| !is_blank_or_comment(line)) else {
        return Value::Text(String::new());
    };
    let first = children[start];
    let trimmed = first.trim_start();
    if trimmed == "-" || trimmed.starts_with("- ") {
        return Value::Unsupported("a sequence");
    }
    if split_entry(trimmed).is_none() {
        // `description:` and the text on the next line: read as if the text
        // had followed the colon.
        return decode(trimmed.trim_end(), &children[start + 1..]).0;
    }
    let indent = leading_spaces(first);
    let mut pairs = Vec::new();
    let content = children[start..]
        .iter()
        .copied()
        .filter(|line| !is_blank_or_comment(line));
    for line in content {
        let entry = if indent == 0 || leading_spaces(line) != indent {
            None
        } else {
            split_entry(line.trim_start())
        };
        let Some((key, rest)) = entry else {
            return Value::Unsupported("a mapping that is nested, spans lines or is tab-indented");
        };
        let Value::Text(value) = decode(rest, &[]).0 else {
            return Value::Unsupported("a mapping value that is not a string");
        };
        pairs.push((key, value));
    }
    Value::Map(pairs)
}

/// Whether a plain value holds `: `, which YAML reads as a nested mapping.
fn has_colon_space(text: &str) -> bool {
    text.contains(": ") || text.contains(":\t") || text.ends_with(':')
}

/// `text` without a trailing ` #` comment.
fn strip_comment(text: &str) -> &str {
    let mut after_space = false;
    for (i, c) in text.char_indices() {
        if c == '#' && after_space {
            return text[..i].trim_end();
        }
        after_space = c == ' ' || c == '\t';
    }
    text
}

/// A plain scalar: its line, then each more-indented line folded on with one
/// space, or a line break per blank line between.
fn plain(first: &str, children: &[&str]) -> (Value, bool) {
    let mut text = strip_comment(first).to_owned();
    let mut lenient = has_colon_space(&text);
    let mut blanks = 0;
    for line in children {
        let line = line.trim();
        if line.is_empty() {
            blanks += 1;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let piece = strip_comment(line);
        lenient |= has_colon_space(piece);
        if blanks > 0 {
            text.extend(std::iter::repeat_n('\n', blanks));
        } else {
            text.push(' ');
        }
        text.push_str(piece);
        blanks = 0;
    }
    (Value::Text(text), lenient)
}

/// A single- or double-quoted scalar, which may run over several lines.
fn quoted(rest: &str, children: &[&str], quote: char) -> Value {
    let mut raw = rest.to_owned();
    for line in children {
        raw.push('\n');
        raw.push_str(line);
    }
    match scan_quoted(&raw, quote) {
        Err(reason) => Value::Malformed(reason),
        Ok((text, after)) => {
            let mut tail = after.lines();
            let closing_line_ends = tail.next().is_none_or(is_blank_or_comment);
            if closing_line_ends && tail.all(is_blank_or_comment) {
                Value::Text(text)
            } else {
                Value::Malformed("text follows the closing quote")
            }
        }
    }
}

/// What a block scalar does with the line breaks at its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chomp {
    /// `-`: none.
    Strip,
    /// The default: one.
    Clip,
    /// `+`: all.
    Keep,
}

/// The `-`, `+` and digit after a `|` or `>`, in either order.
fn block_header(token: &str) -> Option<(Chomp, Option<usize>)> {
    let mut chomp = None;
    let mut indent = None;
    for c in token.chars() {
        match c {
            '+' | '-' if chomp.is_none() => {
                chomp = Some(if c == '+' { Chomp::Keep } else { Chomp::Strip });
            }
            '1'..='9' if indent.is_none() => indent = c.to_digit(10).map(|d| d as usize),
            _ => return None,
        }
    }
    Some((chomp.unwrap_or(Chomp::Clip), indent))
}

/// A `|` (literal) or `>` (folded) block scalar whose header is `header`.
fn block_scalar(header: &str, children: &[&str]) -> Value {
    let folded = header.starts_with('>');
    let after = &header[1..];
    let (token, tail) = after.split_once([' ', '\t']).unwrap_or((after, ""));
    let parsed = block_header(token).filter(|_| is_blank_or_comment(tail));
    let Some((chomp, explicit)) = parsed else {
        return Value::Malformed("a block scalar's header is not valid");
    };
    let indent = explicit.or_else(|| {
        children
            .iter()
            .find(|line| !line.trim().is_empty())
            .map(|line| leading_spaces(line))
    });
    let Some(indent) = indent else {
        // Nothing but blank lines: only `+` keeps them.
        return Value::Text(chomped(String::new(), chomp, children.len(), false));
    };
    let mut lines = Vec::with_capacity(children.len());
    for line in children {
        if line.trim().is_empty() {
            lines.push("");
        } else if indent > 0 && leading_spaces(line) >= indent {
            lines.push(&line[indent..]);
        } else {
            return Value::Malformed("a block scalar has a line indented less than its first");
        }
    }
    let end = lines
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(0, |i| i + 1);
    let trailing = lines.len() - end;
    let body = if folded {
        fold(&lines[..end])
    } else {
        lines[..end].join("\n")
    };
    Value::Text(chomped(body, chomp, trailing, end > 0))
}

fn chomped(mut text: String, chomp: Chomp, trailing: usize, has_content: bool) -> String {
    match chomp {
        Chomp::Strip => {}
        Chomp::Clip => {
            if has_content {
                text.push('\n');
            }
        }
        Chomp::Keep => {
            let breaks = trailing + usize::from(has_content);
            text.extend(std::iter::repeat_n('\n', breaks));
        }
    }
    text
}

/// A folded scalar's lines joined: a single break between two ordinary lines
/// is a space, a blank line is a break, and a more-indented line keeps its
/// breaks.
fn fold(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut blanks = 0;
    let mut previous_indented = false;
    let mut started = false;
    for line in lines {
        if line.is_empty() {
            blanks += 1;
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        if started {
            let kept = previous_indented || indented;
            if blanks > 0 {
                out.extend(std::iter::repeat_n('\n', blanks + usize::from(kept)));
            } else if kept {
                out.push('\n');
            } else {
                out.push(' ');
            }
        }
        out.push_str(line);
        started = true;
        previous_indented = indented;
        blanks = 0;
    }
    out
}

#[cfg(test)]
#[path = "yaml_tests.rs"]
mod tests;
