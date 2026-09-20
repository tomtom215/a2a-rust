// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The replay path waits for a task the winner is still writing.
//!
//! `claim_idempotency_key` commits before `persist_initial_task` writes the
//! row — it has to, or two racing duplicates would each get past the claim and
//! both execute. So there is a window in which the winner holds the key and
//! its task does not yet exist, and the loser's replay lands in it.
//!
//! The per-context lock does not close that window. A send carrying no
//! `context_id` gets a freshly minted one, so two duplicates of the *same*
//! message take two *different* locks and run concurrently — which is
//! precisely the case an idempotency key exists for, so the window is
//! reachable rather than theoretical.
//!
//! Before the wait, the loser was told `TaskNotFound` for a task that existed
//! milliseconds later. That is a misleading answer rather than merely an early
//! one: `TaskNotFound`'s own documentation attributes it to the retention
//! sweep, so a caller following the documented semantics concluded its task
//! was permanently gone and gave up.
//!
//! Both halves are pinned below: the wait resolves a task that arrives late,
//! and a task that is genuinely gone still reports as missing rather than
//! being silently re-executed.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::KEY;
use super::fixtures::{WithholdingStore, keyed_params, task_of, withholding_handler};
use crate::error::ServerError;
use crate::store::TaskStore as _;

#[tokio::test]
async fn a_replayed_key_waits_for_a_task_the_winner_has_not_written_yet() {
    let store = Arc::new(WithholdingStore::default());
    let (handler, executions) = withholding_handler(&store);

    let first = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect("the first keyed send must succeed");
    let first_id = task_of(&first).id.clone();

    // Two reads report the task absent, exactly as they would between the
    // claim commit and the row write. One would be enough to fail the old
    // single-read code; two also proves the loop retries rather than peeking
    // twice by luck.
    store.withhold(&first_id, 2);

    let replay = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect(
            "the replay must wait for the task the winner is still writing, \
             not report it missing",
        );

    assert_eq!(
        task_of(&replay).id,
        first_id,
        "the replay must return the winner's task, not a new one"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "the whole point of the key is that the agent runs once"
    );
}

/// Counter-test: the wait must not turn a genuinely missing task into a
/// success, or into a second execution.
///
/// A key deliberately outlives its task (see the index's own note), so a
/// retry arriving after the retention sweep really does find nothing. The
/// conservative answer is still `TaskNotFound`: dropping the key instead
/// would let the send execute a second time, which is the one outcome the
/// key exists to prevent.
#[tokio::test]
async fn a_key_whose_task_was_swept_still_reports_it_missing() {
    let store = Arc::new(WithholdingStore::default());
    let (handler, executions) = withholding_handler(&store);

    let first = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect("the first keyed send must succeed");
    let first_id = task_of(&first).id.clone();

    // What a retention sweep leaves behind: the key still claimed, the task
    // gone for good.
    store
        .delete(&first_id)
        .await
        .expect("deleting the task must succeed");

    let err = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect_err("a key whose task is really gone must not report success");

    match err {
        ServerError::TaskNotFound(id) => assert_eq!(
            id, first_id,
            "the error must name the task the key still points at"
        ),
        other => panic!("expected TaskNotFound, got {other:?}"),
    }
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "the retry must not re-run the agent just because the task is gone"
    );
}
