// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `A2aClient::follow_task` reopens a cut subscription from the last event id
//! and falls back to polling when that does not work (audit D-11).
//!
//! The server is this repository's, over real sockets, behind the proxy the
//! delegation tests use: it severs every open connection (`cut`) and refuses
//! new ones (`block`), as a rollout or an L4 reset does to a long-lived SSE
//! stream. Each test asserts what the follow reported and how it got there.

// The harness is shared with `delegation_resume`, which uses all of it.
#[allow(dead_code)]
#[path = "delegation_resume/harness.rs"]
mod harness;

use std::net::SocketAddr;
use std::time::Duration;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::delegation::ResumePolicy;
use a2a_protocol_client::methods::follow::TaskFollow;
use a2a_protocol_types::TaskState;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::responses::SendMessageResponse;
use harness::*;

/// Starts a task that holds until `text`'s gate opens, without waiting for it.
async fn start(upstream: SocketAddr, text: &str) -> String {
    let client = ClientBuilder::new(format!("http://{upstream}"))
        .with_return_immediately(true)
        .build()
        .expect("client");
    match client.send_message(job(text)).await.expect("sent") {
        SendMessageResponse::Task(task) => task.id.0.clone(),
        other => panic!("expected a task, got {other:?}"),
    }
}

fn state(event: &StreamResponse) -> Option<TaskState> {
    match event {
        StreamResponse::Task(t) => Some(t.status.state),
        StreamResponse::StatusUpdate(u) => Some(u.status.state),
        _ => None,
    }
}

async fn next(follow: &mut TaskFollow<'_>) -> Option<StreamResponse> {
    tokio::time::timeout(GUARD, follow.next())
        .await
        .expect("the follow neither stalled nor outlived the task")
        .map(|item| item.expect("no error"))
}

/// Reads to the end, returning the states seen.
async fn to_the_end(follow: &mut TaskFollow<'_>) -> Vec<TaskState> {
    let mut states = Vec::new();
    while let Some(event) = next(follow).await {
        states.extend(state(&event));
    }
    states
}

fn quick() -> ResumePolicy {
    ResumePolicy::default()
        .with_backoff(Duration::from_millis(10), Duration::from_millis(50))
        .with_jitter(false)
}

#[tokio::test]
async fn a_cut_subscription_is_reopened_and_the_follow_goes_on_to_the_end() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let hold = key();
    let id = start(upstream, &hold).await;

    let via = client_for(proxy.addr);
    let mut follow = via
        .follow_task(id, Duration::from_millis(20))
        .await
        .expect("follow")
        .with_resume(quick());
    assert!(!follow.is_polling());
    let first = next(&mut follow).await.expect("the task as it stands");
    assert_eq!(state(&first), Some(TaskState::Working));

    let before = proxy.accepted();
    proxy.cut();
    // The task ends only once the follow has reopened its subscription: a
    // task that ended first cannot be subscribed to, and is polled (the next
    // test).
    let reopened = async {
        until("a new connection was opened after the cut", || {
            proxy.accepted() > before
        })
        .await;
        gate(&hold).notify_one();
    };
    let (states, ()) = tokio::join!(to_the_end(&mut follow), reopened);
    assert_eq!(states.last(), Some(&TaskState::Completed), "{states:?}");
    assert!(!follow.is_polling(), "it stayed on a subscription");
}

#[tokio::test]
async fn a_policy_that_never_resumes_hands_over_to_polling_at_once() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let hold = key();
    let id = start(upstream, &hold).await;

    let via = client_for(proxy.addr);
    let mut follow = via
        .follow_task(id, Duration::from_millis(20))
        .await
        .expect("follow")
        .with_resume(ResumePolicy::none());
    next(&mut follow).await.expect("the task as it stands");

    let before = proxy.accepted();
    proxy.cut();
    gate(&hold).notify_one();
    let states = to_the_end(&mut follow).await;
    assert_eq!(states.last(), Some(&TaskState::Completed), "{states:?}");
    assert!(
        follow.is_polling(),
        "it polled, since it may not resubscribe"
    );
    // Polling is one `GetTask` per update, at most a few: no resubscribing.
    assert!(
        proxy.accepted() - before <= 6,
        "{}",
        proxy.accepted() - before
    );
}

#[tokio::test]
async fn a_task_that_ended_while_the_link_was_down_is_reported_and_the_follow_ends() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let hold = key();
    let id = start(upstream, &hold).await;

    let via = client_for(proxy.addr);
    let mut follow = via
        .follow_task(id.clone(), Duration::from_millis(20))
        .await
        .expect("follow")
        .with_resume(quick());
    next(&mut follow).await.expect("the task as it stands");

    // The link goes down and the task ends on the server meanwhile.
    proxy.block(true);
    proxy.cut();
    gate(&hold).notify_one();
    let direct = client_for(upstream);
    tokio::time::timeout(GUARD, async {
        while state_of(&direct, &id).await != TaskState::Completed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the task finished on the server");
    proxy.block(false);

    let states = to_the_end(&mut follow).await;
    assert_eq!(states.last(), Some(&TaskState::Completed), "{states:?}");
    assert!(
        follow.is_polling(),
        "a finished task cannot be subscribed to"
    );
    assert!(next(&mut follow).await.is_none(), "and the follow is over");
}

#[tokio::test]
async fn a_link_that_stays_down_is_given_up_on_after_the_policy_and_polling_reports_the_failure() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let hold = key();
    let id = start(upstream, &hold).await;

    let via = client_for(proxy.addr);
    let policy = ResumePolicy::none()
        .with_attempts(3)
        .with_backoff(Duration::from_millis(100), Duration::from_millis(100))
        .with_deadline(Duration::from_secs(5))
        .with_jitter(false);
    let mut follow = via
        .follow_task(id, Duration::from_millis(20))
        .await
        .expect("follow")
        .with_resume(policy);
    next(&mut follow).await.expect("the task as it stands");

    proxy.block(true);
    let before = proxy.accepted();
    let cut_at = std::time::Instant::now();
    proxy.cut();
    let item = tokio::time::timeout(GUARD, follow.next())
        .await
        .expect("bounded")
        .expect("an item");
    assert!(item.is_err(), "the first poll fails on a link that is down");
    assert!(follow.is_polling());
    // Three attempts wait before the second and the third: 2 x 100 ms.
    assert!(
        cut_at.elapsed() >= Duration::from_millis(190),
        "{:?}",
        cut_at.elapsed()
    );
    // Three reopenings and the one poll.
    let tried = proxy.accepted() - before;
    assert!((4..=12).contains(&tried), "{tried} connections");
    gate(&hold).notify_one();
}

#[tokio::test]
async fn a_subscription_that_keeps_delivering_is_reopened_past_the_attempt_limit() {
    // Two attempts in a row would be spent by three cuts, but each reopened
    // stream delivers events before the next cut, so the count starts again.
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let id = start(upstream, "tick:go").await;

    let via = client_for(proxy.addr);
    let mut follow = via
        .follow_task(id, Duration::from_millis(20))
        .await
        .expect("follow")
        .with_resume(quick().with_attempts(2));
    let mut states = Vec::new();
    let mut cuts = 0;
    while let Some(event) = next(&mut follow).await {
        states.extend(state(&event));
        if cuts < 3 && matches!(event, StreamResponse::ArtifactUpdate(_)) {
            cuts += 1;
            proxy.cut();
        }
    }
    assert_eq!(cuts, 3);
    assert_eq!(states.last(), Some(&TaskState::Completed), "{states:?}");
    assert!(!follow.is_polling());
}
