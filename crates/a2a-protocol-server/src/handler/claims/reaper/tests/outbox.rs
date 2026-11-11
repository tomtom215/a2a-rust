// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! With a push outbox the reaper records the ending it makes, rather than
//! sending it from its own loop (ADR 0037, audit H14).
//!
//! Recorded, the ending queues behind the rows another event of the same task
//! still has reserved, so a webhook sees it after the events the dead replica
//! had stored; and it survives a stop of the reaper's own replica. Sent from
//! the loop it did neither (a live run on two replicas delivered `failed`
//! 40 s before an artifact the dead replica had stored).

#![cfg(feature = "sqlite")]

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::TaskState;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::{remove_sqlite, sqlite_age, sqlite_url, task};
use crate::push::outbox::{InMemoryPushOutbox, PushOutbox};
use crate::push::{InMemoryPushConfigStore, PushConfigStore, PushSender, current_delivery_id};
use crate::store::{SqliteTaskStore, TaskStore};

/// Records each delivery's event and delivery id: an id is the outbox's, so a
/// send from the reaper's own loop has none.
#[derive(Default)]
struct Ids(Mutex<Vec<(StreamResponse, Option<String>)>>);

impl PushSender for Ids {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0
            .lock()
            .unwrap()
            .push((event.clone(), current_delivery_id()));
        Box::pin(async { Ok(()) })
    }
    fn allows_private_urls(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn the_reaper_records_its_ending_in_the_outbox_when_the_handler_has_one() {
    let (url, path) = sqlite_url();
    let store = Arc::new(SqliteTaskStore::new(&url).await.unwrap());
    let age = sqlite_age(sqlx::SqlitePool::connect(&url).await.unwrap(), "tasks");
    let claims_store = store.turn_claims().expect("SQL stores claim turns");
    let id = format!("orphan-{}", uuid::Uuid::new_v4());
    // Killed mid-turn: working, its claim expired long ago.
    store.save(&task(&id, TaskState::Working)).await.unwrap();
    age(id.clone()).await;
    assert!(
        claims_store
            .claim("", &id, "dead", "token", Duration::from_millis(1))
            .await
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(20)).await;

    let configs = Arc::new(InMemoryPushConfigStore::new());
    configs
        .set(TaskPushNotificationConfig::new(&id, "http://127.0.0.1:9/hook").with_id("hook"))
        .await
        .unwrap();
    let sender = Arc::new(Ids::default());
    let limits = crate::handler::limits::HandlerLimits::default();
    let lanes = crate::handler::PushLanes::with_reservations(
        Arc::clone(&configs) as Arc<dyn PushConfigStore>,
        Some(Arc::clone(&sender) as Arc<dyn PushSender>),
        limits.clone(),
        Arc::new(crate::metrics::NoopMetrics),
        TaskTracker::new(),
        Some((
            Arc::new(InMemoryPushOutbox::new()) as Arc<dyn PushOutbox>,
            CancellationToken::new(),
        )),
        Some(Arc::clone(&store) as Arc<dyn TaskStore>),
    );
    let claims = Arc::new(super::super::super::TurnClaims::new(
        claims_store,
        Some("reaper".into()),
        Duration::from_secs(600),
        Duration::from_secs(1),
    ));
    let reaper = super::super::Reaper {
        claims: Arc::downgrade(&claims),
        task_store: Arc::clone(&store) as Arc<dyn TaskStore>,
        push_config_store: configs,
        push_sender: Some(Arc::clone(&sender) as Arc<dyn PushSender>),
        push_lanes: lanes,
        leases: None,
        limits,
        metrics: Arc::new(crate::metrics::NoopMetrics),
    };

    assert!(reaper.look().await.is_some_and(|ended| ended >= 1));

    let sent = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let sent = sender.0.lock().unwrap().clone();
            if !sent.is_empty() {
                return sent;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the ending was delivered");
    assert_eq!(sent.len(), 1, "once: {sent:?}");
    assert!(matches!(
        &sent[0].0,
        StreamResponse::StatusUpdate(e) if e.status.state == TaskState::Failed
    ));
    assert!(
        sent[0].1.is_some(),
        "delivered from the outbox, with its delivery id, not from the reaper's own loop"
    );
    drop(claims);
    remove_sqlite(&path);
}
