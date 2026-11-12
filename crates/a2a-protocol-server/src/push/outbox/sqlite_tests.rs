// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The SQLite outbox against the contract, across a reopen of its file, and
//! with its bodies sealed.

#![cfg(feature = "sqlite")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::task::TaskState;

use super::contract::{self, delivery};
use super::*;
use crate::push::PushConfigCipher;

#[tokio::test]
async fn the_sqlite_outbox_keeps_the_contract() {
    contract::run_all(|| async { SqlitePushOutbox::new("sqlite::memory:").await.unwrap() }).await;
}

/// A file outbox in the system's temporary directory, removed on drop.
struct TempDb(std::path::PathBuf);

impl TempDb {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!(
            "a2a-outbox-{name}-{}-{}.db",
            std::process::id(),
            uuid::Uuid::new_v4()
        )))
    }

    fn url(&self) -> String {
        format!("sqlite://{}", self.0.display())
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

/// What one process recorded, another opening the same file claims, with
/// the same delivery id: the restart the outbox exists for.
#[tokio::test]
async fn rows_outlive_the_outbox_that_recorded_them() {
    let db = TempDb::new("reopen");
    let row = delivery("t", "a", TaskState::Completed);
    let id = row.delivery_id.clone();
    {
        let first = SqlitePushOutbox::new(&db.url()).await.unwrap();
        first.record(vec![row], 10).await.unwrap();
        // Claimed and then the process "stops": its lease runs out (zero).
        first.claim("gone", 10, Duration::ZERO).await.unwrap();
        first.pool.close().await;
    }
    let second = SqlitePushOutbox::new(&db.url()).await.unwrap();
    let claimed = second
        .claim("next", 10, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].delivery.delivery_id, id);
    assert_eq!(claimed[0].delivery.tenant, "t");
    assert!(claimed[0].delivery.ends_turn);
    second.pool.close().await;
}

/// A cipher that prefixes the context it sealed under and refuses to open
/// under any other: enough to see the body is sealed and bound to its row.
struct Bound;

impl PushConfigCipher for Bound {
    fn seal<'a>(
        &'a self,
        plaintext: Vec<u8>,
        context: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<u8>>> + Send + 'a>> {
        Box::pin(async move {
            let mut sealed = u32::try_from(context.len()).unwrap().to_be_bytes().to_vec();
            sealed.extend_from_slice(context);
            sealed.extend(plaintext.iter().map(|b| b ^ 0x5a));
            Ok(sealed)
        })
    }

    fn open<'a>(
        &'a self,
        sealed: Vec<u8>,
        context: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<u8>>> + Send + 'a>> {
        Box::pin(async move {
            let len = usize::try_from(u32::from_be_bytes(sealed[..4].try_into().unwrap())).unwrap();
            if &sealed[4..4 + len] != context {
                return Err(A2aError::internal("sealed under another context"));
            }
            Ok(sealed[4 + len..].iter().map(|b| b ^ 0x5a).collect())
        })
    }
}

/// With a cipher the body is stored sealed, not as the event's JSON, and
/// comes back as it went in.
#[tokio::test]
async fn a_sealed_body_is_not_stored_as_json_and_opens() {
    let outbox = SqlitePushOutbox::new("sqlite::memory:")
        .await
        .unwrap()
        .with_cipher(Arc::new(Bound));
    let row = delivery("t", "a", TaskState::Working);
    let event = row.event.clone();
    outbox.record(vec![row], 10).await.unwrap();
    let stored: String = sqlx::query_scalar("SELECT event FROM a2a_push_outbox")
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
    assert!(stored.starts_with("a2a-sealed:v1:"), "{stored}");
    assert!(!stored.contains("TASK_STATE_WORKING"));
    let claimed = outbox
        .claim("me", 10, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&claimed[0].delivery.event).unwrap(),
        serde_json::to_value(&event).unwrap()
    );
}

