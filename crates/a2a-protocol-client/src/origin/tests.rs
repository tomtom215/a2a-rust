// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The per-origin state machine under a paused clock, to the nanosecond.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use super::{Origins, Outcome, Standing, Ticket, ejection_length, origin_key, outcome_of};
use crate::balance::Ejection;
use crate::error::ClientError;
use crate::retry::RetryBudget;

const SECOND: Duration = Duration::from_secs(1);
const NS: Duration = Duration::from_nanos(1);

fn policy() -> Ejection {
    Ejection::new(3, 10 * SECOND, 35 * SECOND)
}

#[test]
fn ejections_double_up_to_the_cap() {
    let p = policy();
    assert_eq!(ejection_length(p, 0), 10 * SECOND);
    assert_eq!(ejection_length(p, 1), 10 * SECOND);
    assert_eq!(ejection_length(p, 2), 20 * SECOND);
    assert_eq!(ejection_length(p, 3), 35 * SECOND);
    assert_eq!(ejection_length(p, 40), 35 * SECOND);
    assert_eq!(ejection_length(p, u32::MAX), 35 * SECOND);
    let huge = Ejection::new(1, Duration::MAX / 2, Duration::MAX);
    assert_eq!(ejection_length(huge, 3), Duration::MAX);
}

#[test]
fn what_says_the_origin_failed() {
    let status = |status| ClientError::UnexpectedStatus {
        status,
        body: String::new(),
        retry_after: None,
    };
    assert_eq!(outcome_of::<()>(&Ok(())), Outcome::Success);
    for e in [
        ClientError::HttpClient("refused".into()),
        ClientError::Timeout("t".into()),
        status(502),
        status(503),
        status(504),
    ] {
        assert_eq!(outcome_of::<()>(&Err(e)), Outcome::Failure);
    }
    for e in [
        status(500),
        status(429),
        status(404),
        ClientError::Transport("too large".into()),
        ClientError::InvalidAgentResponse("x".into()),
    ] {
        assert_eq!(outcome_of::<()>(&Err(e)), Outcome::Success);
    }
    for e in [
        ClientError::TooManyPendingRequests { limit: 1 },
        ClientError::CircuitOpen {
            origin: "o".into(),
            retry_after: None,
        },
        ClientError::Config("c".into()),
        ClientError::InvalidEndpoint("e".into()),
        ClientError::InsecureTransport("t".into()),
        ClientError::InsecureWebhook("w".into()),
        ClientError::InvalidAuthScheme("s".into()),
        ClientError::CapabilityNotDeclared {
            method: "m".into(),
            capability: "streaming",
        },
    ] {
        assert_eq!(outcome_of::<()>(&Err(e)), Outcome::Neutral);
    }
}

#[test]
fn an_origin_is_scheme_host_and_port() {
    assert_eq!(
        origin_key("https://User:pw@Agent.Example/a2a?k=1", None).as_deref(),
        Some("https://agent.example:443")
    );
    assert_eq!(
        origin_key("http://a.test", None).as_deref(),
        Some("http://a.test:80")
    );
    assert_eq!(
        origin_key("http://a.test:8080/x", None).as_deref(),
        Some("http://a.test:8080")
    );
    let addr = "10.0.0.7:8443".parse().unwrap();
    assert_eq!(
        origin_key("https://svc.test:8443", Some(addr)).as_deref(),
        Some("https://svc.test:8443 via 10.0.0.7:8443")
    );
    assert_eq!(origin_key("/relative", None), None);
    assert_eq!(origin_key("not a url", None), None);
}

/// Fails `n` normal calls.
fn fail(state: &Arc<super::OriginState>, n: u32) {
    for _ in 0..n {
        state
            .enter(Ticket::Normal)
            .unwrap()
            .record(Outcome::Failure);
    }
}

