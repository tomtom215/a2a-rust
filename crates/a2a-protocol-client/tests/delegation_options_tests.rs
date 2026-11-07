// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `DelegationOptions` that cannot be honoured say so before anything is sent,
//! and print what they hold without printing what they carry.
//!
//! The client points at a port nothing listens on: a call that reached the
//! network would fail as a transport error, so a `Config` error shows the
//! options were refused first.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_client::delegation::{Delegation, DelegationOptions, HandOffPolicy, ResumePolicy};
use a2a_protocol_client::{A2aClient, ClientBuilder, ClientError};
use a2a_protocol_types::budget::{Budget, BudgetHandle, ChildGrant};
use a2a_protocol_types::delegation_journal::{DelegationJournal, InMemoryDelegationJournal};
use a2a_protocol_types::{Message, MessageSendParams, Part};

fn nowhere() -> Arc<A2aClient> {
    Arc::new(
        ClientBuilder::new("http://127.0.0.1:9")
            .build()
            .expect("client"),
    )
}

fn job() -> MessageSendParams {
    MessageSendParams::new(Message::user(
        uuid::Uuid::new_v4().to_string(),
        vec![Part::text("work")],
    ))
}

fn grant() -> ChildGrant {
    BudgetHandle::start(&Budget::new())
        .grant(&Budget::new())
        .expect("a grant")
}

fn journal() -> Arc<InMemoryDelegationJournal> {
    Arc::new(InMemoryDelegationJournal::new())
}

fn config_message(error: ClientError) -> String {
    match error {
        ClientError::Config(message) => message,
        other => panic!("expected a configuration error, got {other:?}"),
    }
}

#[tokio::test]
async fn start_with_refuses_a_record_that_exists_and_writes_nothing() {
    let journal = journal();
    let options = DelegationOptions::new()
        .with_journal_record(Arc::clone(&journal) as Arc<dyn DelegationJournal>, "rec-1");
    let error = Delegation::start_with(nowhere(), job(), options)
        .await
        .expect_err("refused");
    assert!(config_message(error).contains("reattach_with"));
    assert!(journal.get("rec-1").is_none());
}

#[tokio::test]
async fn reattach_with_refuses_a_budget() {
    let options = DelegationOptions::new().with_budget(grant());
    let error = Delegation::reattach_with(nowhere(), None, "t-1", None, options)
        .await
        .expect_err("refused");
    assert!(config_message(error).contains("budget"));
}

#[tokio::test]
async fn reattach_with_refuses_a_new_record_and_writes_nothing() {
    let journal = journal();
    let options = DelegationOptions::new().with_journal(
        Arc::clone(&journal) as Arc<dyn DelegationJournal>,
        "supervisor-1",
        "agent-1",
    );
    let error = Delegation::reattach_with(nowhere(), None, "t-1", None, options)
        .await
        .expect_err("refused");
    assert!(config_message(error).contains("with_journal_record"));
    assert!(journal.open("supervisor-1").await.expect("open").is_empty());
}

/// Without any option the handle is a plain delegation: the call goes to
/// the network, which is refused here, and a transport error is the answer.
#[tokio::test]
async fn no_options_is_a_plain_start() {
    let error = Delegation::start_with(nowhere(), job(), DelegationOptions::new())
        .await
        .expect_err("nothing listens");
    assert!(!matches!(error, ClientError::Config(_)), "{error:?}");
}

#[test]
fn what_the_options_hold_is_printed_and_what_they_carry_is_not() {
    assert_eq!(
        format!("{:?}", DelegationOptions::new()),
        "DelegationOptions { lease: None, budget: false, journal: false, resume: None, \
         id_wait: None, lineage: None, hand_off: None }"
    );
    let full = DelegationOptions::new()
        .with_lease(Duration::from_secs(2))
        .with_budget(grant())
        .with_journal(journal(), "supervisor-1", "agent-1")
        .with_resume(ResumePolicy::none())
        .with_id_wait(Duration::from_millis(500))
        .with_hand_off(HandOffPolicy::never());
    assert_eq!(
        format!("{full:?}"),
        "DelegationOptions { lease: Some(2s), budget: true, journal: true, \
         resume: Some(ResumePolicy { attempts: 0, backoff: 0ns, max_backoff: 0ns, \
         deadline: 0ns, healthy_after: 0ns, jitter: false }), id_wait: Some(500ms), \
         lineage: None, hand_off: Some(HandOffPolicy { continuations: 0, backoff: 0ns, max_backoff: 0ns }) }"
    );
}
