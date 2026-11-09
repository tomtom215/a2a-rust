// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Refresh jitter: how much of a card's lifetime a draw takes off, as arithmetic
//! and over a socket, against the counting server of the parent module.

use std::time::Duration;

use super::super::{CachedCard, Freshness};
use super::{CachingCardResolver, SECOND, Server, card_json, elapse, ok};

/// The arithmetic, to the nanosecond: a 10 s card stored with a share of
/// 0.5 is fresh until 5 s after it arrived, and with no share until 10 s.
#[test]
fn jitter_brings_the_expiry_forward_by_its_share_of_the_lifetime() {
    let t0 = tokio::time::Instant::now();
    let mut headers = hyper::HeaderMap::new();
    headers.insert("cache-control", "max-age=10".parse().unwrap());
    let freshness = Freshness::of(&headers, t0, t0, std::time::SystemTime::now());
    let card: a2a_protocol_types::AgentCard = serde_json::from_str(&card_json()).unwrap();
    let cached = |jitter| CachedCard {
        card: card.clone(),
        etag: None,
        last_modified: None,
        freshness: freshness.clone(),
        jitter,
    };
    let resolver = CachingCardResolver::new("http://a.example").unwrap();
    assert_eq!(resolver.fresh_until(&cached(0.5)), Some(t0 + 5 * SECOND));
    assert_eq!(
        resolver.fresh_until(&cached(0.25)),
        Some(t0 + 7500 * Duration::from_millis(1))
    );
    assert_eq!(resolver.fresh_until(&cached(0.0)), Some(t0 + 10 * SECOND));
    let always = resolver.with_always_revalidate(true);
    assert_eq!(always.fresh_until(&cached(0.5)), None);
}

/// Over a socket: the largest draw takes the whole `fraction` off, so a 10 s
/// card with a jitter of 0.5 is revalidated after 5 s, not 10.
#[tokio::test]
async fn jitter_shortens_freshness_by_the_drawn_share() {
    let server = Server::start(ok(&[("cache-control", "max-age=10")])).await;
    let resolver = CachingCardResolver::new(&server.url)
        .unwrap()
        .with_refresh_jitter(0.5)
        .with_draw(|| u64::MAX);
    resolver.resolve().await.expect("first");
    elapse(4 * SECOND).await;
    resolver.resolve().await.expect("fresh");
    assert_eq!(server.requests(), 1);
    elapse(2 * SECOND).await;
    resolver.resolve().await.expect("revalidated");
    assert_eq!(server.requests(), 2);
}

/// The smallest draw changes nothing.
#[tokio::test]
async fn a_zero_draw_leaves_the_lifetime_whole() {
    // The server's own `Date` has one-second resolution, so a response arrives
    // up to a second old (RFC 9111 §4.2.3), which is all the margin 9 s of 10 s
    // leaves: the case failed on one run in about three hundred under load.
    // A `Date` ahead of now is an age of zero by the same section.
    let date: &'static str = Box::leak(
        httpdate::fmt_http_date(std::time::SystemTime::now() + 5 * SECOND).into_boxed_str(),
    );
    let fields: &'static [(&'static str, &'static str)] =
        Box::leak(Box::new([("cache-control", "max-age=10"), ("date", date)]));
    let server = Server::start(ok(fields)).await;
    let resolver = CachingCardResolver::new(&server.url)
        .unwrap()
        .with_refresh_jitter(0.5)
        .with_draw(|| 0);
    resolver.resolve().await.expect("first");
    elapse(9 * SECOND).await;
    resolver.resolve().await.expect("fresh");
    assert_eq!(server.requests(), 1);
}
