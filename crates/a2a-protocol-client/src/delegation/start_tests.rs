// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a failed start does with its budget grant and its journal record.
//! A refusal (no child exists) returns the grant and settles the record
//! `not_started`; an error that may come after the agent made the child
//! keeps both. The transport is scripted and the clock is paused.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_types::budget::{Budget, BudgetHandle};
use a2a_protocol_types::delegation_journal::{
    DelegationJournal, DelegationRecord, InMemoryDelegationJournal,
};
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;

use super::super::journal::WRITE_BOUND;
use crate::ClientBuilder;
use crate::delegation::{Delegation, DelegationOptions};
use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

/// Fails every send with the error `make` builds, and counts the sends.
struct Failing {
    make: fn() -> ClientError,
    sends: Arc<AtomicUsize>,
}

impl Transport for Failing {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Err((self.make)()) })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Err((self.make)()) })
    }
}

/// How the journal answers `settle`.
#[derive(Clone, Copy)]
enum Settle {
    Accept,
    Fail,
    Hang,
}

/// An in-memory journal whose `settle` answers as `mode` says, recording
/// each outcome it was asked to write.
struct Journal {
    inner: InMemoryDelegationJournal,
    mode: Settle,
    asked: Mutex<Vec<String>>,
}

type Fut<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

impl DelegationJournal for Journal {
    fn begin<'a>(&'a self, record: &'a DelegationRecord) -> Fut<'a, ()> {
        self.inner.begin(record)
    }

    fn progress<'a>(
        &'a self,
        id: &'a str,
        task_id: Option<&'a str>,
        last_event_id: Option<&'a str>,
        now_ms: i64,
    ) -> Fut<'a, ()> {
        self.inner.progress(id, task_id, last_event_id, now_ms)
    }

    fn settle<'a>(&'a self, id: &'a str, outcome: &'a str, now_ms: i64) -> Fut<'a, ()> {
        self.asked.lock().unwrap().push(outcome.to_owned());
        match self.mode {
            Settle::Accept => self.inner.settle(id, outcome, now_ms),
            Settle::Fail => Box::pin(async { Err(A2aError::internal("the journal is down")) }),
            Settle::Hang => Box::pin(std::future::pending()),
        }
    }

    fn open<'a>(&'a self, owner: &'a str) -> Fut<'a, Vec<DelegationRecord>> {
        self.inner.open(owner)
    }

    fn purge_settled(&self, before: i64) -> Fut<'_, u64> {
        self.inner.purge_settled(before)
    }
}

/// What a start that failed with `make`'s error left behind.
struct After {
    error: ClientError,
    /// The parent's `tokens` left: 100 before the grant, 60 while the
    /// grant's 40 are reserved.
    tokens_left: Option<u64>,
    settles_asked: Vec<String>,
    open: usize,
    sends: usize,
}

const OWNER: &str = "supervisor";

async fn start_failing(make: fn() -> ClientError, mode: Settle) -> After {
    let sends = Arc::new(AtomicUsize::new(0));
    let client = Arc::new(
        ClientBuilder::new("http://stub.invalid")
            .with_custom_transport(Failing {
                make,
                sends: Arc::clone(&sends),
            })
            .build()
            .unwrap(),
    );
    let parent = BudgetHandle::start(&Budget::new().with_meter("tokens", 100).unwrap());
    let grant = parent
        .grant(&Budget::new().with_meter("tokens", 40).unwrap())
        .unwrap();
    assert_eq!(
        parent.remaining("tokens"),
        Some(60),
        "the grant is reserved"
    );
    let journal = Arc::new(Journal {
        inner: InMemoryDelegationJournal::new(),
        mode,
        asked: Mutex::new(Vec::new()),
    });
    let options = DelegationOptions::new().with_budget(grant).with_journal(
        Arc::clone(&journal) as Arc<dyn DelegationJournal>,
        OWNER,
        "worker",
    );
    let params = MessageSendParams::new(Message::new(
        MessageId::new("first"),
        MessageRole::User,
        vec![Part::text("do it")],
    ));
    // A guard far past WRITE_BOUND, so a start that never returns fails the
    // test instead of hanging it (the paused clock advances to it).
    let error = tokio::time::timeout(
        Duration::from_secs(3600),
        Delegation::start_with(client, params, options),
    )
    .await
    .expect("the start returns")
    .expect_err("the start fails");
    let open = journal.inner.open(OWNER).await.unwrap().len();
    let settles_asked = journal.asked.lock().unwrap().clone();
    After {
        error,
        tokens_left: parent.remaining("tokens"),
        settles_asked,
        open,
        sends: sends.load(Ordering::SeqCst),
    }
}

