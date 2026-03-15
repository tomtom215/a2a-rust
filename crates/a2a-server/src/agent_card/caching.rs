// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! HTTP caching utilities for agent card responses (spec §8.3).
//!
//! Provides `ETag` generation, `Last-Modified` formatting, conditional
//! request checking, and `Cache-Control` configuration.

use std::fmt::Write;
use std::time::SystemTime;

// ── ETag ─────────────────────────────────────────────────────────────────────

/// Generates a weak `ETag` from the given bytes using a simple FNV-1a hash.
///
/// The hash is fast to compute and sufficient for cache validation of
/// relatively short agent card JSON payloads.
#[must_use]
pub fn make_etag(data: &[u8]) -> String {
    let hash = fnv1a(data);
    format!("W/\"{hash:016x}\"")
}

/// FNV-1a 64-bit hash (non-cryptographic, fast, good distribution).
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

// ── Last-Modified ────────────────────────────────────────────────────────────

/// Formats a [`SystemTime`] as an HTTP-date (RFC 7231 §7.1.1.1).
#[must_use]
pub fn format_http_date(time: SystemTime) -> String {
    let dur = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    // Simplified HTTP-date formatter (IMF-fixdate).
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Civil date from days since epoch (algorithm from Howard Hinnant).
    #[allow(clippy::cast_possible_wrap)]
    let (year, month, day) = civil_from_days(days as i64);

    // Day of week: Jan 1 1970 was a Thursday (4).
    let dow = ((days + 4) % 7) as usize;
    let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let mut buf = String::with_capacity(29);
    let _ = write!(
        buf,
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        day_names[dow],
        day,
        month_names[month as usize - 1],
        year,
        hours,
        minutes,
        seconds
    );
    buf
}

/// Converts days since Unix epoch to (year, month, day).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ── Conditional Requests ─────────────────────────────────────────────────────

/// Result of checking conditional request headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalResult {
    /// The client's cache is still valid; respond with 304.
    NotModified,
    /// The client needs the full response.
    SendFull,
}

/// Checks `If-None-Match` and `If-Modified-Since` headers against the
/// current `ETag` and `Last-Modified` values.
///
/// Per RFC 7232, `If-None-Match` takes precedence over `If-Modified-Since`.
#[must_use]
pub fn check_conditional(
    req: &hyper::Request<impl hyper::body::Body>,
    current_etag: &str,
    current_last_modified: &str,
) -> ConditionalResult {
    // Check If-None-Match first (takes precedence per RFC 7232 §6).
    if let Some(inm) = req.headers().get("if-none-match") {
        if let Ok(inm_str) = inm.to_str() {
            if etag_matches(inm_str, current_etag) {
                return ConditionalResult::NotModified;
            }
            // If-None-Match was present but didn't match; skip If-Modified-Since.
            return ConditionalResult::SendFull;
        }
    }

    // Check If-Modified-Since (only when If-None-Match is absent).
    if let Some(ims) = req.headers().get("if-modified-since") {
        if let Ok(ims_str) = ims.to_str() {
            if ims_str == current_last_modified {
                return ConditionalResult::NotModified;
            }
        }
    }

    ConditionalResult::SendFull
}

/// Checks whether any `ETag` in an `If-None-Match` header value matches
/// the current `ETag`.
///
/// Handles `*`, single `ETag` values, and comma-separated lists. Comparison
/// is performed using weak comparison (RFC 7232 §2.3.2).
fn etag_matches(header_value: &str, current: &str) -> bool {
    let header_value = header_value.trim();
    if header_value == "*" {
        return true;
    }
    // Strip W/ prefix for weak comparison.
    let current_bare = current.strip_prefix("W/").unwrap_or(current);

    for candidate in header_value.split(',') {
        let candidate = candidate.trim();
        let candidate_bare = candidate.strip_prefix("W/").unwrap_or(candidate);
        if candidate_bare == current_bare {
            return true;
        }
    }
    false
}

// ── Cache-Control config ─────────────────────────────────────────────────────

/// Configuration for `Cache-Control` headers on agent card responses.
#[derive(Debug, Clone, Copy)]
pub struct CacheConfig {
    /// `max-age` value in seconds.
    pub max_age: u32,
}

impl CacheConfig {
    /// Creates a config with the given `max-age`.
    #[must_use]
    pub const fn with_max_age(max_age: u32) -> Self {
        Self { max_age }
    }

