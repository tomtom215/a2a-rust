// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`PushOutbox`] in PostgreSQL (feature `postgres`), shared by every replica
//! that opens it.
//!
//! The clock is the database's (`now()`), so replicas whose clocks disagree
//! agree on what is due and whose lease has run out (audit E-F16, E-F17). A
//! claim is one `UPDATE` over a `FOR UPDATE SKIP LOCKED` selection: two
//! replicas claiming at once take different rows, and neither waits for the
//! other. A webhook's later row is not claimable while an earlier one exists,
//! locked or not, so its events go out in order from whichever replica.
//!
//! Every statement runs under `store::pg_retry`: a claim and a record are
//! `checked` (a second run could claim or record twice), the rest `retry`.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::error::A2aError;
use sqlx::Row as _;
use sqlx::postgres::PgPool;

use super::body::{Body, context};
use super::{ClaimedDelivery, Held, NewDelivery, OutboxFuture, PushOutbox};
use crate::metrics::{Metrics, NoopMetrics};
use crate::push::PushConfigCipher;
use crate::store::pg_retry;

mod record;
use record::Reserve;

const SCHEMA: [&str; 9] = [
    "CREATE TABLE IF NOT EXISTS a2a_push_outbox (
        seq         BIGSERIAL   PRIMARY KEY,
        delivery_id TEXT        NOT NULL UNIQUE,
        tenant      TEXT        NOT NULL,
        caller      TEXT,
        task_id     TEXT        NOT NULL,
        config_id   TEXT        NOT NULL,
        event       TEXT        NOT NULL,
        ends_turn   BOOLEAN     NOT NULL,
        attempts    INTEGER     NOT NULL DEFAULT 0,
        recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
        due_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
        owner       TEXT,
        lease_until TIMESTAMPTZ,
        armed       BOOLEAN     NOT NULL DEFAULT TRUE,
        reservation TEXT,
        log_seq     BIGINT,
        hold_until  TIMESTAMPTZ
    )",
    // A table an earlier build created has none of the two-phase columns
    // (audit H14); rows it holds are armed.
    "ALTER TABLE a2a_push_outbox ADD COLUMN IF NOT EXISTS armed BOOLEAN NOT NULL DEFAULT TRUE",
    "ALTER TABLE a2a_push_outbox ADD COLUMN IF NOT EXISTS reservation TEXT",
    "ALTER TABLE a2a_push_outbox ADD COLUMN IF NOT EXISTS log_seq BIGINT",
    "ALTER TABLE a2a_push_outbox ADD COLUMN IF NOT EXISTS hold_until TIMESTAMPTZ",
    "CREATE INDEX IF NOT EXISTS a2a_push_outbox_reservation
        ON a2a_push_outbox (reservation) WHERE reservation IS NOT NULL",
    // What `held` looks for, without reading a backlog of armed rows.
    "CREATE INDEX IF NOT EXISTS a2a_push_outbox_unarmed
        ON a2a_push_outbox (hold_until) WHERE NOT armed",
    "CREATE INDEX IF NOT EXISTS a2a_push_outbox_webhook
        ON a2a_push_outbox (tenant, caller, task_id, config_id, seq)",
    "CREATE INDEX IF NOT EXISTS a2a_push_outbox_due ON a2a_push_outbox (due_at)",
];

/// A [`PushOutbox`] in a PostgreSQL database; see the module documentation.
#[derive(Clone)]
pub struct PostgresPushOutbox {
    pub(super) pool: PgPool,
    body: Body,
    metrics: Arc<dyn Metrics>,
}

impl std::fmt::Debug for PostgresPushOutbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresPushOutbox")
            .field("body", &self.body)
            .finish_non_exhaustive()
    }
}

fn to_a2a_error(e: &sqlx::Error) -> A2aError {
    crate::sql_error::sql_error(crate::sql_error::Db::Postgres, "postgres push outbox", e)
}

const fn seconds(d: Duration) -> f64 {
    d.as_secs_f64()
}

