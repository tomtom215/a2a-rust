// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! W3C Baggage on the A2A wire, and which propagators `OTEL_PROPAGATORS`
//! turns on (audit C-10; ADR 0038 §1).
//!
//! Baggage is a data channel to whoever the next hop is, and for an A2A agent
//! that is often another organisation's agent: an upstream service's
//! `user.id=` or `tenant=` would reach it. So nothing crosses by default, and
//! what does is chosen key by key ([`BaggagePolicy::Keys`]) or wholesale
//! ([`BaggagePolicy::All`]) by the operator, on the client that sends and on
//! the server that receives, separately.
//!
//! Bounds follow the W3C Baggage specification's limits (§3.3.2, "Limits":
//! a platform MUST propagate at least 64 list-members and 8192 bytes, and
//! MUST NOT propagate partial list-members): a header over [`MAX_BYTES`] is
//! dropped whole, and at most [`MAX_MEMBERS`] whole members are read or
//! written. Baggage is never made a metric attribute by this crate.

/// The header W3C Baggage travels in (§3.2, "Header Name").
pub const BAGGAGE_HEADER: &str = "baggage";

/// The most list-members read or written (W3C Baggage §3.3.2).
pub const MAX_MEMBERS: usize = 64;

/// The most bytes of a `baggage` header read or written (W3C Baggage §3.3.2).
pub const MAX_BYTES: usize = 8192;

/// Which baggage entries cross the A2A wire.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BaggagePolicy {
    /// None (the default).
    #[default]
    Off,
    /// Only entries whose key is one of these (compared exactly; baggage
    /// keys are case-sensitive).
    Keys(Vec<String>),
    /// Every entry, within the bounds.
    All,
}

impl BaggagePolicy {
    /// Only the entries with these keys.
    #[must_use]
    pub fn keys<K: Into<String>>(keys: impl IntoIterator<Item = K>) -> Self {
        Self::Keys(keys.into_iter().map(Into::into).collect())
    }

    /// Whether an entry with `key` crosses.
    #[must_use]
    pub fn allows(&self, key: &str) -> bool {
        match self {
            Self::Off => false,
            Self::Keys(keys) => keys.iter().any(|k| k == key),
            Self::All => true,
        }
    }

    /// Whether anything can cross.
    #[must_use]
    pub const fn is_off(&self) -> bool {
        matches!(self, Self::Off)
    }
}

/// One baggage entry: its key, its value (percent-decoded) and its
/// properties, as written after the first `;` (kept as they came).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// The key, an RFC 7230 token.
    pub key: String,
    /// The value, decoded.
    pub value: String,
    /// The properties, or `None`.
    pub properties: Option<String>,
}

/// Whether `key` is an RFC 7230 token, as W3C Baggage requires of a key.
fn is_token(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Whether `b` may appear in a value unencoded (W3C Baggage `baggage-octet`:
/// %x21 / %x23-2B / %x2D-3A / %x3C-5B / %x5D-7E).
const fn is_baggage_octet(b: u8) -> bool {
    matches!(b, 0x21 | 0x23..=0x2B | 0x2D..=0x3A | 0x3C..=0x5B | 0x5D..=0x7E) && b != b'%'
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = value.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn percent_encode(value: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if is_baggage_octet(b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The entries of a `baggage` header that `policy` lets in.
///
/// None when the header is over [`MAX_BYTES`], at most [`MAX_MEMBERS`], and
/// a member that is malformed (no `=`, a key that is not a token, a value that does not
/// decode to UTF-8) is skipped, not the header.
#[must_use]
pub fn parse(header: &str, policy: &BaggagePolicy) -> Vec<Member> {
    if policy.is_off() || header.len() > MAX_BYTES {
        return Vec::new();
    }
    header
        .split(',')
        .filter_map(|member| {
            let (pair, properties) = match member.split_once(';') {
                Some((pair, props)) => (pair, Some(props.trim().to_owned())),
                None => (member, None),
            };
            let (key, value) = pair.split_once('=')?;
            let key = key.trim();
            if !is_token(key) || !policy.allows(key) {
                return None;
            }
            Some(Member {
                key: key.to_owned(),
                value: percent_decode(value.trim())?,
                properties: properties.filter(|p| !p.is_empty()),
            })
        })
        .take(MAX_MEMBERS)
        .collect()
}

/// The `baggage` header for the entries `policy` lets out.
///
/// Whole members only, within [`MAX_MEMBERS`] and [`MAX_BYTES`]; `None` when
/// none does.
#[must_use]
pub fn encode<'a>(
    members: impl IntoIterator<Item = (&'a str, &'a str, Option<&'a str>)>,
    policy: &BaggagePolicy,
) -> Option<String> {
    let mut out = String::new();
    let mut count = 0;
    for (key, value, properties) in members {
        if count == MAX_MEMBERS {
            break;
        }
        if !is_token(key) || !policy.allows(key) {
            continue;
        }
        let mut member = format!("{key}={}", percent_encode(value));
        if let Some(props) = properties.filter(|p| !p.is_empty()) {
            member.push(';');
            member.push_str(props);
        }
        let added = member.len() + usize::from(!out.is_empty());
        if out.len() + added > MAX_BYTES {
            continue;
        }
        if !out.is_empty() {
            out.push(',');
        }
        out.push_str(&member);
        count += 1;
    }
    (!out.is_empty()).then_some(out)
}

/// Which propagators `OTEL_PROPAGATORS` turns on for the A2A wire.
///
/// The OpenTelemetry environment specification: a comma-separated list,
/// default `tracecontext,baggage`, `none` for none. This crate carries
/// `tracecontext` and `baggage`; any other name (`b3`, `b3multi`, `jaeger`,
/// `xray`, `ottrace`) has no implementation in the pinned OpenTelemetry
/// crates and is listed in [`unknown`](Self::unknown), and a list that names
/// only such propagators gets the default rather than nothing (decision F3: a
/// value that cannot be used warns and falls back).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Propagators {
    /// W3C Trace Context (`traceparent`, `tracestate`).
    pub tracecontext: bool,
    /// W3C Baggage.
    pub baggage: bool,
    /// Names given that are not carried, to warn about.
    pub unknown: Vec<String>,
}

impl Default for Propagators {
    fn default() -> Self {
        Self {
            tracecontext: true,
            baggage: true,
            unknown: Vec::new(),
        }
    }
}

impl Propagators {
    /// The variable's value read: `None` (unset) or empty is the default.
    #[must_use]
    pub fn from_env_value(value: Option<&str>) -> Self {
        let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
            return Self::default();
        };
        let mut set = Self {
            tracecontext: false,
            baggage: false,
            unknown: Vec::new(),
        };
        let mut none = false;
        for name in value.split(',').map(|n| n.trim().to_ascii_lowercase()) {
            match name.as_str() {
                "tracecontext" => set.tracecontext = true,
                "baggage" => set.baggage = true,
                "none" => none = true,
                "" => {}
                _ => set.unknown.push(name),
            }
        }
        if none {
            return Self {
                tracecontext: false,
                baggage: false,
                unknown: set.unknown,
            };
        }
        if !set.tracecontext && !set.baggage {
            return Self {
                unknown: set.unknown,
                ..Self::default()
            };
        }
        set
    }

    /// `OTEL_PROPAGATORS` from the process environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_env_value(std::env::var("OTEL_PROPAGATORS").ok().as_deref())
    }
}

#[cfg(test)]
mod tests;
