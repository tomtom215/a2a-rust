// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A required audit log keeps every record of a call that ends during a store
//! outage, through a real server (adversarial review A3, 2026-11-12).
//!
//! The agent takes the audit store down in the middle of its run, as a database
//! failover does, and the call ends while it is down. Every record that call
//! and its task owed (their events, and the `call` that says how it ended) is
//! refused by the store. When the store comes back, the chain holds all of
//! them, matched to the call that was admitted before the outage, and verifies.

#![cfg(feature = "audit")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use a2a_protocol_server::audit::record::{AuditRecord, kind};
use a2a_protocol_server::audit::{
    Appended, AuditLog, AuditStore, BoxFuture, InMemoryAuditStore, LegalHold,
};
use a2a_protocol_server::dispatch::RestDispatcher;
use a2a_protocol_server::serve::Server;
use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::audit::Checkpoint;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::TaskState;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;

/// An in-memory store that refuses every read and write while `down`.
struct Outage {
    inner: InMemoryAuditStore,
    down: AtomicBool,
}

impl Outage {
    fn check(&self) -> A2aResult<()> {
        if self.down.load(Ordering::SeqCst) {
            Err(A2aError::internal("the audit store is down"))
        } else {
            Ok(())
        }
    }
}

impl AuditStore for Outage {
    fn head<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<Option<(u64, String)>>> {
        Box::pin(async move {
            self.check()?;
            self.inner.head(c).await
        })
    }
    fn append<'a>(&'a self, r: &'a AuditRecord) -> BoxFuture<'a, A2aResult<Appended>> {
        Box::pin(async move {
            self.check()?;
            self.inner.append(r).await
        })
    }
    fn read<'a>(
        &'a self,
        c: &'a str,
        a: u64,
        l: usize,
    ) -> BoxFuture<'a, A2aResult<Vec<AuditRecord>>> {
        self.inner.read(c, a, l)
    }
    fn chains(&self) -> BoxFuture<'_, A2aResult<Vec<String>>> {
        self.inner.chains()
    }
    fn put_checkpoint<'a>(&'a self, c: &'a Checkpoint) -> BoxFuture<'a, A2aResult<()>> {
        self.inner.put_checkpoint(c)
    }
    fn checkpoints<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<Vec<Checkpoint>>> {
        self.inner.checkpoints(c)
    }
    fn last_before<'a>(
        &'a self,
        c: &'a str,
        ms: i64,
    ) -> BoxFuture<'a, A2aResult<Option<(u64, String)>>> {
        self.inner.last_before(c, ms)
    }
    fn delete_through<'a>(&'a self, c: &'a str, s: u64) -> BoxFuture<'a, A2aResult<u64>> {
        self.inner.delete_through(c, s)
    }
    fn place_hold<'a>(&'a self, h: &'a LegalHold) -> BoxFuture<'a, A2aResult<()>> {
        self.inner.place_hold(h)
    }
    fn release_hold<'a>(&'a self, c: &'a str) -> BoxFuture<'a, A2aResult<bool>> {
        self.inner.release_hold(c)
    }
    fn holds(&self) -> BoxFuture<'_, A2aResult<Vec<LegalHold>>> {
        self.inner.holds()
    }
}

/// The store the running agent takes down; there is one server per test.
static STORE: std::sync::OnceLock<Arc<Outage>> = std::sync::OnceLock::new();

struct Flaky;
agent_executor!(Flaky, |ctx, q| async {
    let emit = EventEmitter::new(ctx, q);
    emit.status(TaskState::Working).await?;
    // The outage begins mid-run.
    STORE.get().unwrap().down.store(true, Ordering::SeqCst);
    emit.artifact("answer", vec![Part::text("done")], None, Some(true))
        .await?;
    emit.status(TaskState::Completed).await
});

#[tokio::test]
async fn the_records_of_a_call_that_ends_in_an_outage_are_all_written_afterwards() {
    let store = Arc::new(Outage {
        inner: InMemoryAuditStore::new(),
        down: AtomicBool::new(false),
    });
    STORE.set(Arc::clone(&store)).ok().unwrap();
    let log = Arc::new(
        AuditLog::new(Arc::clone(&store) as Arc<dyn AuditStore>).require_record(true),
    );
    let handler = RequestHandlerBuilder::new(Flaky)
        .without_agent_card()
        .with_audit(Arc::clone(&log))
        .build()
        .unwrap();
    let bound = Server::bind("127.0.0.1:0").await.unwrap();
    let addr = bound.local_addr().unwrap();
    tokio::spawn(bound.serve_with_shutdown(
        RestDispatcher::new(Arc::new(handler)),
        std::future::pending(),
    ));

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let req = hyper::Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/message:send"))
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .body(Full::new(Bytes::from(
            r#"{"message":{"messageId":"m-1","role":"ROLE_USER","parts":[{"text":"go"}]}}"#,
        )))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    let status = resp.status().as_u16();
    let _ = resp.into_body().collect().await.unwrap();
    assert_eq!(status, 200, "the call itself is served: only its records waited");

    // The store is still down: what the call and its task owed is waiting.
    assert!(log.pending_outcomes() > 0, "nothing was waiting for the store");
    store.down.store(false, Ordering::SeqCst);

    let deadline = Instant::now() + Duration::from_secs(30);
    while log.pending_outcomes() > 0 {
        assert!(Instant::now() < deadline, "the spool never drained");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(log.outcomes_lost(), 0, "a record was lost");

    let records = log.export("").await.unwrap();
    let call_id = |r: &AuditRecord| r.detail.get("callId").and_then(|v| v.as_str()).map(str::to_owned);
    let started: Vec<_> = records.iter().filter(|r| r.kind == kind::CALL_STARTED).collect();
    let ended: Vec<_> = records.iter().filter(|r| r.kind == kind::CALL).collect();
    assert_eq!(started.len(), 1);
    assert_eq!(ended.len(), 1, "the call's outcome is in the chain");
    assert_eq!(call_id(ended[0]), call_id(started[0]));
    assert!(call_id(started[0]).is_some());
    let states: Vec<_> = records
        .iter()
        .filter(|r| r.kind == kind::TASK_EVENT)
        .filter_map(|r| r.state.clone())
        .collect();
    assert!(
        states.iter().any(|s| s.contains("COMPLETED")),
        "the completion event is in the chain: {states:?}"
    );
    assert!(log.unresolved_calls("", 0, Duration::ZERO).await.unwrap().is_empty());
    assert!(log.verify("").await.unwrap().is_intact());
}
