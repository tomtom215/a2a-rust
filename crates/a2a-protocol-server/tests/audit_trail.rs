// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The audit trail, end to end through a real server (ADR 0015).
//!
//! Each test drives HTTP+JSON calls at a `RestDispatcher` built with
//! `with_audit`, then reads the chain back out of the store and checks what a
//! regulator or an incident responder would ask of it: every call is there,
//! refused ones included; each task event is attributable to the caller who
//! started the run; content is recorded by digest only; and the exported
//! chain verifies against the checkpoint key.
#![cfg(feature = "audit")]

use std::sync::Arc;

use a2a_protocol_server::audit::record::{AuditRecord, kind, verify_chain};
use a2a_protocol_server::audit::{AuditLog, AuditStore, InMemoryAuditStore};
use a2a_protocol_server::dispatch::RestDispatcher;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{
    BearerTokenAuthInterceptor, EventEmitter, RequestHandlerBuilder, agent_executor,
};
use a2a_protocol_types::audit::{CheckpointSigner, SigningAlg};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::TaskState;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;

struct Echo;
agent_executor!(Echo, |ctx, q| async {
    let emit = EventEmitter::new(ctx, q);
    emit.status(TaskState::Working).await?;
    emit.artifact("answer", vec![Part::text("echo")], None, Some(true))
        .await?;
    emit.status(TaskState::Completed).await
});

const SECRET_TEXT: &str = "the patient's diagnosis is confidential";

fn signer() -> CheckpointSigner {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    CheckpointSigner::from_pkcs8(SigningAlg::EdDsa, "audit-key-1", pkcs8.as_ref()).unwrap()
}

async fn server(log: Arc<AuditLog>) -> std::net::SocketAddr {
    let handler = RequestHandlerBuilder::new(Echo)
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
            "tok-alice",
            "alice@example.test",
        )]))
        .with_audit(log)
        .build()
        .unwrap();
    serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::new(handler)))
        .await
        .unwrap()
}