fn status(status: u16, retry_after: Option<Duration>) -> ClientError {
    ClientError::UnexpectedStatus {
        status,
        body: String::new(),
        retry_after,
    }
}

fn assert_refused(after: &After) {
    assert_eq!(
        after.tokens_left,
        Some(100),
        "{}: the grant is returned",
        after.error
    );
    assert_eq!(after.settles_asked, ["not_started"], "{}", after.error);
    assert_eq!(after.open, 0, "{}: the record is settled", after.error);
}

fn assert_ambiguous(after: &After) {
    assert_eq!(
        after.tokens_left,
        Some(60),
        "{}: the grant stays reserved",
        after.error
    );
    assert!(
        after.settles_asked.is_empty(),
        "{}: {:?}",
        after.error,
        after.settles_asked
    );
    assert_eq!(after.open, 1, "{}: the record stays open", after.error);
}

/// N26-01: a bare `503` may come after the agent made the child (D-1 calls
/// it ambiguous). Before the fix the grant was returned and the record
/// settled `not_started`, so a restarted supervisor never looked for the
/// child, and its spend was refunded.
#[tokio::test(start_paused = true)]
async fn a_bare_503_keeps_the_grant_and_leaves_the_record_open() {
    let after = start_failing(|| status(503, None), Settle::Accept).await;
    assert!(
        matches!(
            after.error,
            ClientError::UnexpectedStatus { status: 503, .. }
        ),
        "{:?}",
        after.error
    );
    assert_eq!(after.sends, 1, "a bare 503 is not sent again (D-1)");
    assert_ambiguous(&after);
}

#[tokio::test(start_paused = true)]
async fn a_500_and_a_gateways_502_and_504_are_ambiguous() {
    assert_ambiguous(&start_failing(|| status(500, None), Settle::Accept).await);
    assert_ambiguous(&start_failing(|| status(502, None), Settle::Accept).await);
    assert_ambiguous(&start_failing(|| status(504, None), Settle::Accept).await);
    assert_ambiguous(
        &start_failing(|| status(500, Some(Duration::from_secs(1))), Settle::Accept).await,
    );
}

#[tokio::test(start_paused = true)]
async fn a_lost_response_is_ambiguous() {
    let lost = || ClientError::Transport("the response was lost".into());
    assert_ambiguous(&start_failing(lost, Settle::Accept).await);
}

/// The refusals: a `503` naming a delay was raised before the request was
/// processed (D-1), and a `4xx` is the agent saying no.
#[tokio::test(start_paused = true)]
async fn a_4xx_and_a_503_naming_a_delay_are_refusals() {
    assert_refused(&start_failing(|| status(400, None), Settle::Accept).await);
    assert_refused(&start_failing(|| status(429, None), Settle::Accept).await);
    assert_refused(&start_failing(|| status(499, None), Settle::Accept).await);
    let delayed = start_failing(|| status(503, Some(Duration::from_secs(1))), Settle::Accept).await;
    assert_refused(&delayed);
    assert_refused(
        &start_failing(
            || A2aError::invalid_params("no parts").into(),
            Settle::Accept,
        )
        .await,
    );
}

/// N26-04: the settle after a refusal had no bound, so a journal that
/// stopped answering held the start for ever. It is given up at
/// [`WRITE_BOUND`], the refusal is returned, and the record stays open for
/// the supervisor to find.
#[tokio::test(start_paused = true)]
async fn a_settle_that_never_answers_is_given_up_at_the_bound() {
    let began = tokio::time::Instant::now();
    let after = start_failing(|| status(400, None), Settle::Hang).await;
    assert_eq!(began.elapsed(), WRITE_BOUND);
    assert!(
        matches!(
            after.error,
            ClientError::UnexpectedStatus { status: 400, .. }
        ),
        "the refusal is returned: {:?}",
        after.error
    );
    assert_eq!(after.tokens_left, Some(100), "the grant is returned");
    assert_eq!(after.settles_asked, ["not_started"]);
    assert_eq!(after.open, 1, "the record stays open");
}

#[tokio::test(start_paused = true)]
async fn a_settle_that_fails_still_returns_the_refusal_at_once() {
    let began = tokio::time::Instant::now();
    let after = start_failing(|| status(400, None), Settle::Fail).await;
    assert_eq!(began.elapsed(), Duration::ZERO);
    assert!(
        matches!(
            after.error,
            ClientError::UnexpectedStatus { status: 400, .. }
        ),
        "{:?}",
        after.error
    );
    assert_eq!(after.tokens_left, Some(100));
    assert_eq!(after.open, 1, "the record stays open");
}