/// A sealed body moved to another row does not open there; a sealed body
/// without a cipher, or a plain one with a cipher, is refused: such a row is
/// skipped by the claim, and kept.
#[tokio::test]
async fn a_body_opens_only_in_its_own_row_and_with_its_own_kind_of_store() {
    let sealed = SqlitePushOutbox::new("sqlite::memory:")
        .await
        .unwrap()
        .with_cipher(Arc::new(Bound));
    sealed
        .record(
            vec![
                delivery("t", "a", TaskState::Working),
                delivery("t", "b", TaskState::Working),
            ],
            10,
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE a2a_push_outbox SET event = (SELECT event FROM a2a_push_outbox WHERE config_id = 'b')
         WHERE config_id = 'a'",
    )
    .execute(&sealed.pool)
    .await
    .unwrap();
    let claimed = sealed.claim("me", 10, Duration::ZERO).await.unwrap();
    assert_eq!(
        claimed
            .iter()
            .map(|c| c.delivery.config_id.as_str())
            .collect::<Vec<_>>(),
        ["b"],
        "the moved body is skipped, and does not stop the other"
    );

    let plain = SqlitePushOutbox::from_pool(sealed.pool.clone())
        .await
        .unwrap();
    assert!(
        plain
            .claim("me", 10, Duration::ZERO)
            .await
            .unwrap()
            .is_empty(),
        "no cipher"
    );

    let unsealed = SqlitePushOutbox::new("sqlite::memory:").await.unwrap();
    unsealed
        .record(vec![delivery("t", "a", TaskState::Working)], 10)
        .await
        .unwrap();
    let with_cipher = SqlitePushOutbox::from_pool(unsealed.pool.clone())
        .await
        .unwrap()
        .with_cipher(Arc::new(Bound));
    assert!(
        with_cipher
            .claim("me", 10, Duration::ZERO)
            .await
            .unwrap()
            .is_empty(),
        "unsealed"
    );
    assert_eq!(unsealed.pending().await.unwrap(), 1, "skipped, not deleted");
}

#[tokio::test]
async fn the_sqlite_outbox_shows_whether_it_seals() {
    let outbox = SqlitePushOutbox::new("sqlite::memory:").await.unwrap();
    assert!(format!("{outbox:?}").contains("Body { sealed: false }"));
    let outbox = outbox.with_cipher(Arc::new(Bound));
    assert!(format!("{outbox:?}").contains("Body { sealed: true }"));
}

/// A reservation outlives the process that made it: the file, reopened, lists
/// it as held and arming it from there makes its row claimable. This is the
/// state a crash between "reserve" and "arm" leaves behind (audit H14).
#[tokio::test]
async fn a_reservation_outlives_the_outbox_that_made_it() {
    let db = TempDb::new("reserved");
    let row = delivery("t", "a", TaskState::Working);
    let id = row.delivery_id.clone();
    {
        let first = SqlitePushOutbox::new(&db.url()).await.unwrap();
        first
            .reserve("r1", 9, vec![row], 10, Duration::ZERO)
            .await
            .unwrap();
        assert!(
            first
                .claim("x", 10, Duration::from_secs(60))
                .await
                .unwrap()
                .is_empty()
        );
        first.pool.close().await;
    }
    let second = SqlitePushOutbox::new(&db.url()).await.unwrap();
    let held = second.held(10).await.unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!((held[0].reservation.as_str(), held[0].log_seq), ("r1", 9));
    assert_eq!(second.arm("r1").await.unwrap(), 1);
    let claimed = second
        .claim("y", 10, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(claimed[0].delivery.delivery_id, id);
}

/// A table an earlier build created, without the two-phase columns, is
/// upgraded in place and its rows stay armed.
#[tokio::test]
async fn a_table_without_the_reservation_columns_is_upgraded() {
    let db = TempDb::new("upgrade");
    let pool = crate::sqlite_pool::sqlite_pool(&db.url()).await.unwrap();
    sqlx::query(
        "CREATE TABLE a2a_push_outbox (
            seq INTEGER PRIMARY KEY AUTOINCREMENT, delivery_id TEXT NOT NULL UNIQUE,
            tenant TEXT NOT NULL, caller TEXT, task_id TEXT NOT NULL, config_id TEXT NOT NULL,
            event TEXT NOT NULL, ends_turn INTEGER NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
            recorded_ms INTEGER NOT NULL, due_ms INTEGER NOT NULL, owner TEXT, lease_ms INTEGER)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let old = delivery("t", "a", TaskState::Working);
    let body = serde_json::to_string(&old.event).unwrap();
    sqlx::query(
        "INSERT INTO a2a_push_outbox (delivery_id, tenant, task_id, config_id, event, ends_turn,
            recorded_ms, due_ms) VALUES (?1, 't', 't1', 'a', ?2, 0, 0, 0)",
    )
    .bind(&old.delivery_id)
    .bind(body)
    .execute(&pool)
    .await
    .unwrap();

    let outbox = SqlitePushOutbox::from_pool(pool).await.unwrap();
    // Opening it again changes nothing.
    SqlitePushOutbox::from_pool(outbox.pool.clone())
        .await
        .unwrap();
    let claimed = outbox
        .claim("me", 10, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(
        claimed.len(),
        1,
        "the row from before the columns existed is armed"
    );
    assert!(outbox.held(10).await.unwrap().is_empty());
}
