// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Release audit F7: what `EncryptedTaskStore` seals per write, and the
//! budget it seals against.
//!
//! An artifact streamed in `n` chunks used to cost about `n²/2` seals, every
//! write sealing every part of the task, against a per-key budget of 2^32
//! that was counted and not enforced. An in-place write now seals only the
//! parts it stores, the rest of the task going down as placeholders that
//! never open; and the cipher refuses to seal past its limit.

#![cfg(feature = "encryption")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_server::store::encrypted::{
    AES_256_KEY_LEN, Aes256GcmCipher, ContentCipher, EncryptedTaskStore,
};
use a2a_protocol_server::store::{ArtifactDelta, InMemoryTaskStore, TaskStore};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

const CHUNKS: usize = 100;

fn cipher() -> Arc<Aes256GcmCipher> {
    Aes256GcmCipher::new("k1", [5; AES_256_KEY_LEN])
        .expect("cipher")
        .into_arc()
}

fn task(parts: usize) -> Task {
    Task {
        id: TaskId::new("t-seal"),
        context_id: ContextId::new("ctx"),
        status: TaskStatus::new(TaskState::Working),
        history: Some(vec![Message::user_text("h", "a question with some text")]),
        artifacts: Some(vec![Artifact::new(
            "out",
            (0..parts)
                .map(|i| Part::text(format!("chunk {i}")))
                .collect::<Vec<_>>(),
        )]),
        metadata: None,
    }
}

/// Streams `CHUNKS` one-part appends through `store` after a first write of
/// one part, and returns how many values the cipher sealed for the appends.
async fn stream(store: &dyn TaskStore, cipher: &Aes256GcmCipher) -> u64 {
    store.save(&task(1)).await.expect("first write");
    let before = cipher.seal_count();
    for n in 2..=CHUNKS + 1 {
        store
            .save_artifact_delta(
                &task(n),
                ArtifactDelta::AppendedParts { index: 0, count: 1 },
            )
            .await
            .expect("append");
    }
    cipher.seal_count() - before
}

async fn assert_every_chunk_reads_back(store: &dyn TaskStore) {
    let read = store
        .get(&TaskId::new("t-seal"))
        .await
        .expect("every part opens")
        .expect("stored");
    let texts: Vec<_> = read.artifacts.unwrap()[0]
        .parts
        .iter()
        .map(|p| p.text_content().unwrap().to_owned())
        .collect();
    let expected: Vec<_> = (0..=CHUNKS).map(|i| format!("chunk {i}")).collect();
    assert_eq!(texts, expected);
    assert_eq!(
        read.history.unwrap()[0].parts[0].text_content(),
        Some("a question with some text")
    );
}

/// One seal per appended part, and every part reads back where it was
/// written. Before this fix the same stream cost 5,250 seals here, every
/// write sealing every part of the task: measured 2026-10-18 against the
/// previous `EncryptedTaskStore`, and the sum of 2 to 101 parts plus the
/// history's one on each of the 100 writes.
async fn appends_cost_one_seal_each(inner: Arc<dyn TaskStore>) {
    let cipher = cipher();
    let store = EncryptedTaskStore::from_arc(inner, Arc::clone(&cipher) as Arc<dyn ContentCipher>);
    let sealed = stream(&store, &cipher).await;
    assert_eq!(sealed, CHUNKS as u64, "one seal per appended part");
    assert_every_chunk_reads_back(&store).await;
}

#[tokio::test]
async fn in_memory_appends_cost_one_seal_each() {
    appends_cost_one_seal_each(Arc::new(InMemoryTaskStore::new())).await;
}

#[tokio::test]
async fn tenant_aware_appends_cost_one_seal_each() {
    appends_cost_one_seal_each(Arc::new(
        a2a_protocol_server::store::TenantAwareInMemoryTaskStore::new(),
    ))
    .await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_appends_cost_one_seal_each() {
    let path = std::env::temp_dir().join(format!("a2a-seal-cost-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store =
        a2a_protocol_server::store::SqliteTaskStore::new(&format!("sqlite://{}", path.display()))
            .await
            .expect("store");
    appends_cost_one_seal_each(Arc::new(store)).await;
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

/// `#[ignore]`d without a live server, like the other PostgreSQL suites.
#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs A2A_TEST_POSTGRES_URL"]
async fn postgres_appends_cost_one_seal_each() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
    let admin = sqlx::postgres::PgPool::connect(&url)
        .await
        .expect("connect");
    let name = format!("a2a_seal_cost_{}", std::process::id());
    let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .execute(&admin)
        .await;
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("create");
    let db = format!("{}/{name}", url.rsplit_once('/').expect("path").0);
    let store = a2a_protocol_server::store::PostgresTaskStore::new(&db)
        .await
        .expect("store");
    appends_cost_one_seal_each(Arc::new(store)).await;
    let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .execute(&admin)
        .await;
    admin.close().await;
}

/// A status write seals its message alone, not the task's other parts.
#[tokio::test]
async fn a_status_write_seals_only_its_message() {
    let cipher = cipher();
    let store = EncryptedTaskStore::new(
        InMemoryTaskStore::new(),
        Arc::clone(&cipher) as Arc<dyn ContentCipher>,
    );
    store.save(&task(50)).await.expect("first write");
    let before = cipher.seal_count();
    let mut next = task(50);
    next.status = TaskStatus::new(TaskState::InputRequired);
    next.status.message = Some(Message::agent_text("s", "what next?"));
    store.save_status_delta(&next).await.expect("status");
    assert_eq!(cipher.seal_count() - before, 1);
    let read = store
        .get(&TaskId::new("t-seal"))
        .await
        .expect("opens")
        .expect("stored");
    assert_eq!(read.status.state, TaskState::InputRequired);
    assert_eq!(
        read.status.message.unwrap().parts[0].text_content(),
        Some("what next?")
    );
    assert_eq!(read.artifacts.unwrap()[0].parts.len(), 50);
}

/// The store's required methods only: no in-place path at all.
struct WholeWrites(InMemoryTaskStore);

impl TaskStore for WholeWrites {
    fn save<'a>(&'a self, t: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.save(t)
    }
    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        self.0.get(id)
    }
    fn list<'a>(
        &'a self,
        p: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> {
        self.0.list(p)
    }
    fn insert_if_absent<'a>(
        &'a self,
        t: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.0.insert_if_absent(t)
    }
    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.delete(id)
    }
}

