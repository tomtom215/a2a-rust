// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The audit trail of a steering message (ADR 0036): who sent what to which
//! task, and where it went.

use a2a_protocol_server::audit::record::{AuditRecord, kind};
use a2a_protocol_server::audit::{AuditLog, InMemoryAuditStore};

use super::*;

async fn steered_records(log: &AuditLog, want: usize) -> Vec<AuditRecord> {
    let deadline = std::time::Instant::now() + GUARD;
    loop {
        let records: Vec<AuditRecord> = log
            .export("")
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.kind == kind::STEERED)
            .collect();
        if records.len() >= want || std::time::Instant::now() > deadline {
            return records;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Who steered which task, with what (by digest), and where it went.
#[tokio::test]
async fn a_delivery_is_recorded_with_who_sent_what_to_which_task() {
    let shared = Arc::new(Shared::default());
    let log = Arc::new(AuditLog::new(Arc::new(InMemoryAuditStore::new())));
    let handler = builder(&shared)
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
            "alice-token",
            "alice",
        )]))
        .with_audit(Arc::clone(&log))
        .with_steering(SteeringConfig::new().with_inbox_capacity(1))
        .build()
        .unwrap();
    let addr = serve(Binding::Rest, handler).await;
    let auth = [("authorization", "Bearer alice-token")];
    let task = start(Binding::Rest, addr, &shared, "block", &auth).await;

    let secret = "the account number is 12345678";
    let message = steering_message(&task, secret);
    let message_id = message["messageId"].as_str().unwrap().to_owned();
    let (path, body) = send_request(Binding::Rest, message.clone(), json!({}));
    let (status, _, _) = http(addr, path, &body, &auth).await;
    assert_eq!(status, 200);
    // The second finds the inbox full: refused, so not recorded.
    let (status, _, _) = steer(Binding::Rest, addr, &task, "refused", &auth).await;
    assert_eq!(status, 503);

    let records = steered_records(&log, 1).await;
    assert_eq!(records.len(), 1, "one delivery, one record: {records:?}");
    let record = &records[0];
    assert_eq!(record.task_id.as_deref(), Some(task.as_str()));
    assert_eq!(record.message_id.as_deref(), Some(message_id.as_str()));
    assert_eq!(record.actor.as_ref().unwrap().subject, "bearer:alice");
    assert_eq!(record.detail["delivery"], "inbox");
    assert!(
        record.digests["message"].starts_with("sha256:"),
        "{:?}",
        record.digests
    );
    let wire = serde_json::to_string(record).unwrap();
    assert!(
        !wire.contains(secret),
        "content is recorded by digest only: {wire}"
    );
    shared.release.notify_one();
}
