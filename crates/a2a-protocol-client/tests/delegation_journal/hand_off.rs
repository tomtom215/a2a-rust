// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A supervisor shut down on purpose hands its children over instead of
//! cancelling them (audit NEW-h-s0-2, fork F5), and a restarted one picks them
//! up from their records.

use std::sync::Arc;

use a2a_protocol_client::A2aClient;
use a2a_protocol_client::delegation::{Delegation, DelegationOptions, Outcome};
use a2a_protocol_types::delegation_journal::DelegationJournal;
use a2a_protocol_types::{TaskQueryParams, TaskState};

use super::harness::*;

async fn state_of(client: &A2aClient, id: &str) -> TaskState {
    client
        .get_task(TaskQueryParams::new(id))
        .await
        .expect("get_task")
        .status
        .state
}

/// What a graceful shutdown does: the parent's cancellation token fires and so
/// does the supervisor's own signal. Both futures are ready.
#[tokio::test]
async fn a_journaled_child_is_left_running_and_its_record_open_when_the_supervisor_hands_it_off() {
    let client = client_for(serve().await);
    let journal = Probe::new(false);
    let hold = key();
    let mut d = start(&client, job(&hold), &journal).await;
    let id = until_named(&mut d).await;
    let record_id = d.journal_id().expect("journaled").to_owned();

    let done = tokio::time::timeout(
        GUARD,
        d.wait_or_hand_off(std::future::ready(()), std::future::ready(())),
    )
    .await
    .expect("bounded");
    assert!(matches!(done.outcome, Outcome::HandedOff), "{done:?}");
    assert_eq!(done.task_id.as_deref(), Some(id.as_str()));

    assert_eq!(
        state_of(&client, &id).await,
        TaskState::Working,
        "the child was not cancelled"
    );
    assert!(
        journal.settled.lock().unwrap().is_empty(),
        "the record was not settled"
    );
    let open = journal.inner.open("supervisor-1").await.expect("open");
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0].id, record_id);
    assert_eq!(open[0].task_id.as_deref(), Some(id.as_str()));

    // The next supervisor finds the record and follows the child to its end.
    let again = Delegation::reattach_with(
        Arc::clone(&client),
        None,
        id.clone(),
        open[0].last_event_id.clone(),
        DelegationOptions::new().with_journal_record(
            Arc::clone(&journal) as Arc<dyn DelegationJournal>,
            record_id,
        ),
    )
    .await
    .expect("reattach");
    gate(&hold).notify_one();
    let done = tokio::time::timeout(GUARD, again.wait(std::future::pending()))
        .await
        .expect("bounded");
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
    let settled = journal.settled.lock().unwrap().clone();
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].1, "completed");
}

/// Without a record nothing can find the child again, so a hand-off cancels it.
#[tokio::test]
async fn without_a_journal_the_hand_off_cancels_the_child() {
    let client = client_for(serve().await);
    let hold = key();
    let mut d = Delegation::start(Arc::clone(&client), job(&hold))
        .await
        .expect("start");
    let id = until_named(&mut d).await;

    let done = tokio::time::timeout(
        GUARD,
        d.wait_or_hand_off(std::future::pending(), std::future::ready(())),
    )
    .await
    .expect("bounded");
    assert!(
        matches!(
            done.outcome,
            Outcome::Canceled(_) | Outcome::CancelRequested(_)
        ),
        "{done:?}"
    );
    until_state(&client, &id, TaskState::Canceled).await;
}

/// The parent's own cancellation, with no shutdown, still cancels a journaled
/// child and settles its record: only the hand-off future hands off.
#[tokio::test]
async fn a_cancel_that_is_not_a_hand_off_still_cancels_a_journaled_child() {
    let client = client_for(serve().await);
    let journal = Probe::new(false);
    let hold = key();
    let mut d = start(&client, job(&hold), &journal).await;
    let id = until_named(&mut d).await;

    let done = tokio::time::timeout(
        GUARD,
        d.wait_or_hand_off(std::future::ready(()), std::future::pending()),
    )
    .await
    .expect("bounded");
    assert!(
        matches!(
            done.outcome,
            Outcome::Canceled(_) | Outcome::CancelRequested(_)
        ),
        "{done:?}"
    );
    until_state(&client, &id, TaskState::Canceled).await;
    let settled = journal.settled.lock().unwrap().clone();
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert!(
        settled[0].1 == "canceled" || settled[0].1 == "cancel_requested",
        "{settled:?}"
    );
}

/// `wait` is `wait_or_hand_off` that never hands off.
#[tokio::test]
async fn wait_never_hands_off() {
    let client = client_for(serve().await);
    let journal = Probe::new(false);
    let hold = key();
    let mut d = start(&client, job(&hold), &journal).await;
    let _ = until_named(&mut d).await;
    let done = tokio::time::timeout(GUARD, d.wait(std::future::ready(())))
        .await
        .expect("bounded");
    assert!(!matches!(done.outcome, Outcome::HandedOff), "{done:?}");
}

async fn until_state(client: &A2aClient, id: &str, want: TaskState) {
    tokio::time::timeout(GUARD, async {
        while state_of(client, id).await != want {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the child did not reach {want:?}"));
}