#[tokio::test(start_paused = true)]
async fn consecutive_failures_eject_and_a_probe_decides() {
    let origins = Origins::new(Some(policy()), None, None);
    let s = origins.get("http://a.test:80");
    fail(&s, 2);
    s.enter(Ticket::Normal).unwrap().record(Outcome::Success);
    fail(&s, 2);
    assert_eq!(s.admit(Instant::now()).unwrap(), Ticket::Normal, "not yet");
    let t0 = Instant::now();
    fail(&s, 1);
    assert_eq!(s.ejected_until(), Some(t0 + 10 * SECOND));
    assert!(s.is_ejected(t0));

    tokio::time::advance(Duration::from_nanos(9_999_999_999)).await;
    match s.admit(Instant::now()) {
        Err(ClientError::CircuitOpen {
            origin,
            retry_after,
        }) => {
            assert_eq!(origin, "http://a.test:80");
            assert_eq!(retry_after, Some(NS));
        }
        other => panic!("{other:?}"),
    }
    tokio::time::advance(NS).await;
    let now = Instant::now();
    assert!(!s.is_ejected(now), "ended, and no probe yet");
    assert_eq!(s.standing(now), Standing::Available(Ticket::Probe));
    // The slot is taken: a second caller is refused, without an end time.
    assert_eq!(s.standing(now), Standing::Ejected(None));
    assert!(s.is_ejected(now));
    assert!(matches!(
        s.admit(now),
        Err(ClientError::CircuitOpen {
            retry_after: None,
            ..
        })
    ));
    // The probe fails: the second ejection lasts twice as long.
    s.enter(Ticket::Probe).unwrap().record(Outcome::Failure);
    assert_eq!(s.ejected_until(), Some(now + 20 * SECOND));
    tokio::time::advance(20 * SECOND).await;
    assert_eq!(
        s.standing(Instant::now()),
        Standing::Available(Ticket::Probe)
    );
    s.enter(Ticket::Probe).unwrap().record(Outcome::Success);
    assert_eq!(s.ejected_until(), None);
    let snap = s.snapshot(Instant::now());
    assert_eq!(
        (snap.consecutive_failures, snap.consecutive_ejections),
        (0, 0)
    );
}

#[tokio::test(start_paused = true)]
async fn forced_calls_close_on_success_and_change_nothing_on_failure() {
    let origins = Origins::new(Some(policy()), None, None);
    let s = origins.get("o");
    fail(&s, 3);
    let until = s.ejected_until();
    s.enter(Ticket::Forced).unwrap().record(Outcome::Failure);
    assert_eq!(s.ejected_until(), until);
    s.enter(Ticket::Normal).unwrap().record(Outcome::Failure);
    assert_eq!(
        s.ejected_until(),
        until,
        "an ejected origin is not ejected again"
    );
    assert_eq!(s.snapshot(Instant::now()).consecutive_failures, 0);
    s.enter(Ticket::Forced).unwrap().record(Outcome::Success);
    assert_eq!(s.ejected_until(), None);
}

/// A probe's guard is recorded once. A stream opened as the probe keeps its
/// guard after recording success; if the origin is ejected again and a new
/// probe takes the slot, dropping the old stream must not free that slot.
#[tokio::test(start_paused = true)]
async fn a_recorded_probe_dropped_later_leaves_a_newer_probe_alone() {
    let origins = Origins::new(Some(policy()), None, None);
    let s = origins.get("o");
    fail(&s, 3);
    tokio::time::advance(10 * SECOND).await;
    assert_eq!(
        s.standing(Instant::now()),
        Standing::Available(Ticket::Probe)
    );
    let mut stream = s.enter(Ticket::Probe).unwrap();
    stream.record(Outcome::Success);
    fail(&s, 3);
    tokio::time::advance(20 * SECOND).await;
    assert_eq!(
        s.standing(Instant::now()),
        Standing::Available(Ticket::Probe)
    );
    drop(stream);
    assert_eq!(
        s.standing(Instant::now()),
        Standing::Ejected(None),
        "still taken"
    );
}

#[tokio::test(start_paused = true)]
async fn a_probe_that_learns_nothing_frees_its_slot() {
    let origins = Origins::new(Some(policy()), None, None);
    let s = origins.get("o");
    fail(&s, 3);
    tokio::time::advance(10 * SECOND).await;
    assert_eq!(
        s.standing(Instant::now()),
        Standing::Available(Ticket::Probe)
    );
    // Dropped unrecorded: cancelled.
    drop(s.enter(Ticket::Probe).unwrap());
    assert_eq!(
        s.standing(Instant::now()),
        Standing::Available(Ticket::Probe)
    );
    s.enter(Ticket::Probe).unwrap().record(Outcome::Neutral);
    assert_eq!(
        s.standing(Instant::now()),
        Standing::Available(Ticket::Probe)
    );
    s.release_probe();
    assert_eq!(
        s.standing(Instant::now()),
        Standing::Available(Ticket::Probe)
    );
    assert!(s.ejected_until().is_some(), "still not closed");
}

#[tokio::test(start_paused = true)]
async fn without_a_policy_nothing_is_ejected() {
    let origins = Origins::new(None, None, None);
    assert!(!origins.enforces());
    let s = origins.get("o");
    fail(&s, 100);
    assert_eq!(s.ejected_until(), None);
    assert_eq!(s.admit(Instant::now()).unwrap(), Ticket::Normal);
}

