// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What watching leases costs the lease store, per interval, for N leased
//! turns on one replica (audit H5; the numbers are in ADR 0044).
//!
//! The clock is paused, so every turn's watcher starts at the same instant
//! and the window holds an exact number of check intervals.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::lease::Lease;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::TaskState;

use crate::builder::RequestHandlerBuilder;
use crate::store::lease::LapsedLeases;
use crate::store::{InMemoryLeaseStore, InMemoryTaskStore, LeaseStore};
use crate::{EventEmitter, agent_executor};

/// Works until cancelled.
struct Sleeper;
agent_executor!(Sleeper, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Working)
        .await?;
    ctx.cancellation_token.cancelled().await;
    Ok(())
});

/// An in-memory store that counts the reads a watcher makes: one per call,
/// whether it asks about one lease or many.
#[derive(Clone, Default)]
struct Counting {
    inner: InMemoryLeaseStore,
    reads: Arc<AtomicUsize>,
}

impl LeaseStore for Counting {
    fn grant<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
        ttl_ms: i64,
        deadline_ms: i64,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.grant(tenant, task, ttl_ms, deadline_ms)
    }

    fn renew<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
        now_ms: i64,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.inner.renew(tenant, task, now_ms)
    }

    fn deadline<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<i64>>> + Send + 'a>> {
        self.inner.deadline(tenant, task)
    }

    fn lapsed<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<bool>>> + Send + 'a>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.lapsed(tenant, task)
    }

    fn lapsed_many<'a>(
        &'a self,
        leases: &'a [(String, String)],
    ) -> Pin<Box<dyn Future<Output = A2aResult<LapsedLeases>> + Send + 'a>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.lapsed_many(leases)
    }

    fn release<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.release(tenant, task)
    }
}

/// N leased turns cost the lease store one read per check interval, not N.
/// Before 2026-11-04 each turn had a watcher of its own, and 50 turns over
/// three intervals made 150 reads (ADR 0044).
#[tokio::test(start_paused = true)]
async fn n_leased_turns_cost_one_lease_read_per_interval() {
    const N: usize = 50;
    let store = Counting::default();
    let handler = RequestHandlerBuilder::new(Sleeper)
        .without_agent_card()
        .with_task_store(InMemoryTaskStore::new())
        .with_lease_store(store.clone())
        .build()
        .expect("handler");
    for _ in 0..N {
        let mut message = Message::new(
            MessageId::new(uuid::Uuid::new_v4().to_string()),
            MessageRole::User,
            vec![Part::text("work")],
        );
        // A minute: checked every second (the longest check interval), and
        // judged by the wall clock, which this test does not move far.
        Lease::new(Duration::from_secs(60)).attach(&mut message);
        let mut params = MessageSendParams::new(message);
        params.configuration =
            Some(serde_json::from_value(serde_json::json!({ "returnImmediately": true })).unwrap());
        handler.on_send_message(params, false, None).await.unwrap();
    }
    store.reads.store(0, Ordering::SeqCst);
    // Three intervals and a millisecond: three checks of every lease.
    tokio::time::sleep(Duration::from_millis(3_001)).await;
    assert_eq!(store.reads.load(Ordering::SeqCst), 3);
}
