// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `parse_retry_after`: delta-seconds, the HTTP-date form against a fixed
//! clock, the one-hour ceiling, and what is neither.

use super::*;

// ── parse_retry_after ────────────────────────────────────────────────────
//
// This helper had no direct tests at all — every other test only ever set
// `retry_after: None` on the struct. `cargo mutants` replaced the whole
// function body with `Some(Duration::default())`, i.e. "retry immediately",
// and nothing failed. A client would then hammer a server that had explicitly
// asked it to wait, which is the one behaviour `Retry-After` exists to
// prevent.
//
// The gap was pre-existing and invisible until `error.rs` was split, because
// CI mutates changed *files* while the local run had been scoped to changed
// *lines*.

/// Builds a `HeaderMap` carrying a single `Retry-After` value.
fn retry_after_header(value: &str) -> hyper::HeaderMap {
    let mut headers = hyper::HeaderMap::new();
    headers.insert(
        hyper::header::RETRY_AFTER,
        hyper::header::HeaderValue::from_str(value).expect("test header value is ASCII"),
    );
    headers
}

/// The delay is the one the server asked for, not merely *a* delay.
#[test]
fn retry_after_delta_seconds_is_parsed_exactly() {
    assert_eq!(
        parse_retry_after(&retry_after_header("120")),
        Some(std::time::Duration::from_secs(120)),
        "the parsed delay must be the header's value; `Some(0)` would mean \
         retrying instantly against a server that asked for two minutes"
    );
}

/// An absurd value is clamped rather than honoured.
///
/// The clamp is a defence, not a nicety: without it a hostile or misconfigured
/// header parks a retry for as long as it likes.
#[test]
fn retry_after_is_clamped_to_one_hour() {
    assert_eq!(
        parse_retry_after(&retry_after_header("86400")),
        Some(std::time::Duration::from_secs(3600))
    );
    // Exactly at the ceiling is not clamped further.
    assert_eq!(
        parse_retry_after(&retry_after_header("3600")),
        Some(std::time::Duration::from_secs(3600))
    );
}

/// No header means no server-supplied delay — the caller falls back to its own
/// backoff, which is different from being told to wait zero seconds.
#[test]
fn retry_after_absent_is_none() {
    assert_eq!(parse_retry_after(&hyper::HeaderMap::new()), None);
}

/// A value that is neither delta-seconds nor a date yields no delay rather
/// than a wrong one.
#[test]
fn retry_after_that_is_neither_seconds_nor_a_date_is_none() {
    assert_eq!(parse_retry_after(&retry_after_header("not-a-number")), None);
    // Negative values are not delta-seconds either.
    assert_eq!(parse_retry_after(&retry_after_header("-5")), None);
    // A date that is not one: no thirty-second of October.
    assert_eq!(
        parse_retry_after(&retry_after_header("Wed, 32 Oct 2026 07:28:00 GMT")),
        None
    );
    assert_eq!(parse_retry_after(&retry_after_header("")), None);
}

const DATE: &str = "Wed, 21 Oct 2026 07:28:00 GMT";

fn at(date: &str) -> std::time::SystemTime {
    httpdate::parse_http_date(date).expect("a valid date")
}

/// The date form is the time left until it, to the second.
#[test]
fn retry_after_http_date_is_the_time_left_until_it() {
    let secs = std::time::Duration::from_secs;
    for before in [1, 90, 3599, 3600] {
        let now = at(DATE) - secs(before);
        assert_eq!(
            parse_retry_after_at(&retry_after_header(DATE), now),
            Some(secs(before)),
            "{before} s before the date"
        );
    }
    // Delta-seconds ignore the clock.
    assert_eq!(
        parse_retry_after_at(&retry_after_header("45"), at(DATE)),
        Some(secs(45))
    );
}

#[test]
fn retry_after_http_date_that_has_passed_is_zero_and_one_far_off_is_clamped() {
    let secs = std::time::Duration::from_secs;
    assert_eq!(
        parse_retry_after_at(&retry_after_header(DATE), at(DATE)),
        Some(secs(0)),
        "now is the date"
    );
    assert_eq!(
        parse_retry_after_at(&retry_after_header(DATE), at(DATE) + secs(1)),
        Some(secs(0)),
        "a second after"
    );
    assert_eq!(
        parse_retry_after_at(&retry_after_header(DATE), at(DATE) - secs(3601)),
        Some(secs(3600)),
        "a second over the hour"
    );
    assert_eq!(
        parse_retry_after_at(&retry_after_header(DATE), at(DATE) - secs(86_400)),
        Some(secs(3600)),
        "a day away"
    );
}

/// HTTP allows three date formats; a sender may use any of them.
#[test]
fn retry_after_http_date_is_read_in_all_three_formats() {
    let now = at(DATE) - std::time::Duration::from_secs(60);
    for form in [
        DATE,
        "Wednesday, 21-Oct-26 07:28:00 GMT",
        "Wed Oct 21 07:28:00 2026",
    ] {
        assert_eq!(
            parse_retry_after_at(&retry_after_header(form), now),
            Some(std::time::Duration::from_secs(60)),
            "{form}"
        );
    }
}

/// Surrounding whitespace is trimmed before parsing.
#[test]
fn retry_after_tolerates_surrounding_whitespace() {
    assert_eq!(
        parse_retry_after(&retry_after_header("  30  ")),
        Some(std::time::Duration::from_secs(30))
    );
}
