// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A restarted supervisor picks its child up again.

use super::harness::*;
use std::sync::Arc;

use a2a_protocol_client::delegation::{Delegation, Outcome};
use a2a_protocol_types::TaskState;

#[tokio::test]
async fn a_supervisor_that_restarted_reattaches_to_its_running_child() {
    let upstream = serve().await;
    let hold = key();

    let mut first = Delegation::start(client_for(upstream), job(&hold))
        .await
        .expect("start");
    let id = until_named(&mut first).await;
    let position = first.last_event_id().map(str::to_owned);
    assert_eq!(first.detach().as_deref(), Some(id.as_str()));

    // A new process: a new client, the recorded id and position.
    let again = Delegation::reattach(client_for(upstream), None, id.clone(), position)
        .await
        .expect("reattached");
    assert!(!again.is_settled());
    assert_eq!(again.task_id(), Some(id.as_str()));
    gate(&hold).notify_one();
    let done = tokio::time::timeout(GUARD, again.wait(std::future::pending()))
        .await
        .expect("bounded");
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
    assert_eq!(
        state_of(&client_for(upstream), &id).await,
        TaskState::Completed
    );
}

#[tokio::test]
async fn reattaching_to_a_child_that_has_settled_returns_it_settled() {
    let upstream = serve().await;
    let client = client_for(upstream);
    let done = Delegation::start(Arc::clone(&client), job("done"))
        .await
        .expect("start")
        .wait(std::future::pending())
        .await;
    assert!(matches!(done.outcome, Outcome::Completed(_)));
    let id = done.task_id.expect("named");

    let again = Delegation::reattach(Arc::clone(&client), None, id.clone(), None)
        .await
        .expect("reattached to a settled child");
    let settled = tokio::time::timeout(GUARD, again.wait(std::future::pending()))
        .await
        .expect("bounded");
    assert!(
        matches!(settled.outcome, Outcome::Completed(_)),
        "{settled:?}"
    );
    assert_eq!(state_of(&client, &id).await, TaskState::Completed);
}

#[tokio::test]
async fn reattaching_to_a_child_that_does_not_exist_fails_and_cancels_nothing() {
    let upstream = serve().await;
    let client = client_for(upstream);
    let err = Delegation::reattach(client, None, "no-such-task", None).await;
    assert!(err.is_err());
}