async fn call(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: String,
    token: Option<&str>,
) -> (u16, serde_json::Value) {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let mut req = hyper::Request::builder()
        .method(method)
        .uri(format!("http://{addr}{path}"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .header(
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        );
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let resp = client
        .request(req.body(Full::new(Bytes::from(body))).unwrap())
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

fn send_body() -> String {
    serde_json::json!({
        "message": {"messageId": "m-1", "role": "ROLE_USER", "parts": [{"text": SECRET_TEXT}]}
    })
    .to_string()
}

/// Waits until the chain holds a record satisfying `done`; the task's events
/// are recorded by the background processor, after the call returns.
async fn wait_for(log: &AuditLog, done: impl Fn(&[AuditRecord]) -> bool) -> Vec<AuditRecord> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let records = log.export("").await.unwrap();
        if done(&records) || std::time::Instant::now() > deadline {
            return records;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
}

#[tokio::test]
async fn every_call_and_event_is_recorded_and_attributable() {
    let log = Arc::new(AuditLog::new(Arc::new(InMemoryAuditStore::new())).with_signer(signer(), 0));
    let addr = server(Arc::clone(&log)).await;

    // Refused: no credential.
    let (status, _) = call(addr, "POST", "/message:send", send_body(), None).await;
    assert_eq!(status, 401);
    // Served: a task that runs to completion.
    let (status, body) = call(
        addr,
        "POST",
        "/message:send",
        send_body(),
        Some("tok-alice"),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let task_id = body["task"]["id"].as_str().unwrap().to_owned();
    // Failed: a task that does not exist.
    let (status, _) = call(
        addr,
        "GET",
        "/tasks/no-such-task",
        String::new(),
        Some("tok-alice"),
    )
    .await;
    assert_eq!(status, 404);

    let records = wait_for(&log, |rs| {
        rs.iter().filter(|r| r.kind == kind::CALL).count() == 3
            && rs
                .iter()
                .any(|r| r.state.as_deref() == Some("TASK_STATE_COMPLETED"))
    })
    .await;

    // Three calls, in order, with outcomes and the authenticated caller.
    let calls: Vec<&AuditRecord> = records.iter().filter(|r| r.kind == kind::CALL).collect();
    assert_eq!(calls.len(), 3, "{records:#?}");
    assert_eq!(calls[0].outcome.as_ref().unwrap().status, "error");
    assert!(calls[0].actor.is_none(), "a refused caller has no identity");
    assert_eq!(calls[1].outcome.as_ref().unwrap().status, "ok");
    let alice = calls[1].actor.as_ref().unwrap();
    assert_eq!(alice.subject, "alice@example.test");
    assert_eq!(alice.scheme.as_deref(), Some("bearer"));
    assert_eq!(
        calls[2].outcome.as_ref().unwrap().error.as_deref(),
        Some("task_not_found")
    );
    assert_eq!(
        calls[1].trace.as_ref().unwrap().trace_id,
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );

    // The run names who started it, and its message only by digest.
    let run = records
        .iter()
        .find(|r| r.kind == kind::RUN_STARTED)
        .expect("run.started");
    assert_eq!(run.task_id.as_deref(), Some(task_id.as_str()));
    assert_eq!(run.actor.as_ref().unwrap().subject, "alice@example.test");
    assert!(run.digests.contains_key("message") && run.digests.contains_key("part.0"));

    // Every event of the task points at that run.
    let events: Vec<&AuditRecord> = records
        .iter()
        .filter(|r| r.kind == kind::TASK_EVENT && r.task_id.as_deref() == Some(task_id.as_str()))
        .collect();
    assert!(events.len() >= 3, "{events:#?}");
    assert!(
        events.iter().all(|e| e.run_seq == Some(run.seq)),
        "{events:#?}"
    );
    assert!(events.iter().all(|e| e.digests.contains_key("event")));
    assert_eq!(
        events.last().unwrap().state.as_deref(),
        Some("TASK_STATE_COMPLETED")
    );

    // The content itself is nowhere in the chain.
    let exported = serde_json::to_string(&records).unwrap();
    assert!(
        !exported.contains(SECRET_TEXT),
        "message text leaked into the audit log"
    );

    // And the export verifies against the checkpoint key, offline.
    let cp = log.checkpoint("").await.unwrap();
    let report = verify_chain(
        &records_through(&log, cp.seq).await,
        &[cp],
        &[log.trusted_key().unwrap()],
    );
    assert!(report.is_intact(), "{report:?}");
    assert_eq!(report.unsigned_tail(), 0);
}

async fn records_through(log: &AuditLog, seq: u64) -> Vec<AuditRecord> {
    let mut r = log.export("").await.unwrap();
    r.retain(|x| x.seq <= seq);
    r
}

#[tokio::test]
async fn a_cancel_request_names_who_asked() {
    // An executor that waits to be cancelled, so the cancel finds it running.
    struct Waits;
    agent_executor!(Waits, |ctx, q| async {
        let emit = EventEmitter::new(ctx, q);
        emit.status(TaskState::Working).await?;
        ctx.cancellation_token.cancelled().await;
        emit.status(TaskState::Canceled).await
    });
    let log = Arc::new(AuditLog::new(Arc::new(InMemoryAuditStore::new())));
    let handler = RequestHandlerBuilder::new(Waits)
        .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([(
            "tok-bob", "bob",
        )]))
        .with_audit(Arc::clone(&log))
        .build()
        .unwrap();
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::new(handler)))
        .await
        .unwrap();
    let body = serde_json::json!({
        "message": {"messageId": "m-2", "role": "ROLE_USER", "parts": [{"text": "wait"}]},
        "configuration": {"returnImmediately": true}
    })
    .to_string();
    let (_, sent) = call(addr, "POST", "/message:send", body, Some("tok-bob")).await;
    let task_id = sent["task"]["id"].as_str().unwrap().to_owned();
    let (status, _) = call(
        addr,
        "POST",
        &format!("/tasks/{task_id}:cancel"),
        "{}".to_owned(),
        Some("tok-bob"),
    )
    .await;
    assert_eq!(status, 200);

    let records = wait_for(&log, |rs| {
        rs.iter().any(|r| r.kind == kind::CANCEL_REQUESTED)
    })
    .await;
    let cancel = records
        .iter()
        .find(|r| r.kind == kind::CANCEL_REQUESTED)
        .expect("task.cancel_requested");
    assert_eq!(cancel.task_id.as_deref(), Some(task_id.as_str()));
    assert_eq!(cancel.actor.as_ref().unwrap().subject, "bob");
    assert!(verify_chain(&records, &[], &[]).is_intact());
}

/// A store that refuses every write.
struct Down;
impl AuditStore for Down {
    fn head<'a>(
        &'a self,
        _: &'a str,
    ) -> a2a_protocol_server::audit::BoxFuture<
        'a,
        a2a_protocol_types::error::A2aResult<Option<(u64, String)>>,
    > {
        Box::pin(async {
            Err(a2a_protocol_types::error::A2aError::internal(
                "audit store down",
            ))
        })
    }
    fn append<'a>(
        &'a self,
        _: &'a AuditRecord,
    ) -> a2a_protocol_server::audit::BoxFuture<
        'a,
        a2a_protocol_types::error::A2aResult<a2a_protocol_server::audit::Appended>,
    > {
        Box::pin(async {
            Err(a2a_protocol_types::error::A2aError::internal(
                "audit store down",
            ))
        })
    }
    fn read<'a>(
        &'a self,
        _: &'a str,
        _: u64,
        _: usize,
    ) -> a2a_protocol_server::audit::BoxFuture<
        'a,
        a2a_protocol_types::error::A2aResult<Vec<AuditRecord>>,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn chains(
        &self,
    ) -> a2a_protocol_server::audit::BoxFuture<'_, a2a_protocol_types::error::A2aResult<Vec<String>>>
    {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn put_checkpoint<'a>(
        &'a self,
        _: &'a a2a_protocol_types::audit::Checkpoint,
    ) -> a2a_protocol_server::audit::BoxFuture<'a, a2a_protocol_types::error::A2aResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn checkpoints<'a>(
        &'a self,
        _: &'a str,
    ) -> a2a_protocol_server::audit::BoxFuture<
        'a,
        a2a_protocol_types::error::A2aResult<Vec<a2a_protocol_types::audit::Checkpoint>>,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn last_before<'a>(
        &'a self,
        _: &'a str,
        _: i64,
    ) -> a2a_protocol_server::audit::BoxFuture<
        'a,
        a2a_protocol_types::error::A2aResult<Option<(u64, String)>>,
    > {
        Box::pin(async { Ok(None) })
    }
    fn delete_through<'a>(
        &'a self,
        _: &'a str,
        _: u64,
    ) -> a2a_protocol_server::audit::BoxFuture<'a, a2a_protocol_types::error::A2aResult<u64>> {
        Box::pin(async { Ok(0) })
    }
    fn place_hold<'a>(
        &'a self,
        _: &'a a2a_protocol_server::audit::LegalHold,
    ) -> a2a_protocol_server::audit::BoxFuture<'a, a2a_protocol_types::error::A2aResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn release_hold<'a>(
        &'a self,
        _: &'a str,
    ) -> a2a_protocol_server::audit::BoxFuture<'a, a2a_protocol_types::error::A2aResult<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn holds(
        &self,
    ) -> a2a_protocol_server::audit::BoxFuture<
        '_,
        a2a_protocol_types::error::A2aResult<Vec<a2a_protocol_server::audit::LegalHold>>,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[tokio::test]
