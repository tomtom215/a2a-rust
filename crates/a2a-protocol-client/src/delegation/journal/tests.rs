// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The bounds on the journal's writes (audit NEW-h-s0-3), in paused time so
//! that each is pinned to the millisecond, not to how long a loop took.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_types::error::{A2aError, A2aResult};

use super::*;
use crate::delegation::testing::parked;

type Fut<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

#[derive(Clone, Copy)]
enum Mode {
    /// Never answers, as a database that has stopped responding.
    Hang,
    /// Refuses at once.
    Fail,
    /// Accepts at once.
    Accept,
}

/// A journal whose answer the test chooses, counting each kind of call.
struct Scripted {
    mode: Mutex<Mode>,
    /// What `settle` answers when it is not what every other call does.
    settle_mode: Mutex<Option<Mode>>,
    begun: AtomicUsize,
    progress: AtomicUsize,
    settled: AtomicUsize,
    /// Settles that were accepted.
    settled_ok: AtomicUsize,
}

impl Scripted {
    fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            mode: Mutex::new(mode),
            settle_mode: Mutex::new(None),
            begun: AtomicUsize::new(0),
            progress: AtomicUsize::new(0),
            settled: AtomicUsize::new(0),
            settled_ok: AtomicUsize::new(0),
        })
    }

    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }

    fn settle_only(&self, mode: Mode) {
        *self.settle_mode.lock().unwrap() = Some(mode);
    }

    fn answer(&self) -> Fut<'_, ()> {
        let mode = *self.mode.lock().unwrap();
        Self::answer_as(mode)
    }

    fn answer_as(mode: Mode) -> Fut<'static, ()> {
        match mode {
            Mode::Hang => Box::pin(std::future::pending()),
            Mode::Fail => Box::pin(async { Err(A2aError::internal("the journal is down")) }),
            Mode::Accept => Box::pin(async { Ok(()) }),
        }
    }
}

impl DelegationJournal for Scripted {
    fn begin<'a>(&'a self, _record: &'a DelegationRecord) -> Fut<'a, ()> {
        self.begun.fetch_add(1, Ordering::SeqCst);
        self.answer()
    }

    fn progress<'a>(
        &'a self,
        _id: &'a str,
        _task_id: Option<&'a str>,
        _last_event_id: Option<&'a str>,
        _now_ms: i64,
    ) -> Fut<'a, ()> {
        self.progress.fetch_add(1, Ordering::SeqCst);
        self.answer()
    }

    fn settle<'a>(&'a self, _id: &'a str, _outcome: &'a str, _now_ms: i64) -> Fut<'a, ()> {
        self.settled.fetch_add(1, Ordering::SeqCst);
        let override_mode = *self.settle_mode.lock().unwrap();
        let mode = override_mode.unwrap_or_else(|| *self.mode.lock().unwrap());
        match mode {
            Mode::Accept => Box::pin(async move {
                self.settled_ok.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            other => Self::answer_as(other),
        }
    }

    fn open<'a>(&'a self, _owner: &'a str) -> Fut<'a, Vec<DelegationRecord>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn purge_settled(&self, _before: i64) -> Fut<'_, u64> {
        Box::pin(async { Ok(0) })
    }
}

fn journaled(mode: Mode) -> (Delegation, Arc<Scripted>) {
    let journal = Scripted::new(mode);
    let d = parked().with_journal(Arc::clone(&journal) as Arc<dyn DelegationJournal>, "rec-1");
    (d, journal)
}

fn calls(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::SeqCst)
}

#[tokio::test(start_paused = true)]
async fn a_progress_write_that_never_answers_is_given_up_at_the_bound() {
    let (mut d, journal) = journaled(Mode::Hang);
    let began = Instant::now();
    d.note_progress(true).await;
    assert_eq!(began.elapsed(), WRITE_BOUND);
    assert_eq!(calls(&journal.progress), 1);
    assert_eq!(WRITE_BOUND, Duration::from_secs(5));
}

#[tokio::test(start_paused = true)]
async fn a_write_that_timed_out_is_not_tried_again_until_the_progress_interval_has_passed() {
    let (mut d, journal) = journaled(Mode::Hang);
    d.note_progress(false).await; // the child was just named: due at once
    assert_eq!(calls(&journal.progress), 1);

    let again = Instant::now();
    d.note_progress(false).await;
    assert_eq!(again.elapsed(), Duration::ZERO, "held off, not waited for");
    assert_eq!(calls(&journal.progress), 1);

    tokio::time::advance(PROGRESS_EVERY.saturating_sub(Duration::from_millis(1))).await;
    d.note_progress(false).await;
    assert_eq!(calls(&journal.progress), 1, "a millisecond early");

    tokio::time::advance(Duration::from_millis(1)).await;
    d.note_progress(false).await;
    assert_eq!(calls(&journal.progress), 2, "due");
}

#[tokio::test(start_paused = true)]
async fn a_forced_write_is_not_held_off() {
    let (mut d, journal) = journaled(Mode::Hang);
    d.note_progress(false).await;
    d.note_progress(true).await;
    assert_eq!(calls(&journal.progress), 2);
}

