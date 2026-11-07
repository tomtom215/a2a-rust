// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The quoted scalars of the YAML subset (see `yaml.rs`): single- and
//! double-quoted, with the escapes and the line folding YAML gives them.

use std::iter::Peekable;

const UNCLOSED: &str = "a quoted value is not closed";
const BAD_ESCAPE: &str = "a double-quoted value has an escape that is not valid";

/// The scalar `raw` opens with a `quote` and closes, and what follows its
/// closing quote. Line breaks fold as YAML folds them in a flow scalar: the
/// white space around a break goes, one break is a space, and each blank line
/// is a break.
pub(super) fn scan_quoted(raw: &str, quote: char) -> Result<(String, &str), &'static str> {
    let mut out = String::new();
    // `out`'s length after the last character that is not raw trailing
    // white space, which a line break trims.
    let mut keep = 0;
    let mut chars = raw.char_indices().skip(1).peekable();
    while let Some((i, c)) = chars.next() {
        if c == quote {
            if quote == '\'' && chars.next_if(|&(_, next)| next == '\'').is_some() {
                out.push('\'');
                keep = out.len();
                continue;
            }
            return Ok((out, &raw[i + 1..]));
        }
        match c {
            '\\' if quote == '"' => {
                let (_, escaped) = chars.next().ok_or(UNCLOSED)?;
                if let Some(decoded) = escape(escaped, &mut chars)? {
                    out.push(decoded);
                } else {
                    skip_blanks(&mut chars);
                }
                keep = out.len();
            }
            '\n' => {
                out.truncate(keep);
                fold_break(&mut out, &mut chars);
                keep = out.len();
            }
            _ => {
                out.push(c);
                if c != ' ' && c != '\t' {
                    keep = out.len();
                }
            }
        }
    }
    Err(UNCLOSED)
}

fn skip_blanks<I: Iterator<Item = (usize, char)>>(chars: &mut Peekable<I>) {
    while chars.next_if(|&(_, c)| c == ' ' || c == '\t').is_some() {}
}

/// Appends what the line break just read stands for: a space, or one line
/// break for each blank line after it.
fn fold_break<I: Iterator<Item = (usize, char)>>(out: &mut String, chars: &mut Peekable<I>) {
    let mut breaks = 0;
    loop {
        skip_blanks(chars);
        if chars.next_if(|&(_, c)| c == '\n').is_some() {
            breaks += 1;
        } else {
            break;
        }
    }
    if breaks == 0 {
        out.push(' ');
    } else {
        out.extend(std::iter::repeat_n('\n', breaks));
    }
}

/// The character after a backslash stands for; `None` for an escaped line
/// break, which stands for nothing.
fn escape<I: Iterator<Item = (usize, char)>>(
    escaped: char,
    chars: &mut I,
) -> Result<Option<char>, &'static str> {
    Ok(Some(match escaped {
        '0' => '\0',
        'a' => '\x07',
        'b' => '\x08',
        't' | '\t' => '\t',
        'n' => '\n',
        'v' => '\x0b',
        'f' => '\x0c',
        'r' => '\r',
        'e' => '\x1b',
        ' ' => ' ',
        '"' => '"',
        '/' => '/',
        '\\' => '\\',
        'N' => '\u{85}',
        '_' => '\u{a0}',
        'L' => '\u{2028}',
        'P' => '\u{2029}',
        'x' => hex(chars, 2)?,
        'u' => hex(chars, 4)?,
        'U' => hex(chars, 8)?,
        '\n' => return Ok(None),
        _ => return Err(BAD_ESCAPE),
    }))
}

fn hex<I: Iterator<Item = (usize, char)>>(
    chars: &mut I,
    digits: usize,
) -> Result<char, &'static str> {
    let mut value = 0_u32;
    for _ in 0..digits {
        let (_, c) = chars.next().ok_or(BAD_ESCAPE)?;
        // Eight digits fill a `u32` exactly, so this cannot overflow.
        value = value * 16 + c.to_digit(16).ok_or(BAD_ESCAPE)?;
    }
    char::from_u32(value).ok_or(BAD_ESCAPE)
}