/// A store without the in-place methods still gets a correct record: the
/// wrapper seals the whole task and writes it.
#[tokio::test]
async fn a_store_without_in_place_writes_is_written_whole() {
    let cipher = cipher();
    let store = EncryptedTaskStore::new(
        WholeWrites(InMemoryTaskStore::new()),
        Arc::clone(&cipher) as Arc<dyn ContentCipher>,
    );
    stream(&store, &cipher).await;
    assert_every_chunk_reads_back(&store).await;
}

/// The same for a status write: with no in-place status path, the wrapper
/// writes the task whole and the new status reads back.
#[tokio::test]
async fn a_status_write_to_a_store_without_in_place_writes_is_written_whole() {
    let store = EncryptedTaskStore::new(
        WholeWrites(InMemoryTaskStore::new()),
        cipher() as Arc<dyn ContentCipher>,
    );
    store.save(&task(1)).await.expect("first write");
    let mut next = task(1);
    next.status = TaskStatus::new(TaskState::InputRequired);
    store.save_status_delta(&next).await.expect("status");
    let read = store
        .get(&TaskId::new("t-seal"))
        .await
        .expect("opens")
        .expect("stored");
    assert_eq!(read.status.state, TaskState::InputRequired);
}

/// A store that breaks the in-place contract and writes the task it was
/// handed whole stores placeholders, and the read fails: never plaintext,
/// never a part from another place.
struct WritesTheWholeTask(InMemoryTaskStore);

impl TaskStore for WritesTheWholeTask {
    fn save<'a>(&'a self, t: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.save(t)
    }
    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        self.0.get(id)
    }
    fn list<'a>(
        &'a self,
        p: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> {
        self.0.list(p)
    }
    fn insert_if_absent<'a>(
        &'a self,
        t: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.0.insert_if_absent(t)
    }
    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.delete(id)
    }
    fn try_save_artifact_delta<'a>(
        &'a self,
        t: &'a Task,
        _delta: ArtifactDelta,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        Box::pin(async move { self.0.save(t).await.map(|()| true) })
    }
}

#[tokio::test]
async fn a_store_that_writes_the_partial_task_whole_fails_closed() {
    for accept_unsealed in [false, true] {
        let cipher = cipher();
        let mut store = EncryptedTaskStore::new(
            WritesTheWholeTask(InMemoryTaskStore::new()),
            Arc::clone(&cipher) as Arc<dyn ContentCipher>,
        );
        if accept_unsealed {
            store = store.accept_unsealed_parts();
        }
        store.save(&task(1)).await.expect("first write");
        store
            .save_artifact_delta(
                &task(2),
                ArtifactDelta::AppendedParts { index: 0, count: 1 },
            )
            .await
            .expect("the misbehaving write");
        let read = store.get(&TaskId::new("t-seal")).await;
        assert!(
            read.is_err(),
            "accept_unsealed={accept_unsealed}: a placeholder was read back: {read:?}"
        );
    }
}

/// Past its limit the cipher refuses, the write fails, and the count does
/// not move: the budget is enforced, not only counted.
#[tokio::test]
async fn the_seal_limit_is_enforced() {
    let cipher = Aes256GcmCipher::new("k1", [5; AES_256_KEY_LEN])
        .expect("cipher")
        .with_seal_limit(3)
        .into_arc();
    for _ in 0..3 {
        cipher
            .seal(b"x".to_vec(), b"ctx")
            .await
            .expect("within the limit");
    }
    let refused = cipher
        .seal(b"x".to_vec(), b"ctx")
        .await
        .expect_err("past the limit");
    assert!(refused.message.contains("rotate"), "{refused:?}");
    assert_eq!(cipher.seal_count(), 3);

    let store = EncryptedTaskStore::new(InMemoryTaskStore::new(), cipher as Arc<dyn ContentCipher>);
    assert!(
        store.save(&task(1)).await.is_err(),
        "a write past the limit"
    );
}