    /// Returns the `Cache-Control` header value.
    #[must_use]
    pub fn header_value(&self) -> String {
        format!("public, max-age={}", self.max_age)
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        // Default: 1 hour.
        Self { max_age: 3600 }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use a2a_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
    use bytes::Bytes;
    use http_body_util::Full;

    /// Helper to build a minimal agent card for tests.
    pub fn minimal_agent_card() -> AgentCard {
        AgentCard {
            name: "Test Agent".into(),
            description: "A test agent".into(),
            version: "1.0.0".into(),
            supported_interfaces: vec![AgentInterface {
                url: "https://agent.example.com/rpc".into(),
                protocol_binding: "JSONRPC".into(),
                protocol_version: "1.0.0".into(),
                tenant: None,
            }],
            default_input_modes: vec!["text/plain".into()],
            default_output_modes: vec!["text/plain".into()],
            skills: vec![AgentSkill {
                id: "echo".into(),
                name: "Echo".into(),
                description: "Echoes input".into(),
                tags: vec!["echo".into()],
                examples: None,
                input_modes: None,
                output_modes: None,
                security_requirements: None,
            }],
            capabilities: AgentCapabilities::none(),
            provider: None,
            icon_url: None,
            documentation_url: None,
            security_schemes: None,
            security_requirements: None,
            signatures: None,
        }
    }

    #[test]
    fn make_etag_deterministic() {
        let data = b"hello world";
        let etag1 = make_etag(data);
        let etag2 = make_etag(data);
        assert_eq!(etag1, etag2);
        assert!(etag1.starts_with("W/\""));
        assert!(etag1.ends_with('"'));
    }

    #[test]
    fn make_etag_different_for_different_data() {
        let etag1 = make_etag(b"hello");
        let etag2 = make_etag(b"world");
        assert_ne!(etag1, etag2);
    }

    #[test]
    fn format_http_date_epoch() {
        let epoch = SystemTime::UNIX_EPOCH;
        let date = format_http_date(epoch);
        assert_eq!(date, "Thu, 01 Jan 1970 00:00:00 GMT");
    }

    #[test]
    fn etag_matches_exact() {
        assert!(etag_matches("W/\"abc\"", "W/\"abc\""));
    }

    #[test]
    fn etag_matches_wildcard() {
        assert!(etag_matches("*", "W/\"abc\""));
    }

    #[test]
    fn etag_matches_comma_list() {
        assert!(etag_matches("W/\"aaa\", W/\"bbb\", W/\"ccc\"", "W/\"bbb\""));
    }

    #[test]
    fn etag_no_match() {
        assert!(!etag_matches("W/\"xxx\"", "W/\"yyy\""));
    }

    #[test]
    fn check_conditional_if_none_match_hit() {
        let req = hyper::Request::builder()
            .header("if-none-match", "W/\"abc\"")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(
            check_conditional(&req, "W/\"abc\"", "Thu, 01 Jan 2026 00:00:00 GMT"),
            ConditionalResult::NotModified,
        );
    }

    #[test]
    fn check_conditional_if_none_match_miss() {
        let req = hyper::Request::builder()
            .header("if-none-match", "W/\"xyz\"")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(
            check_conditional(&req, "W/\"abc\"", "Thu, 01 Jan 2026 00:00:00 GMT"),
            ConditionalResult::SendFull,
        );
    }

    #[test]
    fn check_conditional_if_modified_since_match() {
        let lm = "Thu, 01 Jan 2026 00:00:00 GMT";
        let req = hyper::Request::builder()
            .header("if-modified-since", lm)
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(
            check_conditional(&req, "W/\"abc\"", lm),
            ConditionalResult::NotModified,
        );
    }

    #[test]
    fn check_conditional_no_headers() {
        let req = hyper::Request::builder()
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(
            check_conditional(&req, "W/\"abc\"", "Thu, 01 Jan 2026 00:00:00 GMT"),
            ConditionalResult::SendFull,
        );
    }

    #[test]
    fn cache_config_default() {
        let c = CacheConfig::default();
        assert_eq!(c.header_value(), "public, max-age=3600");
    }

    #[test]
    fn cache_config_custom() {
        let c = CacheConfig::with_max_age(600);
        assert_eq!(c.header_value(), "public, max-age=600");
    }
}
