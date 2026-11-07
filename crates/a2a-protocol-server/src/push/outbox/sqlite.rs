// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`PushOutbox`] in SQLite (feature `sqlite`).
//!
//! The clock is the database's (`julianday('now')`, in milliseconds), which
//! is the same for every statement step, so a claim's "due" and its lease are
//! read from one instant. A claim is one `UPDATE ... RETURNING` statement, so
//! two processes sharing the file cannot both claim a row.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::error::A2aError;
use sqlx::Row as _;
use sqlx::sqlite::SqlitePool;

use super::body::{Body, context};
use super::{ClaimedDelivery, NewDelivery, OutboxFuture, PushOutbox};
use crate::push::PushConfigCipher;

/// Now, in milliseconds since the Unix epoch, by the database's clock: a
/// macro, so each statement is one `&'static str` (`concat!`), never text
/// formatted at run time.
macro_rules! now_ms {
    () => {
        "CAST((julianday('now') - 2440587.5) * 86400000.0 AS INTEGER)"
    };
}

const SCHEMA: [&str; 2] = [
    "CREATE TABLE IF NOT EXISTS a2a_push_outbox (
        seq         INTEGER PRIMARY KEY AUTOINCREMENT,
        delivery_id TEXT    NOT NULL UNIQUE,
        tenant      TEXT    NOT NULL,
        caller      TEXT,
        task_id     TEXT    NOT NULL,
        config_id   TEXT    NOT NULL,
        event       TEXT    NOT NULL,
        ends_turn   INTEGER NOT NULL,
        attempts    INTEGER NOT NULL DEFAULT 0,
        recorded_ms INTEGER NOT NULL,
        due_ms      INTEGER NOT NULL,
        owner       TEXT,
        lease_ms    INTEGER
    )",
    "CREATE INDEX IF NOT EXISTS a2a_push_outbox_webhook
        ON a2a_push_outbox (tenant, caller, task_id, config_id, seq)",
];

/// A [`PushOutbox`] in an SQLite database; see the module documentation.
#[derive(Debug, Clone)]
pub struct SqlitePushOutbox {
    pub(super) pool: SqlitePool,
    body: Body,
}

fn to_a2a_error(e: &sqlx::Error) -> A2aError {
    crate::sql_error::sql_error(crate::sql_error::Db::Sqlite, "sqlite push outbox", e)
}

/// Milliseconds in `d`, saturating.
fn millis(d: Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}

impl SqlitePushOutbox {
    /// Opens (or creates) the database at `url` and its table.
    ///
    /// # Errors
    ///
    /// The database cannot be opened, or the table created.
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        Self::from_pool(crate::sqlite_pool::sqlite_pool(url).await?).await
    }

    /// The outbox on `pool`, creating its table.
    ///
    /// # Errors
    ///
    /// The table cannot be created.
    pub async fn from_pool(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        for statement in SCHEMA {
            sqlx::query(statement).execute(&pool).await?;
        }
        Ok(Self {
            pool,
            body: Body::default(),
        })
    }

    /// Seals every event body with `cipher`, as the push-config stores seal
    /// configs (`PushConfigCipher`): an event can carry what the task store
    /// encrypts. A store that seals refuses a body it did not seal.
    #[must_use]
    pub fn with_cipher(mut self, cipher: Arc<dyn PushConfigCipher>) -> Self {
        self.body = Body::with_cipher(cipher);
        self
    }

    /// Records one delivery inside `tx`; whether a row was dropped.
    async fn record_one(
        tx: &mut sqlx::SqliteConnection,
        d: &NewDelivery,
        event: &str,
        per_webhook: usize,
    ) -> Result<usize, sqlx::Error> {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM a2a_push_outbox
             WHERE tenant = ?1 AND caller IS ?2 AND task_id = ?3 AND config_id = ?4",
        )
        .bind(&d.tenant)
        .bind(&d.caller)
        .bind(&d.task_id)
        .bind(&d.config_id)
        .fetch_one(&mut *tx)
        .await?;
        let dropped = if usize::try_from(waiting).unwrap_or(usize::MAX) >= per_webhook.max(1) {
            let removed = sqlx::query(concat!(
                "DELETE FROM a2a_push_outbox WHERE seq = (
                    SELECT seq FROM a2a_push_outbox
                    WHERE tenant = ?1 AND caller IS ?2 AND task_id = ?3 AND config_id = ?4
                      AND ends_turn = 0 AND (lease_ms IS NULL OR lease_ms <= ",
                now_ms!(),
                ")
                    ORDER BY seq LIMIT 1)"
            ))
            .bind(&d.tenant)
            .bind(&d.caller)
            .bind(&d.task_id)
            .bind(&d.config_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if removed == 0 && !d.ends_turn {
                return Ok(1);
            }
            usize::try_from(removed).unwrap_or(0)
        } else {
            0
        };
        sqlx::query(concat!(
            "INSERT INTO a2a_push_outbox
                (delivery_id, tenant, caller, task_id, config_id, event, ends_turn,
                 recorded_ms, due_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ",
            now_ms!(),
            ", ",
            now_ms!(),
            ")"
        ))
        .bind(&d.delivery_id)
        .bind(&d.tenant)
        .bind(&d.caller)
        .bind(&d.task_id)
        .bind(&d.config_id)
        .bind(event)
        .bind(d.ends_turn)
        .execute(&mut *tx)
        .await?;
        Ok(dropped)
    }

    /// One claimed row, read; `None`, said so, for a row whose event does
    /// not open.
    async fn read(&self, row: &sqlx::sqlite::SqliteRow) -> Option<ClaimedDelivery> {
        let (tenant, caller, task_id, delivery_id): (String, Option<String>, String, String) = (
            row.get("tenant"),
            row.get("caller"),
            row.get("task_id"),
            row.get("delivery_id"),
        );
        let ctx = context(&tenant, caller.as_deref(), &task_id, &delivery_id);
        // A body that does not open (moved from another row, or
        // sealed under a key this store does not hold) is left
        // leased and said so, rather than failing the claim: one bad
        // row must not stop every other delivery.
        let event = match self.body.decode(row.get::<&str, _>("event"), &ctx).await {
            Ok(event) => event,
            Err(_err) => {
                trace_warn!(
                    task_id = %task_id,
                    error = %_err,
                    "push outbox: a row's event could not be read; it is skipped"
                );
                return None;
            }
        };
        Some(ClaimedDelivery {
            seq: row.get("seq"),
            attempts: u32::try_from(row.get::<i64, _>("attempts")).unwrap_or(u32::MAX),
            age: Duration::from_millis(u64::try_from(row.get::<i64, _>("age_ms")).unwrap_or(0)),
            delivery: NewDelivery {
                delivery_id,
                tenant,
                caller,
                task_id,
                config_id: row.get("config_id"),
                event,
                ends_turn: row.get("ends_turn"),
            },
        })
    }
}

