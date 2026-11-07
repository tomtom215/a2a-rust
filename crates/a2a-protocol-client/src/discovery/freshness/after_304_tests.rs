// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a `304` leaves of a stored response's freshness (RFC 9111 §4.3.4 and
//! §3.2): the caching fields it does not carry stay, the ones it carries
//! replace the stored ones whole, and `Date` and `Age` are the `304`'s own.

use std::time::{Duration, SystemTime};

use hyper::HeaderMap;
use tokio::time::Instant;

use super::Freshness;

const SECOND: Duration = Duration::from_secs(1);

fn headers(fields: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in fields {
        map.append(*name, value.parse().unwrap());
    }
    map
}

/// Reads `fields` as a response that took no time to arrive, at `t0`
/// on the monotonic clock and `wall` on the wall clock.
fn read(fields: &[(&'static str, &str)], t0: Instant, wall: SystemTime) -> Freshness {
    Freshness::of(&headers(fields), t0, t0, wall)
}

fn date(t: SystemTime) -> String {
    httpdate::fmt_http_date(t)
}

/// A wall clock on a whole second, as an HTTP-date can say it.
fn wall() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_hours(500_000)
}

/// `stored` freshened by a `304` with the fields `update`, which arrived
/// at `t0` on the monotonic clock and `wall` on the wall clock.
fn freshened(
    stored: &[(&'static str, &str)],
    update: &[(&'static str, &str)],
    t0: Instant,
    wall: SystemTime,
) -> Freshness {
    read(stored, t0, wall).after_304(&headers(update), t0, t0, wall)
}

/// RFC 9111 §4.3.4: a `304` that leaves a caching field out leaves the
/// stored one in force, with its lifetime and its stale-if-error window.
#[test]
fn a_304_without_cache_control_keeps_the_stored_one() {
    let (t0, wall) = (Instant::now(), wall());
    let after = freshened(
        &[("cache-control", "max-age=60, stale-if-error=300")],
        &[],
        t0,
        wall,
    );
    assert_eq!(after.fresh_until(None), Some(t0 + 60 * SECOND));
    assert_eq!(after.stale_window(None), Some(300 * SECOND));
}

/// `Date` and `Age` say when the message in hand was made and how old it
/// is: a `304` is dated by itself, never by the stored response. Carried
/// over, an hour-old `Date` would make a fresh `304` stale on arrival.
#[test]
fn a_304_does_not_inherit_the_stored_date_or_age() {
    let (t0, wall) = (Instant::now(), wall());
    let stored = [
        ("cache-control", "max-age=3600"),
        ("date", &date(wall - 3600 * SECOND)),
        ("age", "100"),
    ];
    // The control: as stored, the hour-old `Date` has used the lifetime up.
    assert_eq!(read(&stored, t0, wall).fresh_until(None), Some(t0));
    assert_eq!(
        freshened(&stored, &[], t0, wall).fresh_until(None),
        Some(t0 + 3600 * SECOND),
        "no Date, no Age on the 304: it arrived now"
    );
    assert_eq!(
        freshened(&stored, &[("age", "600")], t0, wall).fresh_until(None),
        Some(t0 + 3000 * SECOND),
        "the 304's own Age"
    );
    assert_eq!(
        freshened(&stored, &[("date", &date(wall - 60 * SECOND))], t0, wall).fresh_until(None),
        Some(t0 + 3540 * SECOND),
        "the 304's own Date"
    );
}

/// A field a `304` carries replaces the stored one whole (§3.2), not
/// directive by directive: its `max-age=10` also drops the stored
/// `must-revalidate` and `stale-if-error`.
#[test]
fn a_304s_cache_control_replaces_the_stored_one_whole() {
    let (t0, wall) = (Instant::now(), wall());
    let after = freshened(
        &[(
            "cache-control",
            "max-age=60, must-revalidate, stale-if-error=5",
        )],
        &[("cache-control", "max-age=10")],
        t0,
        wall,
    );
    assert_eq!(after.fresh_until(None), Some(t0 + 10 * SECOND));
    assert_eq!(after.stale_window(None), None, "the stored window is gone");
    assert_eq!(
        after.stale_window(Some(7 * SECOND)),
        Some(7 * SECOND),
        "and `must-revalidate` with it: a configured window applies"
    );
}

/// The stored `Expires` is an instant, so it stays; measured from the
/// `304`'s `Date` it leaves what is left of it.
#[test]
fn a_304_keeps_the_stored_expires_and_measures_it_from_its_own_date() {
    let (t0, wall) = (Instant::now(), wall());
    let stored = [("expires", date(wall + 100 * SECOND))];
    let stored: Vec<(&'static str, &str)> = stored.iter().map(|(n, v)| (*n, v.as_str())).collect();
    assert_eq!(
        freshened(&stored, &[("date", &date(wall))], t0, wall).fresh_until(None),
        Some(t0 + 100 * SECOND)
    );
    assert_eq!(
        freshened(&stored, &[("date", &date(wall + 40 * SECOND))], t0, wall).fresh_until(None),
        Some(t0 + 60 * SECOND),
        "a later Date leaves less of the same instant"
    );
}

#[test]
fn a_304_keeps_the_stored_vary_star_unless_it_names_another_field() {
    let (t0, wall) = (Instant::now(), wall());
    let stored = [("cache-control", "max-age=60"), ("vary", "*")];
    assert_eq!(freshened(&stored, &[], t0, wall).fresh_until(None), None);
    assert_eq!(
        freshened(&stored, &[("vary", "accept")], t0, wall).fresh_until(None),
        Some(t0 + 60 * SECOND)
    );
}

/// What one `304` leaves is what the next one updates.
#[test]
fn a_second_304_freshens_what_the_first_left() {
    let (t0, wall) = (Instant::now(), wall());
    let first = freshened(
        &[("cache-control", "max-age=60")],
        &[("cache-control", "max-age=30")],
        t0,
        wall,
    );
    let second = first.after_304(&headers(&[]), t0, t0, wall);
    assert_eq!(second.fresh_until(None), Some(t0 + 30 * SECOND));
}

/// A `304` that says `no-store` is not kept, whatever was stored.
#[test]
fn a_304_that_says_no_store_is_not_kept() {
    let (t0, wall) = (Instant::now(), wall());
    let after = freshened(
        &[("cache-control", "max-age=60")],
        &[("cache-control", "no-store")],
        t0,
        wall,
    );
    assert!(after.no_store);
}
