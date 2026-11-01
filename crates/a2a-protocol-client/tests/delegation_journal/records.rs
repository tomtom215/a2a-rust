// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a record holds: the idempotency key, and what a refusal leaves behind.

use a2a_protocol_client::delegation::Delegation;
use a2a_protocol_types::delegation_journal::DelegationJournal;
use a2a_protocol_types::{Message, MessageSendParams};

use super::harness::*;

/// The 64 hex digits of a key `Delegation::start` makes.
fn is_fresh_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

#[tokio::test]
async fn an_unkeyed_message_to_an_agent_not_known_to_honour_keys_is_recorded_without_one() {
    let client = client_for(serve().await);
    let journal = Probe::new(false);
    let d = start(&client, job("done:plain"), &journal).await;
    let record = d.journal_id().unwrap().to_owned();
    let _ = d.wait(std::future::pending()).await;
    assert_eq!(journal.inner.get(&record).unwrap().idempotency_key, None);
}

#[tokio::test]
async fn an_agent_known_to_honour_keys_gets_one_and_the_record_holds_it() {
    let client = honouring_client_for(serve().await);
    let journal = Probe::new(false);
    let d = start(&client, job("done:fresh"), &journal).await;
    let record = d.journal_id().unwrap().to_owned();
    let _ = d.wait(std::future::pending()).await;
    let key = journal.inner.get(&record).unwrap().idempotency_key;
    assert!(key.as_deref().is_some_and(is_fresh_key), "{key:?}");
}

#[tokio::test]
async fn a_key_the_caller_set_is_kept_even_for_an_agent_that_honours_keys() {
    let client = honouring_client_for(serve().await);
    let journal = Probe::new(false);
    let mine = "m".repeat(40);
    let d = start(&client, keyed("done:mine", &mine), &journal).await;
    let record = d.journal_id().unwrap().to_owned();
    let _ = d.wait(std::future::pending()).await;
    assert_eq!(
        journal
            .inner
            .get(&record)
            .unwrap()
            .idempotency_key
            .as_deref(),
        Some(mine.as_str())
    );
}

/// An agent that answers with a refusal created no child: the record is
/// settled `not_started`, not left open for a restarted supervisor to chase.
#[tokio::test]
async fn a_send_the_agent_refuses_settles_its_record_not_started() {
    let client = client_for(serve().await);
    let journal = Probe::new(false);
    // A message with no parts is invalid; the server answers with an error.
    let empty = MessageSendParams::new(Message::user(uuid::Uuid::new_v4().to_string(), vec![]));
    let err = Delegation::start_journaled(
        client,
        empty,
        journal.clone() as std::sync::Arc<dyn DelegationJournal>,
        "supervisor-1",
        "http://worker.example",
    )
    .await
    .expect_err("refused");
    let settled = journal.settled.lock().unwrap().clone();
    assert_eq!(settled.len(), 1, "{settled:?} after {err}");
    assert_eq!(settled[0].1, "not_started");
    assert!(
        journal.inner.get(&settled[0].0).unwrap().outcome.is_some(),
        "and the record says so"
    );
    let open = journal.inner.open("supervisor-1").await.expect("open");
    assert!(open.is_empty(), "{open:?}");
}

/// An agent that cannot be reached may still have got the message: the record
/// stays open, for the restarted supervisor to find.
#[tokio::test]
async fn a_send_that_never_reaches_the_agent_leaves_its_record_open() {
    let free = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let client = client_for(free);
    let journal = Probe::new(false);
    let err = Delegation::start_journaled(
        client,
        job("done:nobody-home"),
        journal.clone() as std::sync::Arc<dyn DelegationJournal>,
        "supervisor-1",
        "http://worker.example",
    )
    .await
    .expect_err("nothing is listening");
    assert!(journal.settled.lock().unwrap().is_empty(), "after {err}");
    let open = journal.inner.open("supervisor-1").await.expect("open");
    assert_eq!(open.len(), 1, "{open:?}");
}
