// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A cut stream is resumed, and the policy that bounds it.

use super::harness::*;
use std::time::Duration;

use a2a_protocol_client::delegation::{Delegation, Outcome, ResumePolicy};
use a2a_protocol_types::TaskState;

#[tokio::test]
async fn a_cut_stream_is_resumed_and_the_running_child_completes() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let (via, direct) = (client_for(proxy.addr), client_for(upstream));
    let hold = key();

    let mut d = Delegation::start(via, job(&hold)).await.expect("start");
    let id = until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));

    let before = proxy.accepted();
    proxy.cut();
    // The delegation comes back through the proxy while the child still runs.
    until("the delegation reconnected", || proxy.accepted() > before).await;
    assert_eq!(state_of(&direct, &id).await, TaskState::Working);
    gate(&hold).notify_one();

    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("bounded")
        .expect("joined");
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
    assert_eq!(
        state_of(&direct, &id).await,
        TaskState::Completed,
        "the child was never cancelled"
    );
}

#[tokio::test]
async fn a_child_that_finished_while_the_stream_was_down_is_reported_as_finished() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let (via, direct) = (client_for(proxy.addr), client_for(upstream));
    let hold = key();

    let mut d = Delegation::start(via, job(&hold)).await.expect("start");
    let id = until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));

    // Down: the child finishes where nobody can see it, then the link returns.
    proxy.block(true);
    proxy.cut();
    gate(&hold).notify_one();
    tokio::time::timeout(GUARD, async {
        while state_of(&direct, &id).await != TaskState::Completed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the child finished");
    proxy.block(false);

    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("bounded")
        .expect("joined");
    assert!(
        matches!(done.outcome, Outcome::Completed(_)),
        "a finished child is not Lost: {done:?}"
    );
    assert_eq!(state_of(&direct, &id).await, TaskState::Completed);
}

#[tokio::test]
async fn a_link_that_stays_down_is_given_up_on_after_the_policy_and_no_longer() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let (via, direct) = (client_for(proxy.addr), client_for(upstream));
    let hold = key();
    let policy = ResumePolicy::none()
        .with_attempts(3)
        .with_backoff(Duration::from_millis(100), Duration::from_millis(100))
        .with_deadline(Duration::from_secs(5));

    let mut d = Delegation::start(via, job(&hold))
        .await
        .expect("start")
        .with_resume(policy);
    let id = until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));

    proxy.block(true);
    let before = proxy.accepted();
    let cut_at = std::time::Instant::now();
    proxy.cut();
    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("bounded")
        .expect("joined");
    assert!(matches!(done.outcome, Outcome::Lost(_)), "{done:?}");
    // Three attempts wait before the second and the third: 2 x 100 ms. A
    // lower bound only; nothing about how fast a refused connection fails.
    assert!(
        cut_at.elapsed() >= Duration::from_millis(190),
        "the attempts were spaced by the backoff: {:?}",
        cut_at.elapsed()
    );
    // Three reopenings, a look at the task after each, and the cancel: a
    // handful of connections, not a loop.
    let tried = proxy.accepted() - before;
    assert!((3..=40).contains(&tried), "{tried} connections");
    // The link was down for the cancel too, so the child is still running.
    assert_eq!(state_of(&direct, &id).await, TaskState::Working);
    gate(&hold).notify_one();
}

#[tokio::test]
async fn the_deadline_ends_a_loss_that_the_attempts_alone_would_keep_working_on() {
    // Ten thousand attempts, 20 ms apart: only the 400 ms deadline stops this.
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let (via, direct) = (client_for(proxy.addr), client_for(upstream));
    let hold = key();
    let policy = ResumePolicy::none()
        .with_attempts(10_000)
        .with_backoff(Duration::from_millis(20), Duration::from_millis(20))
        .with_deadline(Duration::from_millis(400));

    let mut d = Delegation::start(via, job(&hold))
        .await
        .expect("start")
        .with_resume(policy);
    let id = until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));

    proxy.block(true);
    let before = proxy.accepted();
    let cut_at = std::time::Instant::now();
    proxy.cut();
    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("the deadline ended it")
        .expect("joined");
    let took = cut_at.elapsed();
    assert!(matches!(done.outcome, Outcome::Lost(_)), "{done:?}");
    assert!(
        took >= Duration::from_millis(300),
        "it worked on the loss: {took:?}"
    );
    assert!(
        took < Duration::from_secs(10),
        "and stopped at the deadline: {took:?}"
    );
    let tried = proxy.accepted() - before;
    assert!(
        tried < 500,
        "{tried} connections, not ten thousand attempts"
    );
    assert_eq!(state_of(&direct, &id).await, TaskState::Working);
    gate(&hold).notify_one();
}

#[tokio::test]
async fn a_link_that_keeps_delivering_is_resumed_past_the_attempt_limit() {
    // Two attempts in a row would be spent by three cuts. But the position
    // moves between the cuts, so each one starts the count again.
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let (via, direct) = (client_for(proxy.addr), client_for(upstream));
    let policy = ResumePolicy::default()
        .with_attempts(2)
        .with_backoff(Duration::from_millis(10), Duration::from_millis(50));

    let mut d = Delegation::start(via, job("tick:go"))
        .await
        .expect("start")
        .with_resume(policy);
    let id = until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(350)).await;
        proxy.cut();
    }
    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("bounded")
        .expect("joined");
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
    assert_eq!(state_of(&direct, &id).await, TaskState::Completed);
}

/// `last_event_id` is the position a resume or a reattach starts from, so it
/// has to be the stream's: absent before the server has numbered an event, then
/// one of its numbers, and larger after more events.
#[tokio::test]
async fn the_recorded_position_is_the_streams_and_it_advances() {
    let upstream = serve().await;
    let mut d = Delegation::start(client_for(upstream), job("tick:go"))
        .await
        .expect("start");
    assert_eq!(d.last_event_id(), None, "nothing received yet");
    let number = |d: &Delegation| d.last_event_id().and_then(|p| p.parse::<u64>().ok());
    let first = tokio::time::timeout(GUARD, async {
        loop {
            d.next_event()
                .await
                .expect("an event")
                .expect("not an error");
            if let Some(n) = number(&d) {
                return n;
            }
        }
    })
    .await
    .expect("the server numbered an event");
    let later = tokio::time::timeout(GUARD, async {
        loop {
            d.next_event()
                .await
                .expect("an event")
                .expect("not an error");
            if let Some(n) = number(&d).filter(|n| *n > first) {
                return n;
            }
        }
    })
    .await
    .expect("and numbered a later one");
    assert!(later > first, "{first} then {later}");
    let _ = d.detach();
}
