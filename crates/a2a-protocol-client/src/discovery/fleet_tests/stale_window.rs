// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How a stale-if-error window is counted, against the counting server of the
//! parent module.
//!
//! That server's `Date` has one-second resolution, so a response arrives up to a
//! second old (RFC 9111 §4.2.3), and with `max-age=1` that second comes off the
//! lifetime: the card went stale up to a second before `max-age` says, and its
//! window has run that much longer. The cases in the parent that expect a card to
//! be served left no margin for it and failed on about one run in a few hundred.

use std::time::SystemTime;

use super::{Mode, SECOND, elapse, stale_resolver};

/// A response that arrives with its lifetime already used up is stale from the
/// moment it arrives, so its window is counted from then, not from `max-age`
/// (RFC 9111 §4.2.3). The `Date` here is two seconds old, which makes certain
/// what the one-second resolution of the server's own `Date` makes a matter of
/// chance in the cases above (the flake this test is the regression test of).
#[tokio::test]
async fn a_response_that_arrives_stale_has_its_window_counted_from_arrival() {
    let date: &'static str =
        Box::leak(httpdate::fmt_http_date(SystemTime::now() - 2 * SECOND).into_boxed_str());
    let fields: &'static [(&'static str, &'static str)] =
        Box::leak(Box::new([("cache-control", "max-age=1"), ("date", date)]));
    let (_server, resolver) = stale_resolver(
        fields,
        Some(20 * SECOND),
        Mode::Status(503),
        // The helper adds the one second of `max-age`: 19.5 s in all.
        SECOND * 37 / 2,
    )
    .await;
    assert_eq!(
        resolver.resolve().await.expect("inside the window").name,
        "fleet"
    );
    elapse(SECOND).await;
    resolver
        .resolve()
        .await
        .expect_err("20.5 s after arrival, past a 20 s window; 20.5 s after max-age would not be");
}