impl PushOutbox for SqlitePushOutbox {
    fn record(&self, deliveries: Vec<NewDelivery>, per_webhook: usize) -> OutboxFuture<'_, usize> {
        Box::pin(async move {
            let mut bodies = Vec::with_capacity(deliveries.len());
            for d in &deliveries {
                let ctx = context(&d.tenant, d.caller.as_deref(), &d.task_id, &d.delivery_id);
                bodies.push(self.body.encode(&d.event, &ctx).await?);
            }
            let mut tx = self.pool.begin().await.map_err(|e| to_a2a_error(&e))?;
            let mut dropped = 0;
            for (d, event) in deliveries.iter().zip(&bodies) {
                dropped += Self::record_one(&mut tx, d, event, per_webhook)
                    .await
                    .map_err(|e| to_a2a_error(&e))?;
            }
            tx.commit().await.map_err(|e| to_a2a_error(&e))?;
            Ok(dropped)
        })
    }

    fn claim<'a>(
        &'a self,
        owner: &'a str,
        max: usize,
        lease: Duration,
    ) -> OutboxFuture<'a, Vec<ClaimedDelivery>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "UPDATE a2a_push_outbox SET owner = ?1, lease_ms = ",
                now_ms!(),
                " + ?2
                 WHERE seq IN (
                    SELECT seq FROM a2a_push_outbox o
                    WHERE o.due_ms <= ",
                now_ms!(),
                "
                      AND (o.lease_ms IS NULL OR o.lease_ms <= ",
                now_ms!(),
                ")
                      AND NOT EXISTS (
                        SELECT 1 FROM a2a_push_outbox e
                        WHERE e.tenant = o.tenant AND e.caller IS o.caller
                          AND e.task_id = o.task_id AND e.config_id = o.config_id
                          AND e.seq < o.seq)
                    ORDER BY o.seq LIMIT ?3)
                 RETURNING seq, delivery_id, tenant, caller, task_id, config_id, event,
                           ends_turn, attempts, ",
                now_ms!(),
                " - recorded_ms AS age_ms"
            ))
            .bind(owner)
            .bind(millis(lease))
            .bind(i64::try_from(max).unwrap_or(i64::MAX))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;
            let mut claimed = Vec::with_capacity(rows.len());
            for row in &rows {
                if let Some(read) = self.read(row).await {
                    claimed.push(read);
                }
            }
            claimed.sort_by_key(|c| c.seq);
            Ok(claimed)
        })
    }

    fn complete(&self, seq: i64) -> OutboxFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query("DELETE FROM a2a_push_outbox WHERE seq = ?1")
                .bind(seq)
                .execute(&self.pool)
                .await
                .map_err(|e| to_a2a_error(&e))?;
            Ok(())
        })
    }

    fn retry_after<'a>(
        &'a self,
        seq: i64,
        owner: &'a str,
        after: Duration,
    ) -> OutboxFuture<'a, ()> {
        Box::pin(async move {
            sqlx::query(concat!(
                "UPDATE a2a_push_outbox
                 SET attempts = attempts + 1, due_ms = ",
                now_ms!(),
                " + ?3, owner = NULL, lease_ms = NULL
                 WHERE seq = ?1 AND owner = ?2"
            ))
            .bind(seq)
            .bind(owner)
            .bind(millis(after))
            .execute(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;
            Ok(())
        })
    }

    fn pending(&self) -> OutboxFuture<'_, usize> {
        Box::pin(async move {
            let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM a2a_push_outbox")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| to_a2a_error(&e))?;
            Ok(usize::try_from(n).unwrap_or(usize::MAX))
        })
    }
}
