// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Release audit F5: the SQL stores' artifact appends are compare-and-append.
//!
//! Two replicas hold the same task. Replica A appends; replica B, whose
//! snapshot predates A's write, appends too. Before this fix B's `UPDATE`
//! matched on the task id alone and concatenated its tail after A's, so the
//! stored artifact held parts neither replica's snapshot did — and under
//! `EncryptedTaskStore`, whose parts are sealed to their positions, the
//! record could no longer be read. Now B's `UPDATE` matches only the length
//! (and, for parts, the artifact id) its snapshot assumed; zero rows sends it
//! to a whole-record `save`, which is a lost update at worst and never an
//! unreadable record.
//!
//! `#[ignore]`d without a live server, like the other PostgreSQL suites:
//!
//! ```bash
//! A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
//!   cargo test -p a2a-protocol-server --features postgres,encryption \
//!   --test artifact_compare_and_append -- --include-ignored
//! ```

use std::sync::Arc;

use a2a_protocol_server::store::{ArtifactDelta, TaskStore};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

/// A scratch database, dropped (forcibly) at the end.
#[cfg(feature = "postgres")]
struct Db {
    admin: sqlx::postgres::PgPool,
    name: String,
    url: String,
}

#[cfg(feature = "postgres")]
impl Db {
    async fn create(tag: &str) -> Self {
        let admin_url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");
        let admin = sqlx::postgres::PgPool::connect(&admin_url)
            .await
            .expect("connect");
        let name = format!("a2a_cas_{tag}_{}", std::process::id());
        let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
            .execute(&admin)
            .await;
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .expect("create database");
        let url = format!("{}/{name}", admin_url.rsplit_once('/').expect("path").0);
        Self { admin, name, url }
    }

    async fn replica(&self) -> Arc<dyn TaskStore> {
        Arc::new(
            a2a_protocol_server::store::PostgresTaskStore::new(&self.url)
                .await
                .expect("store"),
        )
    }

    async fn drop_db(self) {
        let _ = sqlx::query(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ))
        .execute(&self.admin)
        .await;
        self.admin.close().await;
    }
}

fn task(artifacts: Vec<Artifact>) -> Task {
    Task {
        id: TaskId::new("t-cas"),
        context_id: ContextId::new("ctx"),
        status: TaskStatus::new(TaskState::Working),
        history: None,
        artifacts: Some(artifacts),
        metadata: None,
    }
}

fn parts(texts: &[&str]) -> Vec<Part> {
    texts.iter().map(|t| Part::text(*t)).collect()
}

fn texts(task: &Task) -> Vec<Vec<String>> {
    task.artifacts
        .iter()
        .flatten()
        .map(|a| {
            a.parts
                .iter()
                .map(|p| p.text_content().unwrap_or("<sealed>").to_owned())
                .collect()
        })
        .collect()
}

/// B appends a part after A did, from a snapshot that has not seen A's.
async fn stale_part_append(a: &dyn TaskStore, b: &dyn TaskStore) {
    a.save(&task(vec![Artifact::new("art", parts(&["p0"]))]))
        .await
        .expect("seed");
    a.save_artifact_delta(
        &task(vec![Artifact::new("art", parts(&["p0", "a1"]))]),
        ArtifactDelta::AppendedParts { index: 0, count: 1 },
    )
    .await
    .expect("A appends");
    b.save_artifact_delta(
        &task(vec![Artifact::new("art", parts(&["p0", "b1"]))]),
        ArtifactDelta::AppendedParts { index: 0, count: 1 },
    )
    .await
    .expect("B appends");
}

/// A replaces the artifact at index 0 with another of the same length; B,
/// whose snapshot still holds the old one, appends a part to it. The length
/// matches, so only the artifact id tells the two apart.
async fn append_to_a_replaced_artifact(a: &dyn TaskStore, b: &dyn TaskStore) {
    a.save(&task(vec![Artifact::new("art", parts(&["p0"]))]))
        .await
        .expect("seed");
    a.save(&task(vec![Artifact::new("other", parts(&["x"]))]))
        .await
        .expect("A replaces");
    b.save_artifact_delta(
        &task(vec![Artifact::new("art", parts(&["p0", "b1"]))]),
        ArtifactDelta::AppendedParts { index: 0, count: 1 },
    )
    .await
    .expect("B appends");
}

fn ids_and_texts(task: &Task) -> (Vec<String>, Vec<Vec<String>>) {
    (
        task.artifacts
            .iter()
            .flatten()
            .map(|a| a.id.0.clone())
            .collect(),
        texts(task),
    )
}