impl PostgresPushOutbox {
    /// Opens a pool of [`DEFAULT_POOL_SIZE`](crate::store::DEFAULT_POOL_SIZE)
    /// connections to `url`, with the task stores' options
    /// (`store::pg_retry`), and creates the table under the crate's schema
    /// lock.
    ///
    /// # Errors
    ///
    /// The database cannot be reached, or the table created.
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        // sqlx's default pool size, 10: `DEFAULT_POOL_SIZE`, as every store's.
        let pool = pg_retry::pool_options().connect(url).await?;
        Self::from_pool(pool).await
    }

    /// The outbox on `pool`, creating its table. The pool should have the
    /// task stores' options (`pg_retry::pool_options`), as
    /// [`new`](Self::new)'s does, or a ping before every statement.
    ///
    /// # Errors
    ///
    /// The table cannot be created.
    pub async fn from_pool(pool: PgPool) -> Result<Self, sqlx::Error> {
        crate::store::pg_schema::apply(&pool, &SCHEMA).await?;
        Ok(Self {
            pool,
            body: Body::default(),
            metrics: Arc::new(NoopMetrics),
        })
    }

    /// Seals every event body with `cipher`; see
    /// [`SqlitePushOutbox::with_cipher`](super::SqlitePushOutbox).
    #[must_use]
    pub fn with_cipher(mut self, cipher: Arc<dyn PushConfigCipher>) -> Self {
        self.body = Body::with_cipher(cipher);
        self
    }

    /// Reports a statement run again after a lost connection to `metrics`
    /// (`Metrics::on_store_retry`).
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn Metrics>) -> Self {
        self.metrics = metrics;
        self
    }

    /// The reservations `rows` name, their events opened; one that does not open
    /// is left in place and said so.
    async fn read_held(&self, rows: &[sqlx::postgres::PgRow]) -> Vec<Held> {
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
    async fn read(&self, row: &sqlx::postgres::PgRow) -> Option<ClaimedDelivery> {
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
            attempts: u32::try_from(row.get::<i32, _>("attempts")).unwrap_or(0),
            age: Duration::try_from_secs_f64(row.get::<f64, _>("age_secs")).unwrap_or_default(),
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

impl PostgresPushOutbox {
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
        pg_retry::checked(async {
            let mut tx = self.pool.begin().await?;
            let dropped =
                Self::record_in(&mut tx, &deliveries, &bodies, per_webhook, reserve.as_ref())
                    .await?;
            tx.commit().await?;
            Ok::<_, sqlx::Error>(dropped)
        })
        .await
        .map_err(|e| to_a2a_error(&e))
    }
}

impl PushOutbox for PostgresPushOutbox {
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
                hold_secs: seconds(hold),
            }),
        ))
    }

    fn arm<'a>(&'a self, reservation: &'a str) -> OutboxFuture<'a, usize> {
        Box::pin(async move {
            // Idempotent: a second run matches the same rows.
            let armed = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query("UPDATE a2a_push_outbox SET armed = TRUE WHERE reservation = $1")
                    .bind(reservation)
                    .execute(&self.pool)
            })
            .await
            .map_err(|e| to_a2a_error(&e))?
            .rows_affected();
            Ok(usize::try_from(armed).unwrap_or(usize::MAX))
        })
    }

    fn release<'a>(&'a self, reservation: &'a str) -> OutboxFuture<'a, ()> {
        Box::pin(async move {
            pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query("DELETE FROM a2a_push_outbox WHERE reservation = $1 AND NOT armed")
                    .bind(reservation)
                    .execute(&self.pool)
            })
            .await
            .map_err(|e| to_a2a_error(&e))?;
            Ok(())
        })
    }

    fn held(&self, limit: usize) -> OutboxFuture<'_, Vec<Held>> {
        Box::pin(async move {
            let rows = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query(
                    "SELECT delivery_id, tenant, caller, task_id, event, log_seq, reservation
                     FROM a2a_push_outbox
                     WHERE seq IN (
                        SELECT MIN(seq) FROM a2a_push_outbox
                        WHERE NOT armed AND hold_until <= now()
                        GROUP BY reservation ORDER BY MIN(seq) LIMIT $1)
                     ORDER BY seq",
                )
                .bind(i64::try_from(limit).unwrap_or(i64::MAX))
                .fetch_all(&self.pool)
            })
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
            let rows = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query(
                    "SELECT delivery_id, tenant, caller, task_id, event, log_seq, reservation
                     FROM a2a_push_outbox
                     WHERE seq IN (
                        SELECT MIN(seq) FROM a2a_push_outbox
                        WHERE NOT armed AND tenant = $1
                          AND caller IS NOT DISTINCT FROM $2 AND task_id = $3
                        GROUP BY reservation)
                     ORDER BY seq",
                )
                .bind(tenant)
                .bind(caller)
                .bind(task_id)
                .fetch_all(&self.pool)
            })
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
            // Idempotent: a second run sets the same end.
            let renewed = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query(
                    "UPDATE a2a_push_outbox
                     SET lease_until = now() + make_interval(secs => $2)
                     WHERE owner = $1 AND lease_until > now()",
                )
                .bind(owner)
                .bind(seconds(lease))
                .execute(&self.pool)
            })
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
            let rows = pg_retry::checked(
                sqlx::query(
                    "UPDATE a2a_push_outbox
                     SET owner = $1, lease_until = now() + make_interval(secs => $2)
                     WHERE seq IN (
                        SELECT seq FROM a2a_push_outbox o
                        WHERE o.due_at <= now() AND o.armed
                          AND (o.lease_until IS NULL OR o.lease_until <= now())
                          AND NOT EXISTS (
                            SELECT 1 FROM a2a_push_outbox e
                            WHERE e.tenant = o.tenant
                              AND e.caller IS NOT DISTINCT FROM o.caller
                              AND e.task_id = o.task_id AND e.config_id = o.config_id
                              AND e.seq < o.seq)
                        ORDER BY o.seq LIMIT $3
                        FOR UPDATE SKIP LOCKED)
                     RETURNING seq, delivery_id, tenant, caller, task_id, config_id, event,
                               ends_turn, attempts,
                               EXTRACT(EPOCH FROM now() - recorded_at)::float8 AS age_secs",
                )
                .bind(owner)
                .bind(seconds(lease))
                .bind(i64::try_from(max).unwrap_or(i64::MAX))
                .fetch_all(&self.pool),
            )
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
            pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query("DELETE FROM a2a_push_outbox WHERE seq = $1")
                    .bind(seq)
                    .execute(&self.pool)
            })
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
            // A second run finds the row released and changes nothing.
            pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query(
                    "UPDATE a2a_push_outbox
                     SET attempts = attempts + 1,
                         due_at = now() + make_interval(secs => $3),
                         owner = NULL, lease_until = NULL
                     WHERE seq = $1 AND owner = $2",
                )
                .bind(seq)
                .bind(owner)
                .bind(seconds(after))
                .execute(&self.pool)
            })
            .await
            .map_err(|e| to_a2a_error(&e))?;
            Ok(())
        })
    }

    fn pending(&self) -> OutboxFuture<'_, usize> {
        Box::pin(async move {
            let n: i64 = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query_scalar("SELECT COUNT(*) FROM a2a_push_outbox").fetch_one(&self.pool)
            })
            .await
            .map_err(|e| to_a2a_error(&e))?;
            Ok(usize::try_from(n).unwrap_or(usize::MAX))
        })
    }
}
