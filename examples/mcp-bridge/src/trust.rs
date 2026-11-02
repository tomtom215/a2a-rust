// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the bridge does about a remote agent's card being untrusted input.
//!
//! Everything a card says reaches the MCP host's model: the skill text becomes
//! a tool's description, and the model reads tool descriptions as part of its
//! context. A hostile agent, or anyone in the path of a card fetched over
//! plain `http://`, can therefore write into the model's prompt, and the one
//! free-text argument every bridged tool takes is a ready channel back to the
//! same agent (audit SEC-16). The bridge cannot make the text trustworthy. It
//! can bound it, strip what hides it, say whose words it is, and refuse to
//! fetch a card in the clear from another machine.

/// The most characters of a remote agent's description carried into a tool.
pub const MAX_REMOTE_DESCRIPTION: usize = 400;

/// The most characters of a remote agent's name or a skill's id or name.
pub const MAX_REMOTE_NAME: usize = 80;

/// Environment variable that lets the bridge talk to a non-loopback `http://`
/// agent, for a network the operator trusts.
pub const ALLOW_PLAINTEXT: &str = "A2A_MCP_BRIDGE_ALLOW_PLAINTEXT";

/// Whether `c` is a character that hides text rather than showing it:
/// controls (newlines included), bidirectional overrides and isolates, and
/// zero-width characters. A prompt injection that must not be seen by a person
/// reading the tool list is written with these.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

/// Text a remote agent wrote, made fit to quote to a model: hidden characters
/// are dropped, whitespace runs become one space, a double quote becomes a
/// single quote (so the text cannot close the quotation it is placed in), and
/// the result is cut at `max` characters with an ellipsis.
#[must_use]
pub fn inert(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    let mut space = false;
    for c in text.chars() {
        // A newline or tab is whitespace to a reader; other controls are not.
        let c = if matches!(c, '\n' | '\r' | '\t') {
            ' '
        } else {
            c
        };
        if hidden(c) {
            continue;
        }
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            count += 1;
            space = false;
        }
        if count >= max {
            out.push('…');
            return out;
        }
        out.push(if c == '"' { '\'' } else { c });
        count += 1;
    }
    out
}

/// Whether `host` is this machine.
fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Refuses an `http://` agent URL whose host is not this machine, unless
/// [`ALLOW_PLAINTEXT`] is set to `1`.
///
/// # Errors
///
/// A message saying what was refused and how to allow it.
pub fn check_agent_url(url: &str, allow_plaintext: bool) -> Result<(), String> {
    let Ok(uri) = url.parse::<hyper::Uri>() else {
        return Err(format!("not a URL: {url}"));
    };
    let plaintext = uri
        .scheme_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("http"));
    if plaintext && !uri.host().is_some_and(is_loopback) && !allow_plaintext {
        return Err(format!(
            "refusing {url}: an http:// agent on another machine can be read and rewritten in \
             transit, and what its card says goes to your model as tool descriptions. Use \
             https://, or set {ALLOW_PLAINTEXT}=1 for a network you trust"
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "trust_tests.rs"]
mod tests;
