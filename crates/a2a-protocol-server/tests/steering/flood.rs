// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A flood of steering messages (ADR 0036): the rate limit, then the inbox.

use super::*;

/// The rate limit sits in front of the inbox: a flood is refused by it before
/// it reaches the inbox, which holds what got through and nothing more.
#[tokio::test]
async fn a_flood_meets_the_rate_limit_before_it_reaches_the_inbox() {
    let shared = Arc::new(Shared::default());
    // Four requests an hour: the task's own send, then three more.
    let limiter = RateLimitInterceptor::new(
        RateLimitConfig::default()
            .with_requests_per_window(4)
            .with_window(Duration::from_secs(3600)),
    )
    .unwrap();
    let handler = builder(&shared).with_interceptor(limiter).build().unwrap();
    let addr = serve(Binding::Rest, handler).await;
    let task = start(Binding::Rest, addr, &shared, "block", &[]).await;

    let mut statuses = Vec::new();
    for i in 0..8 {
        let (status, _, _) = steer(Binding::Rest, addr, &task, &format!("m{i}"), &[]).await;
        statuses.push(status);
    }
    assert_eq!(
        statuses,
        [200, 200, 200, 429, 429, 429, 429, 429],
        "three got through, the rest were throttled"
    );
    shared.release.notify_one();
}