async fn a_required_log_refuses_calls_it_cannot_record_and_an_optional_one_does_not() {
    for (required, want) in [(true, 500), (false, 200)] {
        let log = Arc::new(AuditLog::new(Arc::new(Down)).require_record(required));
        let addr = server(Arc::clone(&log)).await;
        let (status, body) = call(
            addr,
            "POST",
            "/message:send",
            send_body(),
            Some("tok-alice"),
        )
        .await;
        assert_eq!(status, want, "required={required}: {body}");
        assert!(
            log.failures() > 0,
            "required={required}: the failure was not counted"
        );
    }
}

#[tokio::test]
async fn each_tenant_gets_its_own_chain() {
    use a2a_protocol_server::push::TenantAwareInMemoryPushConfigStore;
    use a2a_protocol_server::store::TenantAwareInMemoryTaskStore;

    let log = Arc::new(AuditLog::new(Arc::new(InMemoryAuditStore::new())));
    let handler = RequestHandlerBuilder::new(Echo)
        .with_task_store(TenantAwareInMemoryTaskStore::new())
        .with_push_config_store(TenantAwareInMemoryPushConfigStore::new())
        .with_audit(Arc::clone(&log))
        .build()
        .unwrap();
    let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::new(handler)))
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for tenant in ["acme", "globex"] {
        let (status, body) = call(
            addr,
            "POST",
            &format!("/{tenant}/message:send"),
            send_body(),
            None,
        )
        .await;
        assert_eq!(status, 200, "{tenant}: {body}");
        tasks.push(body["task"]["id"].as_str().unwrap().to_owned());
    }
    for (tenant, own, other) in [
        ("acme", &tasks[0], &tasks[1]),
        ("globex", &tasks[1], &tasks[0]),
    ] {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let records = loop {
            let rs = log.export(tenant).await.unwrap();
            if rs
                .iter()
                .any(|r| r.state.as_deref() == Some("TASK_STATE_COMPLETED"))
                || std::time::Instant::now() > deadline
            {
                break rs;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        };
        assert!(records.iter().all(|r| r.chain == tenant));
        assert!(
            records
                .iter()
                .any(|r| r.task_id.as_deref() == Some(own.as_str())),
            "{tenant}: {records:#?}"
        );
        assert!(
            !records
                .iter()
                .any(|r| r.task_id.as_deref() == Some(other.as_str())),
            "{tenant} holds the other tenant's task"
        );
        assert!(records.iter().any(|r| r.kind == kind::RUN_STARTED));
        assert!(verify_chain(&records, &[], &[]).is_intact());
    }
    assert!(
        log.export("").await.unwrap().is_empty(),
        "nothing leaked into the untenanted chain"
    );
}
