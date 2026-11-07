// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An endpoint that keeps failing is taken out of rotation, let back for one
//! probe when its ejection ends, and never are more than the allowed share
//! of the set out at once. Time moves between calls, never during one.

use std::time::Duration;

use a2a_protocol_client::balance::{BalancePolicy, Ejection, StaticEndpoints};
use a2a_protocol_client::{A2aClient, ClientBuilder, ClientError};

use super::harness::{Mode, Replica, counts, elapse, get, three, urls};

const SECOND: Duration = Duration::from_secs(1);

/// Two failures in a row eject, for 30 s, then 60 s.
fn client(replicas: &[std::sync::Arc<Replica>], percent: u8) -> A2aClient {
    ClientBuilder::new(replicas[0].url())
        .with_balancing(
            StaticEndpoints::new(urls(replicas)),
            BalancePolicy::new()
                .with_ejection(Ejection::new(2, 30 * SECOND, 300 * SECOND))
                .with_max_ejected_percent(percent),
        )
        .build()
        .unwrap()
}

/// `n` calls; the names that answered, and `!` for each error.
async fn calls(client: &A2aClient, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(get(client).await.unwrap_or_else(|_| "!".into()));
    }
    out
}

#[tokio::test]
async fn a_failing_replica_is_ejected_then_probed_back() {
    let replicas = three().await;
    replicas[1].set(Mode::Status(503));
    let client = client(&replicas, 50);
    // b fails twice in its turns and is ejected.
    assert_eq!(calls(&client, 6).await, ["a", "!", "c", "a", "!", "c"]);
    assert_eq!(calls(&client, 4).await, ["a", "c", "a", "c"]);
    assert_eq!(replicas[1].requests(), 2);

    // Still out a second before its 30 s end. (The clock also runs between
    // calls, by milliseconds; the arithmetic to the nanosecond is
    // `origin::tests`'.)
    elapse(29 * SECOND).await;
    assert_eq!(calls(&client, 2).await, ["a", "c"]);
    // At the end, one probe; it fails, and b is out for 60 s.
    elapse(SECOND).await;
    assert_eq!(calls(&client, 1).await, ["!"]);
    assert_eq!(calls(&client, 2).await, ["a", "c"]);
    elapse(59 * SECOND).await;
    assert_eq!(calls(&client, 2).await, ["a", "c"]);
    // b recovers; its probe succeeds and it is back in rotation.
    replicas[1].set(Mode::Ok);
    elapse(SECOND).await;
    assert_eq!(calls(&client, 1).await, ["b"]);
    assert_eq!(calls(&client, 3).await, ["a", "c", "b"]);
    assert_eq!(counts(&replicas), vec![8, 5, 8]);
}

/// A success between failures resets the count: failures must be in a row.
#[tokio::test]
async fn failures_must_be_consecutive() {
    let replicas = vec![Replica::start("a").await];
    let client = ClientBuilder::new(replicas[0].url())
        .with_balancing(
            StaticEndpoints::new(urls(&replicas)),
            BalancePolicy::new()
                .with_ejection(Ejection::new(2, 30 * SECOND, 300 * SECOND))
                .with_max_ejected_percent(100),
        )
        .build()
        .unwrap();
    for _ in 0..3 {
        replicas[0].set(Mode::Status(502));
        assert_eq!(calls(&client, 1).await, ["!"]);
        replicas[0].set(Mode::Ok);
        assert_eq!(calls(&client, 1).await, ["a"]);
    }
    // Two in a row: out, and with 100% allowed the call fails fast.
    replicas[0].set(Mode::Status(504));
    assert_eq!(calls(&client, 2).await, ["!", "!"]);
    let err = get(&client).await.expect_err("circuit open");
    match err {
        ClientError::CircuitOpen { retry_after, .. } => {
            // 30 s from the ejection, less the milliseconds since.
            let left = retry_after.expect("an ejection end");
            assert!(left <= 30 * SECOND && left > 29 * SECOND, "{left:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(replicas[0].requests(), 8, "the refused call was not sent");
}

/// An answer that is an error is still an answer: a `500`, unlike `503`,
/// says nothing against the replica.
#[tokio::test]
async fn an_error_answer_does_not_eject() {
    let replicas = three().await;
    replicas[1].set(Mode::Status(500));
    let client = client(&replicas, 50);
    assert_eq!(
        calls(&client, 9).await,
        ["a", "!", "c", "a", "!", "c", "a", "!", "c"]
    );
    assert_eq!(counts(&replicas), vec![3, 3, 3]);
}

/// With every replica failing, at most half (rounded down) are out at once:
/// the others stay in rotation, so a blip on all of them is not an outage.
#[tokio::test]
async fn no_more_than_the_allowed_share_is_out() {
    let replicas = three().await;
    for r in &replicas {
        r.set(Mode::Status(503));
    }
    let client = client(&replicas, 50);
    // Each fails twice and is ejected, but one of three is the most that
    // may be out: the two whose ejections end soonest stay in rotation.
    // a, b, c fail once each; a's second failure ejects it (one of three may
    // be out), b's ejects it too, so a, ending first, is back as a forced
    // choice; c's second ejects c, and a and b, ending first, stay in use.
    // From then on a and b take the calls in turn and c receives none.
    let _ = calls(&client, 12).await;
    assert_eq!(counts(&replicas), vec![5, 5, 2]);
    // Recovery is seen at once by a forced call.
    for r in &replicas {
        r.set(Mode::Ok);
    }
    // a (forced) succeeds and is back; b (forced, the one now out longest
    // past the share) succeeds and is back; a again. c stays out.
    assert_eq!(calls(&client, 3).await, ["a", "b", "a"]);
    assert_eq!(counts(&replicas), vec![7, 6, 2]);
}

/// Without ejection nothing is ever taken out.
#[tokio::test]
async fn without_ejection_a_failing_replica_keeps_its_turns() {
    let replicas = three().await;
    replicas[2].set(Mode::Status(503));
    let client = ClientBuilder::new(replicas[0].url())
        .with_balancing(
            StaticEndpoints::new(urls(&replicas)),
            BalancePolicy::new().without_ejection(),
        )
        .build()
        .unwrap();
    assert_eq!(
        calls(&client, 12)
            .await
            .iter()
            .filter(|a| *a == "!")
            .count(),
        4
    );
    assert_eq!(counts(&replicas), vec![4, 4, 4]);
}
