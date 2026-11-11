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
use super::{ClaimedDelivery, Held, NewDelivery, OutboxFuture, PushOutbox};
use crate::push::PushConfigCipher;

/// Now, in milliseconds since the Unix epoch, by the database's clock: a
/// macro, so each statement is one `&'static str` (`concat!`), never text
/// formatted at run time.
macro_rules! now_ms {
    () => {
        "CAST((julianday('now') - 2440587.5) * 86400000.0 AS INTEGER)"
    };
}

const SCHEMA: [&str; 4] = [
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
        lease_ms    INTEGER,
        armed       INTEGER NOT NULL DEFAULT 1,
        reservation TEXT,
        log_seq     INTEGER,
        hold_ms     INTEGER
    )",
    "CREATE INDEX IF NOT EXISTS a2a_push_outbox_webhook
        ON a2a_push_outbox (tenant, caller, task_id, config_id, seq)",
    "CREATE INDEX IF NOT EXISTS a2a_push_outbox_reservation
        ON a2a_push_outbox (reservation) WHERE reservation IS NOT NULL",
    // What `held` looks for, without reading a backlog of armed rows.
    "CREATE INDEX IF NOT EXISTS a2a_push_outbox_unarmed
        ON a2a_push_outbox (hold_ms) WHERE armed = 0",
];

/// The columns of the two-phase record (audit H14), added to a table an
/// earlier build of this outbox created without them. `SQLite` has no
/// `ADD COLUMN IF NOT EXISTS`, so each is looked for first.
const TWO_PHASE_COLUMNS: [(&str, &str); 4] = [
    (
        "armed",
        "ALTER TABLE a2a_push_outbox ADD COLUMN armed INTEGER NOT NULL DEFAULT 1",
    ),
    (
        "reservation",
        "ALTER TABLE a2a_push_outbox ADD COLUMN reservation TEXT",
    ),
    (
        "log_seq",
        "ALTER TABLE a2a_push_outbox ADD COLUMN log_seq INTEGER",
    ),
    (
        "hold_ms",
        "ALTER TABLE a2a_push_outbox ADD COLUMN hold_ms INTEGER",
    ),
];