/// B pushes a second artifact after A did, from a snapshot with one.
async fn stale_push(a: &dyn TaskStore, b: &dyn TaskStore) {
    let first = Artifact::new("first", parts(&["f"]));
    a.save(&task(vec![first.clone()])).await.expect("seed");
    a.save_artifact_delta(
        &task(vec![first.clone(), Artifact::new("from-a", parts(&["a"]))]),
        ArtifactDelta::Pushed { index: 1 },
    )
    .await
    .expect("A pushes");
    b.save_artifact_delta(
        &task(vec![first, Artifact::new("from-b", parts(&["b"]))]),
        ArtifactDelta::Pushed { index: 1 },
    )
    .await
    .expect("B pushes");
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs A2A_TEST_POSTGRES_URL"]
async fn a_stale_part_append_is_rewritten_whole_not_concatenated() {
    let db = Db::create("parts").await;
    let (a, b) = (db.replica().await, db.replica().await);
    stale_part_append(&*a, &*b).await;
    let stored = a
        .get(&TaskId::new("t-cas"))
        .await
        .expect("get")
        .expect("stored");
    assert_eq!(
        texts(&stored),
        [["p0", "b1"]],
        "B's snapshot, whole; before the fix [p0, a1, b1], which neither replica held"
    );
    drop((a, b));
    db.drop_db().await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs A2A_TEST_POSTGRES_URL"]
async fn a_stale_push_is_rewritten_whole_not_appended() {
    let db = Db::create("push").await;
    let (a, b) = (db.replica().await, db.replica().await);
    stale_push(&*a, &*b).await;
    let stored = a
        .get(&TaskId::new("t-cas"))
        .await
        .expect("get")
        .expect("stored");
    let ids: Vec<&str> = stored
        .artifacts
        .iter()
        .flatten()
        .map(|a| a.id.0.as_str())
        .collect();
    assert_eq!(
        ids,
        ["first", "from-b"],
        "before the fix [first, from-a, from-b]"
    );
    drop((a, b));
    db.drop_db().await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "needs A2A_TEST_POSTGRES_URL"]
async fn a_part_for_a_replaced_artifact_is_not_appended_to_its_successor() {
    let db = Db::create("replaced").await;
    let (a, b) = (db.replica().await, db.replica().await);
    append_to_a_replaced_artifact(&*a, &*b).await;
    let stored = a
        .get(&TaskId::new("t-cas"))
        .await
        .expect("get")
        .expect("stored");
    assert_eq!(
        ids_and_texts(&stored),
        (
            vec!["art".to_owned()],
            vec![vec!["p0".to_owned(), "b1".to_owned()]]
        ),
        "without the id check, B's part joins `other`: [other: x, b1]"
    );
    drop((a, b));
    db.drop_db().await;
}

/// The reported failure: under `EncryptedTaskStore` a stale append left a
/// part sealed for one position stored at another, and the record failed
/// to decrypt.
#[cfg(all(feature = "postgres", feature = "encryption"))]
#[tokio::test]
#[ignore = "needs A2A_TEST_POSTGRES_URL"]
async fn an_encrypted_record_stays_readable_after_a_stale_append() {
    use a2a_protocol_server::store::encrypted::{
        AES_256_KEY_LEN, Aes256GcmCipher, EncryptedTaskStore,
    };
    let db = Db::create("sealed").await;
    let cipher: Arc<dyn a2a_protocol_server::store::encrypted::ContentCipher> =
        Aes256GcmCipher::new("k1", [7; AES_256_KEY_LEN])
            .expect("cipher")
            .into_arc();
    let a = EncryptedTaskStore::from_arc(db.replica().await, Arc::clone(&cipher));
    let b = EncryptedTaskStore::from_arc(db.replica().await, cipher);
    stale_part_append(&a, &b).await;
    let stored = a
        .get(&TaskId::new("t-cas"))
        .await
        .expect("the record decrypts")
        .expect("stored");
    assert_eq!(texts(&stored), [["p0", "b1"]]);
    drop((a, b));
    db.drop_db().await;
}

/// The same three cases on SQLite, two pools on one database file: two
/// processes on one host.
#[cfg(feature = "sqlite")]
mod sqlite {
    use super::*;
    use a2a_protocol_server::store::SqliteTaskStore;

    struct File(std::path::PathBuf);

    impl File {
        fn new(tag: &str) -> Self {
            let f =
                Self(std::env::temp_dir().join(format!("a2a-cas-{tag}-{}.db", std::process::id())));
            f.remove();
            f
        }
        fn url(&self) -> String {
            format!("sqlite://{}", self.0.display())
        }
        async fn replica(&self) -> Arc<dyn TaskStore> {
            Arc::new(SqliteTaskStore::new(&self.url()).await.expect("store"))
        }
        fn remove(&self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
            }
        }
    }

    impl Drop for File {
        fn drop(&mut self) {
            self.remove();
        }
    }

    #[tokio::test]
    async fn a_stale_part_append_is_rewritten_whole() {
        let file = File::new("parts");
        let (a, b) = (file.replica().await, file.replica().await);
        stale_part_append(&*a, &*b).await;
        let stored = a
            .get(&TaskId::new("t-cas"))
            .await
            .expect("get")
            .expect("stored");
        assert_eq!(texts(&stored), [["p0", "b1"]]);
    }

    #[tokio::test]
    async fn a_stale_push_is_rewritten_whole() {
        let file = File::new("push");
        let (a, b) = (file.replica().await, file.replica().await);
        stale_push(&*a, &*b).await;
        let stored = a
            .get(&TaskId::new("t-cas"))
            .await
            .expect("get")
            .expect("stored");
        let ids: Vec<&str> = stored
            .artifacts
            .iter()
            .flatten()
            .map(|a| a.id.0.as_str())
            .collect();
        assert_eq!(ids, ["first", "from-b"]);
    }

    #[tokio::test]
    async fn a_part_for_a_replaced_artifact_is_not_appended_to_its_successor() {
        let file = File::new("replaced");
        let (a, b) = (file.replica().await, file.replica().await);
        append_to_a_replaced_artifact(&*a, &*b).await;
        let stored = a
            .get(&TaskId::new("t-cas"))
            .await
            .expect("get")
            .expect("stored");
        assert_eq!(
            ids_and_texts(&stored),
            (
                vec!["art".to_owned()],
                vec![vec!["p0".to_owned(), "b1".to_owned()]]
            )
        );
    }

    #[cfg(feature = "encryption")]
    #[tokio::test]
    async fn an_encrypted_record_stays_readable() {
        use a2a_protocol_server::store::encrypted::{
            AES_256_KEY_LEN, Aes256GcmCipher, EncryptedTaskStore,
        };
        let file = File::new("sealed");
        let cipher: Arc<dyn a2a_protocol_server::store::encrypted::ContentCipher> =
            Aes256GcmCipher::new("k1", [7; AES_256_KEY_LEN])
                .expect("cipher")
                .into_arc();
        let a = EncryptedTaskStore::from_arc(file.replica().await, Arc::clone(&cipher));
        let b = EncryptedTaskStore::from_arc(file.replica().await, cipher);
        stale_part_append(&a, &b).await;
        let stored = a
            .get(&TaskId::new("t-cas"))
            .await
            .expect("the record decrypts")
            .expect("stored");
        assert_eq!(texts(&stored), [["p0", "b1"]]);
    }
}

/// The in-memory store applies the same deltas in place, and two handlers in
/// one process can share it: the same two cases against one shared store.
mod in_memory {
    use super::*;
    use a2a_protocol_server::store::InMemoryTaskStore;

    #[tokio::test]
    async fn a_stale_part_append_is_rewritten_whole() {
        let shared: Arc<dyn TaskStore> = Arc::new(InMemoryTaskStore::new());
        stale_part_append(&*shared, &*shared).await;
        let stored = shared
            .get(&TaskId::new("t-cas"))
            .await
            .expect("get")
            .expect("stored");
        assert_eq!(texts(&stored), [["p0", "b1"]]);
    }

    #[tokio::test]
    async fn a_part_for_a_replaced_artifact_is_not_appended_to_its_successor() {
        let shared: Arc<dyn TaskStore> = Arc::new(InMemoryTaskStore::new());
        append_to_a_replaced_artifact(&*shared, &*shared).await;
        let stored = shared
            .get(&TaskId::new("t-cas"))
            .await
            .expect("get")
            .expect("stored");
        assert_eq!(
            ids_and_texts(&stored),
            (
                vec!["art".to_owned()],
                vec![vec!["p0".to_owned(), "b1".to_owned()]]
            )
        );
    }

    #[tokio::test]
    async fn a_stale_push_is_rewritten_whole() {
        let shared: Arc<dyn TaskStore> = Arc::new(InMemoryTaskStore::new());
        stale_push(&*shared, &*shared).await;
        let stored = shared
            .get(&TaskId::new("t-cas"))
            .await
            .expect("get")
            .expect("stored");
        let ids: Vec<&str> = stored
            .artifacts
            .iter()
            .flatten()
            .map(|a| a.id.0.as_str())
            .collect();
        assert_eq!(ids, ["first", "from-b"]);
    }
}