#[tokio::test]
async fn the_cap_refuses_past_its_limit_and_hands_back_a_probe() {
    let origins = Origins::new(Some(policy()), Some(1), None);
    assert!(origins.enforces());
    let s = origins.get("o");
    let first = s.enter(Ticket::Normal).unwrap();
    assert_eq!(s.in_flight(), 1);
    assert!(matches!(
        s.enter(Ticket::Normal),
        Err(ClientError::TooManyPendingRequests { limit: 1 })
    ));
    assert_eq!(s.in_flight(), 1, "a refused call is not in flight");
    drop(first);
    assert_eq!(s.in_flight(), 0);
    // A released permit keeps the call in flight but frees the place.
    let mut stream = s.enter(Ticket::Normal).unwrap();
    stream.release_permit();
    let second = s.enter(Ticket::Normal).unwrap();
    assert_eq!(s.in_flight(), 2);
    drop((stream, second));
}

#[tokio::test(start_paused = true)]
async fn a_probe_refused_by_the_cap_hands_its_slot_back() {
    let origins = Origins::new(Some(policy()), Some(1), None);
    let s = origins.get("o");
    fail(&s, 3);
    let holder = s.enter(Ticket::Forced).unwrap();
    tokio::time::advance(10 * SECOND).await;
    assert_eq!(
        s.standing(Instant::now()),
        Standing::Available(Ticket::Probe)
    );
    assert!(s.enter(Ticket::Probe).is_err());
    drop(holder);
    assert_eq!(
        s.standing(Instant::now()),
        Standing::Available(Ticket::Probe)
    );
}

#[test]
fn origins_are_shared_by_key_and_snapshotted_in_order() {
    let origins = Origins::new(None, None, Some(RetryBudget::new(0.1, 0.0)));
    assert!(origins.has_budget());
    assert!(!origins.enforces());
    let b = origins.get("http://b.test:80");
    let a = origins.get("http://a.test:80");
    assert!(Arc::ptr_eq(&a, &origins.get("http://a.test:80")));
    assert!(a.budget().is_some());
    let _guard = b.enter(Ticket::Normal).unwrap();
    let snap = origins.snapshot();
    assert_eq!(snap.len(), 2);
    assert_eq!(snap[0].origin, "http://a.test:80");
    assert_eq!(snap[1].in_flight, 1);
    assert_eq!(snap[1].ejected_for, None);
    assert!(Origins::new(None, None, None).get("o").budget().is_none());
}

#[test]
fn idle_healthy_origins_are_forgotten_past_the_bound() {
    let origins = Origins::new(Some(policy()), None, None);
    let held = origins.get("held");
    let ejected = origins.get("ejected");
    fail(&ejected, 3);
    drop(ejected);
    for i in 0..super::PRUNE_ABOVE - 1 {
        drop(origins.get(&format!("o{i}")));
    }
    // The bound was reached on the last insert: everything idle and healthy
    // went, the held and the ejected stayed, and the new one is in.
    let left: Vec<String> = origins.snapshot().into_iter().map(|s| s.origin).collect();
    assert_eq!(left.len(), 3, "{left:?}");
    assert!(left.contains(&"held".to_owned()) && left.contains(&"ejected".to_owned()));
    drop(held);
}

#[test]
fn debug_names_the_origin_and_its_state() {
    let origins = Origins::new(Some(policy()), Some(2), None);
    let s = origins.get("http://a.test:80");
    let guard = s.enter(Ticket::Probe).unwrap();
    assert_eq!(
        format!("{guard:?}"),
        "CallGuard { origin: \"http://a.test:80\", ticket: Probe, recorded: false, .. }"
    );
    let text = format!("{s:?}");
    assert!(
        text.starts_with(
            "OriginState { origin: \"http://a.test:80\", in_flight: 1, health: Health { failures: 0"
        ),
        "{text}"
    );
    drop(guard);
    let text = format!("{origins:?}");
    assert!(
        text.contains("cap: Some(2)")
            && text.contains(
                "origins: [OriginSnapshot { origin: \"http://a.test:80\", in_flight: 0, \
                 ejected_for: None, consecutive_failures: 0, consecutive_ejections: 0 }]"
            ),
        "{text}"
    );
}

/// A stream's guard gives back its place under the cap once the stream has
/// opened, and counts the stream in flight until it is dropped.
#[tokio::test]
async fn an_open_stream_frees_its_place_and_counts_until_dropped() {
    let origins = Origins::new(None, Some(1), None);
    let s = origins.get("o");
    let (_tx, rx) = tokio::sync::mpsc::channel(1);
    let stream = super::transport::stream(s.enter(Ticket::Normal).unwrap(), async {
        Ok(crate::streaming::EventStream::from_event_channel(rx))
    })
    .await
    .unwrap();
    let call = s.enter(Ticket::Normal).expect("the stream's place is free");
    assert_eq!(s.in_flight(), 2);
    drop(call);
    drop(stream);
    assert_eq!(s.in_flight(), 0);
}