/// How a row being inserted was reserved, when it was.
struct Reserve<'a> {
    id: &'a str,
    log_seq: i64,
    hold_ms: i64,
}

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
        sqlx::query(SCHEMA[0]).execute(&pool).await?;
        for (column, alter) in TWO_PHASE_COLUMNS {
            let present: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM pragma_table_info('a2a_push_outbox') WHERE name = ?1",
            )
            .bind(column)
            .fetch_one(&pool)
            .await?;
            if present == 0 {
                sqlx::query(alter).execute(&pool).await?;
            }
        }
        for statement in SCHEMA.iter().skip(1).copied() {
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

    /// Makes room under `per_webhook` for one more row of `d`'s webhook: how
    /// many rows it dropped, or `None` when there is nothing to drop and `d`
    /// does not end a turn, so `d` is the row not kept.
    async fn make_room(
        tx: &mut sqlx::SqliteConnection,
        d: &NewDelivery,
        per_webhook: usize,
    ) -> Result<Option<usize>, sqlx::Error> {
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
        if usize::try_from(waiting).unwrap_or(usize::MAX) < per_webhook.max(1) {
            return Ok(Some(0));
        }
        let removed = sqlx::query(concat!(
            "DELETE FROM a2a_push_outbox WHERE seq = (
                SELECT seq FROM a2a_push_outbox
                WHERE tenant = ?1 AND caller IS ?2 AND task_id = ?3 AND config_id = ?4
                  AND ends_turn = 0 AND armed = 1 AND (lease_ms IS NULL OR lease_ms <= ",
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
            return Ok(None);
        }
        Ok(Some(usize::try_from(removed).unwrap_or(0)))
    }

    /// Records one delivery inside `tx`, reserved when `reserve` is given;
    /// whether a row was dropped.
    async fn record_one(
        tx: &mut sqlx::SqliteConnection,
        d: &NewDelivery,
        event: &str,
        per_webhook: usize,
        reserve: Option<&Reserve<'_>>,
    ) -> Result<usize, sqlx::Error> {
        // Recorded already (an id is a row's identity): left as it is.
        let present: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM a2a_push_outbox WHERE delivery_id = ?1")
                .bind(&d.delivery_id)
                .fetch_one(&mut *tx)
                .await?;
        if present > 0 {
            return Ok(0);
        }
        let Some(dropped) = Self::make_room(tx, d, per_webhook).await? else {
            return Ok(1);
        };
        sqlx::query(concat!(
            "INSERT INTO a2a_push_outbox
                (delivery_id, tenant, caller, task_id, config_id, event, ends_turn,
                 armed, reservation, log_seq, hold_ms, recorded_ms, due_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                     CASE WHEN ?11 IS NULL THEN NULL ELSE ",
            now_ms!(),
            " + ?11 END, ",
            now_ms!(),
            ", ",
            now_ms!(),
            ") ON CONFLICT (delivery_id) DO NOTHING"
        ))
        .bind(&d.delivery_id)
        .bind(&d.tenant)
        .bind(&d.caller)
        .bind(&d.task_id)
        .bind(&d.config_id)
        .bind(event)
        .bind(d.ends_turn)
        .bind(reserve.is_none())
        .bind(reserve.map(|r| r.id))
        .bind(reserve.map(|r| r.log_seq))
        .bind(reserve.map(|r| r.hold_ms))
        .execute(&mut *tx)
        .await?;
        Ok(dropped)
    }

    /// The reservations `rows` name, their events opened; one that does not open
    /// is left in place and said so.
    async fn read_held(&self, rows: &[sqlx::sqlite::SqliteRow]) -> Vec<Held> {
        let mut held = Vec::with_capacity(rows.len());
        for row in rows {
            let (tenant, caller, task_id, delivery_id): (String, Option<String>, String, String) = (
                row.get("tenant"),
                row.get("caller"),
                row.get("task_id"),
                row.get("delivery_id"),
            );
            let ctx = context(&tenant, caller.as_deref(), &task_id, &delivery_id);
            let Ok(event) = self.body.decode(row.get::<&str, _>("event"), &ctx).await else {
                trace_warn!(
                    task_id = %task_id,
                    "push outbox: a reserved row's event could not be read; it is left"
                );
                continue;
            };
            held.push(Held {
                reservation: row.get("reservation"),
                tenant,
                caller,
                task_id,
                log_seq: u64::try_from(row.get::<i64, _>("log_seq")).unwrap_or(0),
                event,
            });
        }
        held
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

impl SqlitePushOutbox {
    /// Records `deliveries` in one transaction, reserved when `reserve` is
    /// given; how many were dropped for the bound.
    async fn insert(
        &self,
        deliveries: Vec<NewDelivery>,
        per_webhook: usize,
        reserve: Option<Reserve<'_>>,
    ) -> a2a_protocol_types::error::A2aResult<usize> {
        let mut bodies = Vec::with_capacity(deliveries.len());
        for d in &deliveries {
            let ctx = context(&d.tenant, d.caller.as_deref(), &d.task_id, &d.delivery_id);
            bodies.push(self.body.encode(&d.event, &ctx).await?);
        }
        let mut tx = self.pool.begin().await.map_err(|e| to_a2a_error(&e))?;
        let mut dropped = 0;
        for (d, event) in deliveries.iter().zip(&bodies) {
            dropped += Self::record_one(&mut tx, d, event, per_webhook, reserve.as_ref())
                .await
                .map_err(|e| to_a2a_error(&e))?;
        }
        tx.commit().await.map_err(|e| to_a2a_error(&e))?;
        Ok(dropped)
    }
}

impl PushOutbox for SqlitePushOutbox {
    fn record(&self, deliveries: Vec<NewDelivery>, per_webhook: usize) -> OutboxFuture<'_, usize> {
        Box::pin(self.insert(deliveries, per_webhook, None))
    }

    fn supports_reservations(&self) -> bool {
        true
    }

    fn reserve<'a>(
        &'a self,
        reservation: &'a str,
        log_seq: u64,
        deliveries: Vec<NewDelivery>,
        per_webhook: usize,
        hold: Duration,
    ) -> OutboxFuture<'a, usize> {
        Box::pin(self.insert(
            deliveries,
            per_webhook,
            Some(Reserve {
                id: reservation,
                log_seq: i64::try_from(log_seq).unwrap_or(i64::MAX),
                hold_ms: millis(hold),
            }),
        ))
    }

    fn arm<'a>(&'a self, reservation: &'a str) -> OutboxFuture<'a, usize> {
        Box::pin(async move {
            let armed = sqlx::query("UPDATE a2a_push_outbox SET armed = 1 WHERE reservation = ?1")
                .bind(reservation)
                .execute(&self.pool)
                .await
                .map_err(|e| to_a2a_error(&e))?
                .rows_affected();
            Ok(usize::try_from(armed).unwrap_or(usize::MAX))
        })
    }

    fn release<'a>(&'a self, reservation: &'a str) -> OutboxFuture<'a, ()> {
        Box::pin(async move {
            sqlx::query("DELETE FROM a2a_push_outbox WHERE reservation = ?1 AND armed = 0")
                .bind(reservation)
                .execute(&self.pool)
                .await
                .map_err(|e| to_a2a_error(&e))?;
            Ok(())
        })
    }

    fn held(&self, limit: usize) -> OutboxFuture<'_, Vec<Held>> {
        Box::pin(async move {
            let rows = sqlx::query(concat!(
                "SELECT delivery_id, tenant, caller, task_id, event, log_seq, reservation
                 FROM a2a_push_outbox
                 WHERE seq IN (
                    SELECT MIN(seq) FROM a2a_push_outbox
                    WHERE armed = 0 AND hold_ms <= ",
                now_ms!(),
                "
                    GROUP BY reservation ORDER BY MIN(seq) LIMIT ?1)
                 ORDER BY seq"
            ))
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;
            Ok(self.read_held(&rows).await)
        })
    }

    fn held_for_task<'a>(
        &'a self,
        tenant: &'a str,
        caller: Option<&'a str>,
        task_id: &'a str,
    ) -> OutboxFuture<'a, Vec<Held>> {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT delivery_id, tenant, caller, task_id, event, log_seq, reservation
                 FROM a2a_push_outbox
                 WHERE seq IN (
                    SELECT MIN(seq) FROM a2a_push_outbox
                    WHERE armed = 0 AND tenant = ?1 AND caller IS ?2 AND task_id = ?3
                    GROUP BY reservation)
                 ORDER BY seq",
            )
            .bind(tenant)
            .bind(caller)
            .bind(task_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;
            Ok(self.read_held(&rows).await)
        })
    }

    fn supports_renewal(&self) -> bool {
        true
    }

    fn renew<'a>(&'a self, owner: &'a str, lease: Duration) -> OutboxFuture<'a, usize> {
        Box::pin(async move {
            let renewed = sqlx::query(concat!(
                "UPDATE a2a_push_outbox SET lease_ms = ",
                now_ms!(),
                " + ?2 WHERE owner = ?1 AND lease_ms > ",
                now_ms!()
            ))
            .bind(owner)
            .bind(millis(lease))
            .execute(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?
            .rows_affected();
            Ok(usize::try_from(renewed).unwrap_or(usize::MAX))
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
                      AND o.armed = 1
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
