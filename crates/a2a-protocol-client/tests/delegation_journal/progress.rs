// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The child's id and stream position reach the record, at a bounded rate.

use std::sync::atomic::Ordering;
use std::time::Duration;

use a2a_protocol_client::delegation::Outcome;

use super::harness::*;

#[tokio::test]
async fn progress_is_written_when_the_child_is_named_and_then_throttled() {
    let client = client_for(serve().await);
    let journal = Probe::new(false);
    // Sixteen events 100 ms apart: about 1.6 s of stream.
    let d = start(&client, job("tick:go"), &journal).await;
    let record = d.journal_id().unwrap().to_owned();
    let done = d.wait(std::future::pending()).await;
    assert!(matches!(done.outcome, Outcome::Completed(_)));
    let calls = journal.progress_calls.load(Ordering::SeqCst);
    assert!(
        (1..=4).contains(&calls),
        "one write when named, at most one more per two seconds, one at the end: {calls}"
    );
    let r = journal.inner.get(&record).unwrap();
    assert_eq!(r.task_id, done.task_id);
}

/// Twenty-six events 100 ms apart: about 2.6 s, longer than the two seconds the
/// record may lag. It is written when the child is named, once more when two
/// seconds have passed since, and at the end of the stream however recent the
/// last write was; never twice inside the two seconds.
#[tokio::test]
async fn progress_is_written_again_after_two_seconds_and_flushed_at_the_end() {
    let client = client_for(serve().await);
    let journal = Probe::new(false);
    let mut d = start(&client, job("slowtick:go"), &journal).await;
    let record = d.journal_id().unwrap().to_owned();
    while let Some(event) = tokio::time::timeout(GUARD, d.next_event())
        .await
        .expect("the stream ends in time")
    {
        event.expect("not an error");
    }

    // The stream is over and the record is not settled yet: its position is
    // the stream's last, though the write before it was under two seconds ago.
    let last = d.last_event_id().map(str::to_owned);
    assert!(last.is_some(), "the server numbered its events");
    let r = journal.inner.get(&record).unwrap();
    assert_eq!(r.last_event_id, last, "the end of the stream is written");
    assert_eq!(r.task_id.as_deref(), d.task_id());

    let at = journal.progress_at.lock().unwrap().clone();
    assert!(
        (3..=4).contains(&at.len()),
        "named, two seconds on, and the end: {} writes",
        at.len()
    );
    for pair in at[..at.len() - 1].windows(2) {
        let gap = pair[1] - pair[0];
        assert!(
            gap >= Duration::from_millis(1_900),
            "two writes {gap:?} apart, inside the two seconds"
        );
    }
    let done = d.wait(std::future::pending()).await;
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
}
