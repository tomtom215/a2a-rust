// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Metadata an artifact chunk carries when it is appended is kept, whatever
//! the store.
//!
//! Both event processors merge an appended chunk's metadata into the stored
//! artifact, then write the change as `ArtifactDelta::AppendedParts`. The
//! in-memory store's delta copies the artifact's metadata; the SQLite and
//! PostgreSQL deltas write the new parts alone, so the merged metadata was
//! lost there and `GetTask` disagreed between stores (found 2026-10-22).
//!
//! The PostgreSQL case is `#[ignore]`d without a live server:
//!
//! ```bash
//! A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
//!   cargo test -p a2a-protocol-server --features postgres \
//!   --test artifact_append_metadata -- --include-ignored
//! ```

use std::sync::Arc;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::handler::{RequestHandler, SendMessageResult};
use a2a_protocol_server::store::{InMemoryTaskStore, TaskStore};
use a2a_protocol_server::streaming::EventQueueReader;
use a2a_protocol_server::{EventEmitter, agent_executor};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent};
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::params::{MessageSendParams, TaskQueryParams};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskState};
use serde_json::json;

/// One artifact in two chunks, each carrying metadata: the first `{"a": 1,
/// "b": 1}`, the second, appended, `{"b": 2, "c": 3}`.
struct Chunked;
agent_executor!(Chunked, |ctx, queue| async {
    let chunk = |parts: &str, metadata: serde_json::Value, append: Option<bool>, last: bool| {
        let mut artifact = Artifact::new("report", vec![Part::text(parts)]);
        artifact.metadata = Some(metadata);
        StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            artifact,
            append,
            last_chunk: Some(last),
            metadata: None,
        })
    };
    queue
        .write(chunk("one", json!({"a": 1, "b": 1}), None, false))
        .await?;
    queue
        .write(chunk("two", json!({"b": 2, "c": 3}), Some(true), true))
        .await?;
    EventEmitter::new(ctx, queue)
        .status(TaskState::Completed)
        .await
});

fn handler(store: Arc<dyn TaskStore>) -> RequestHandler {
    RequestHandlerBuilder::new(Chunked)
        .without_agent_card()
        .with_task_store_arc(store)
        .build()
        .expect("handler")
}

/// Sends one message, blocking (the sync collector stores the events) or
/// streaming (the background processor does), and returns the task id.
async fn send(handler: &RequestHandler, streaming: bool) -> String {
    let params = MessageSendParams::new(Message::user_text("m-1", "go"));
    match handler.on_send_message(params, streaming, None).await {
        Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) => task.id.0,
        Ok(SendMessageResult::Stream(mut reader)) => {
            let mut id = None;
            while let Some(frame) = reader.read().await {
                match frame.expect("a frame").event {
                    StreamResponse::Task(t) => id = Some(t.id.0),
                    StreamResponse::ArtifactUpdate(u) => id = Some(u.task_id.0),
                    _ => {}
                }
            }
            id.expect("the stream names its task")
        }
        other => panic!("unexpected {:?}", other.map(|_| ())),
    }
}

/// The artifact as `GetTask` reads it back, after the background processor
/// has stored the last event.
async fn stored(handler: &RequestHandler, id: &str) -> Artifact {
    for _ in 0..200 {
        let task = handler
            .on_get_task(
                TaskQueryParams {
                    tenant: None,
                    id: id.to_owned(),
                    history_length: None,
                },
                None,
            )
            .await
            .expect("get");
        if task.status.state == TaskState::Completed {
            return task.artifacts.expect("artifacts").remove(0);
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the task never completed in the store");
}

async fn keeps_appended_metadata(store: Arc<dyn TaskStore>) {
    for streaming in [false, true] {
        let h = handler(Arc::clone(&store));
        let id = send(&h, streaming).await;
        let artifact = stored(&h, &id).await;
        assert_eq!(artifact.parts.len(), 2, "streaming={streaming}");
        assert_eq!(
            artifact.metadata,
            Some(json!({"a": 1, "b": 2, "c": 3})),
            "streaming={streaming}: the second chunk's metadata is merged and kept"
        );
    }
}

#[tokio::test]
async fn the_in_memory_store_keeps_appended_metadata() {
    keeps_appended_metadata(Arc::new(InMemoryTaskStore::new())).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn the_sqlite_store_keeps_appended_metadata() {
    let store = a2a_protocol_server::store::SqliteTaskStore::new("sqlite::memory:")
        .await
        .expect("store");
    keeps_appended_metadata(Arc::new(store)).await;
}

#[cfg(all(feature = "sqlite", feature = "encryption"))]
#[tokio::test]
async fn the_encrypted_sqlite_store_keeps_appended_metadata() {
    use a2a_protocol_server::store::encrypted::{
        AES_256_KEY_LEN, Aes256GcmCipher, EncryptedTaskStore,
    };
    let inner = a2a_protocol_server::store::SqliteTaskStore::new("sqlite::memory:")
        .await
        .expect("store");
    let cipher = Arc::new(Aes256GcmCipher::new("k1", [3; AES_256_KEY_LEN]).expect("cipher"));
    keeps_appended_metadata(Arc::new(EncryptedTaskStore::new(inner, cipher))).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs A2A_TEST_POSTGRES_URL"]
async fn the_postgres_store_keeps_appended_metadata() {
    let admin_url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let admin = sqlx::postgres::PgPool::connect(&admin_url)
        .await
        .expect("connect");
    let name = format!("a2a_append_meta_{}", std::process::id());
    let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .execute(&admin)
        .await;
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("create database");
    let url = format!("{}/{name}", admin_url.rsplit_once('/').expect("path").0);
    let store = a2a_protocol_server::store::PostgresTaskStore::new(&url)
        .await
        .expect("store");
    keeps_appended_metadata(Arc::new(store)).await;
    let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .execute(&admin)
        .await;
}