#[tokio::test(start_paused = true)]
async fn a_write_that_failed_is_held_off_in_the_same_way_and_costs_no_time() {
    let (mut d, journal) = journaled(Mode::Fail);
    let began = Instant::now();
    d.note_progress(false).await;
    assert_eq!(
        began.elapsed(),
        Duration::ZERO,
        "a refusal is not waited for"
    );
    assert_eq!(calls(&journal.progress), 1);
    assert!(
        d.journal.as_ref().unwrap().noted_task.is_none(),
        "not noted"
    );

    d.note_progress(false).await;
    assert_eq!(calls(&journal.progress), 1, "held off");
    tokio::time::advance(PROGRESS_EVERY).await;
    journal.set(Mode::Accept);
    d.note_progress(false).await;
    assert_eq!(calls(&journal.progress), 2);
    assert_eq!(
        d.journal.as_ref().unwrap().noted_task.as_deref(),
        Some("child"),
        "noted once written"
    );
}

#[tokio::test(start_paused = true)]
async fn a_recorded_position_is_rewritten_only_after_the_progress_interval() {
    let (mut d, journal) = journaled(Mode::Accept);
    d.note_progress(false).await;
    assert_eq!(calls(&journal.progress), 1);

    d.last_event_id = Some("e-2".to_owned());
    tokio::time::advance(PROGRESS_EVERY.saturating_sub(Duration::from_millis(1))).await;
    d.note_progress(false).await;
    assert_eq!(calls(&journal.progress), 1, "a millisecond early");
    tokio::time::advance(Duration::from_millis(1)).await;
    d.note_progress(false).await;
    assert_eq!(calls(&journal.progress), 2, "due");

    // Nothing moved: nothing to write, forced or not.
    d.note_progress(true).await;
    assert_eq!(calls(&journal.progress), 2);
}

#[tokio::test(start_paused = true)]
async fn the_two_writes_at_the_end_share_one_deadline() {
    // Neither answers: the delegation waits the bound once, not twice.
    let (mut d, journal) = journaled(Mode::Hang);
    let began = Instant::now();
    d.note_settled(&Outcome::Unreachable).await;
    assert_eq!(began.elapsed(), WRITE_BOUND);
    assert_eq!((calls(&journal.progress), calls(&journal.settled)), (1, 1));
    assert!(d.journal_id().is_none(), "the record is let go");

    // The position is written at once and the outcome never answers.
    let (mut d, journal) = journaled(Mode::Accept);
    journal.settle_only(Mode::Hang);
    let began = Instant::now();
    d.note_settled(&Outcome::Unreachable).await;
    assert_eq!(began.elapsed(), WRITE_BOUND);
    assert_eq!(calls(&journal.settled_ok), 0);

    // The position is refused at once: the outcome still has the whole bound.
    let (mut d, journal) = journaled(Mode::Fail);
    journal.settle_only(Mode::Hang);
    let began = Instant::now();
    d.note_settled(&Outcome::Unreachable).await;
    assert_eq!(began.elapsed(), WRITE_BOUND);

    // Everything answers at once: no wait, and both are written.
    let (mut d, journal) = journaled(Mode::Accept);
    let began = Instant::now();
    d.note_settled(&Outcome::Unreachable).await;
    assert_eq!(began.elapsed(), Duration::ZERO);
    assert_eq!((calls(&journal.progress), calls(&journal.settled)), (1, 1));
    assert_eq!(calls(&journal.settled_ok), 1);
}

#[tokio::test(start_paused = true)]
async fn a_journal_that_answers_the_outcome_still_settles_after_a_position_that_timed_out() {
    let (mut d, journal) = journaled(Mode::Hang);
    journal.settle_only(Mode::Accept);
    let began = Instant::now();
    d.note_settled(&Outcome::Unreachable).await;
    assert_eq!(began.elapsed(), WRITE_BOUND, "the position used the bound");
    assert_eq!(
        calls(&journal.settled_ok),
        1,
        "and the outcome was recorded"
    );
}

#[tokio::test(start_paused = true)]
async fn the_record_is_written_before_sending_within_the_bound_or_nothing_is_sent() {
    let record = new_record("owner", "agent", None, None);

    let journal = Scripted::new(Mode::Hang);
    let began = Instant::now();
    let error = begin(&*journal, &record).await.expect_err("timed out");
    assert_eq!(began.elapsed(), WRITE_BOUND);
    match error {
        ClientError::Timeout(message) => assert!(
            message.contains("5s") && message.contains("nothing was sent"),
            "{message}"
        ),
        other => panic!("expected a timeout, got {other:?}"),
    }

    let journal = Scripted::new(Mode::Fail);
    assert!(matches!(
        begin(&*journal, &record).await,
        Err(ClientError::Protocol(_))
    ));
    let journal = Scripted::new(Mode::Accept);
    begin(&*journal, &record).await.expect("recorded");
    assert_eq!(calls(&journal.begun), 1);
}
