// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What an executor adds to the audit trail itself (control map AIA-12-3).
//!
//! The SDK has no notion of a reference database or a match; the system that
//! does must be able to log them. It does so by appending a record of its own
//! to the log the handler was built with: the fields it needs go in
//! `detail` and `digests`, the record lands in the same per-tenant chain as
//! the server's own records, and an edit to those fields breaks that chain.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::audit::{AuditRecord, CheckpointSigner, SigningAlg, kind, verify_chain};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::Message;
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::TaskState;
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;

use super::{AuditLog, InMemoryAuditStore};
use crate::builder::RequestHandlerBuilder;
use crate::{AgentExecutor, EventEmitter, EventQueueWriter, RequestContext, SendMessageResult};

const SEARCH: &str = "search.matched";

/// Checks its input against a reference database and logs which database it
/// searched and the digest of the input that matched, as Article 12(3)(b)
/// and (c) ask of a remote biometric identification system.
struct Searches(Arc<AuditLog>);

impl AgentExecutor for Searches {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            let mut r = AuditRecord::new("", SEARCH);
            r.task_id = Some(ctx.task_id.0.clone());
            r.detail
                .insert("referenceDatabase".to_owned(), "watchlist-2026-10".into());
            r.digests
                .insert("matchedInput".to_owned(), "sha256:ab".to_owned());
            self.0.append(r).await?;
            emit.status(TaskState::Completed).await
        })
    }
}

#[tokio::test]
async fn an_executors_own_fields_are_recorded_in_the_tenants_chain() {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let signer =
        CheckpointSigner::from_pkcs8(SigningAlg::EdDsa, "audit-1", pkcs8.as_ref()).unwrap();
    let log = Arc::new(AuditLog::new(Arc::new(InMemoryAuditStore::new())).with_signer(signer, 0));
    let h = RequestHandlerBuilder::new(Searches(Arc::clone(&log)))
        .without_agent_card()
        .with_audit(Arc::clone(&log))
        .build()
        .expect("handler");
    let sent = h
        .on_send_message(
            MessageSendParams::new(Message::user_text("m-1", "probe")),
            false,
            None,
        )
        .await
        .expect("send");
    let SendMessageResult::Response(SendMessageResponse::Task(task)) = sent else {
        panic!("expected a task, got {sent:?}");
    };
    assert_eq!(task.status.state, TaskState::Completed);

    let records = log.export("").await.expect("export");
    let found = records
        .iter()
        .find(|r| r.kind == SEARCH)
        .unwrap_or_else(|| panic!("no {SEARCH} record: {records:#?}"));
    assert_eq!(found.task_id.as_deref(), Some(task.id.0.as_str()));
    assert_eq!(found.detail["referenceDatabase"], "watchlist-2026-10");
    assert_eq!(found.digests["matchedInput"], "sha256:ab");
    assert!(
        !found.time.is_empty(),
        "stamped with the time it was written"
    );
    // The same chain as the server's own record of the call that ran it.
    assert!(records.iter().any(|r| r.kind == kind::CALL), "{records:#?}");

    // The chain verifies, and the executor's fields are part of what it
    // protects: changing the database it names breaks it.
    let cp = log.checkpoint("").await.expect("checkpoint");
    let key = log.trusted_key().expect("key");
    let through: Vec<AuditRecord> = records.into_iter().filter(|r| r.seq <= cp.seq).collect();
    let report = verify_chain(
        &through,
        std::slice::from_ref(&cp),
        std::slice::from_ref(&key),
    );
    assert!(report.is_intact(), "{report:?}");
    let mut edited = through;
    let at = edited
        .iter()
        .position(|r| r.kind == SEARCH)
        .expect("still there");
    edited[at]
        .detail
        .insert("referenceDatabase".to_owned(), "another".into());
    let report = verify_chain(&edited, &[cp], &[key]);
    assert!(
        !report.is_intact(),
        "an edited detail went unnoticed: {report:?}"
    );
}
