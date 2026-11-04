// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`PostgresRateLimitCounter`]: the shared counter in a `PostgreSQL` table.

use super::{A2aError, A2aResult, Future, Pin, RateLimitCounter};
use std::sync::atomic::{AtomicU64, Ordering};

/// The table the counter lives in.
///
/// `window_no` is part of the primary key rather than a column to
/// overwrite, so a request arriving as the window turns lands in the new
/// window's row instead of racing an update against the old one.
const CREATE_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS a2a_rate_limit (
        caller       TEXT   NOT NULL,
        window_no    BIGINT NOT NULL,
        request_count BIGINT NOT NULL,
        PRIMARY KEY (caller, window_no)
    )";

/// Count one request and return the new total, in one statement.
///
/// `INSERT .. ON CONFLICT DO UPDATE .. RETURNING` is what makes this
/// atomic: `PostgreSQL` takes the row lock for the upsert and returns the
/// post-increment value, so two replicas hitting the same row get 1 and 2
/// rather than 1 and 1. A `SELECT` followed by an `UPDATE` would lose
/// increments under precisely the concurrency this exists for.
///
/// The window is `floor(epoch / window_secs)` on the **database's** clock,
/// not the number the interceptor computed from the replica's own:
/// replicas whose clocks differ by a second would otherwise count in
/// different rows for that second at every boundary, and each would admit
/// a full limit.
///
/// `$2` is the window width in seconds.
const COUNT_SQL: &str = "INSERT INTO a2a_rate_limit (caller, window_no, request_count) \
     VALUES ($1, floor(extract(epoch from now()) / $2::bigint)::bigint, 1) \
     ON CONFLICT (caller, window_no) \
     DO UPDATE SET request_count = a2a_rate_limit.request_count + 1 \
     RETURNING request_count";

/// Drop rows for windows before the previous one, on the database's clock.
///
/// The previous window is kept: a count that was already past `now()` when
/// the window turned lands in it, and a clock that steps back (a failover
/// onto a host with a different clock) puts `now()` one window behind
/// rows already written. `$1` is the window width in seconds.
const SWEEP_SQL: &str = "DELETE FROM a2a_rate_limit \
     WHERE window_no < floor(extract(epoch from now()) / $1::bigint)::bigint - 1";

/// How many counts between sweeps of expired windows.
///
/// The same amortisation the in-process limiter uses for its bucket map,
/// for the same reason: the table is bounded by callers-per-window, and
/// without a sweep it would instead be bounded by callers-per-window times
/// the age of the deployment.
const SWEEP_INTERVAL: u64 = 1_000;

/// A [`RateLimitCounter`] backed by a `PostgreSQL` table.
///
/// Suits a deployment already sharing a `PostgreSQL` task store: no second
/// piece of infrastructure, and the counter is as available as the store
/// the agent already depends on.
///
/// Windows are numbered by the database's clock, not the replica's, so
/// replicas whose clocks differ still count in the same row and one
/// replica's sweep cannot delete a window another is still counting in.
///
/// It is not the fastest possible shared counter — an in-memory keyspace
/// like Redis will beat a durable table — and that is the trade being
/// made. A deployment that needs the last microsecond implements
/// [`RateLimitCounter`] against Redis instead; the trait exists so that is
/// a few lines rather than a fork.
pub struct PostgresRateLimitCounter {
    pool: sqlx::PgPool,
    counted: AtomicU64,
}

impl std::fmt::Debug for PostgresRateLimitCounter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresRateLimitCounter")
            .finish_non_exhaustive()
    }
}

impl PostgresRateLimitCounter {
    /// Connects to `url` and creates the counter table if it is absent.
    ///
    /// # Durability, deliberately traded away
    ///
    /// Sessions on this pool run with `synchronous_commit = off`, so an
    /// increment is not waiting on a WAL fsync. Measured on loopback, that
    /// is the difference between **598us and 232us per request** — the
    /// fsync was almost two thirds of the cost.
    ///
    /// It is the right trade for this data specifically, and would not be
    /// for most: a rate-limit count describes one window, is superseded
    /// when the window rolls, and is swept away shortly after. The worst a
    /// crash can do is forget that a caller had used part of its budget in
    /// the last moments before it — which lets a few extra requests
    /// through, once, on a server that has just restarted.
    ///
    /// The setting is scoped to the pool this constructor owns.
    /// [`from_pool`](Self::from_pool) deliberately does *not* apply it: the
    /// pool handed in there is usually the task store's, and quietly making
    /// a task store non-durable to speed up a counter would be an
    /// appalling thing to do behind a caller's back. A caller who wants
    /// both can say so on their own pool.
    ///
    /// # Errors
    ///
    /// [`A2aError::internal`] if the database cannot be reached or the
    /// table cannot be created.
    pub async fn new(url: &str) -> A2aResult<Self> {
        let pool =
            crate::store::pg_retry::recycling_after_failover(sqlx::postgres::PgPoolOptions::new())
                .after_connect(|conn, _meta| {
                    Box::pin(async move {
                        sqlx::query("SET synchronous_commit = off")
                            .execute(&mut *conn)
                            .await
                            .map(|_| ())
                    })
                })
                .connect(url)
                .await
                .map_err(|e| A2aError::internal(format!("rate-limit counter connect: {e}")))?;
        Self::from_pool(pool).await
    }

    /// Uses an existing pool — the usual choice when the deployment already
    /// has one for its task store, so the limiter adds no connections.
    ///
    /// Unlike [`new`](Self::new), this leaves the pool's settings exactly
    /// as the caller configured them, including durability. That costs
    /// roughly 2.8x per request against a pool with
    /// `synchronous_commit = off`, and it is not this constructor's call to
    /// make on a pool it does not own.
    ///
    /// # Errors
    ///
    /// [`A2aError::internal`] if the table cannot be created.
    pub async fn from_pool(pool: sqlx::PgPool) -> A2aResult<Self> {
        // Under the crate's schema lock, so replicas starting against an
        // empty database do not race. See `store::pg_schema`.
        crate::store::pg_schema::apply(&pool, &[CREATE_TABLE_SQL])
            .await
            .map_err(|e| A2aError::internal(format!("rate-limit counter migrate: {e}")))?;
        Ok(Self {
            pool,
            counted: AtomicU64::new(0),
        })
    }

    /// Deletes rows for windows before the previous one, as the database
    /// numbers them for windows `window_secs` wide.
    ///
    /// Failure is traced and swallowed: a sweep that did not happen costs
    /// disk, while an error propagated from here would reject a request
    /// that was correctly counted.
    async fn sweep(&self, window_secs: i64) {
        if let Err(_e) = sqlx::query(SWEEP_SQL)
            .bind(window_secs)
            .execute(&self.pool)
            .await
        {
            trace_warn!(error = %_e, "rate-limit counter sweep failed");
        }
    }
}

impl RateLimitCounter for PostgresRateLimitCounter {
    fn count<'a>(
        &'a self,
        key: &'a str,
        _window: u64,
        window_secs: u64,
    ) -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + 'a>> {
        Box::pin(async move {
            let window_secs = i64::try_from(window_secs)
                .map_err(|_| A2aError::internal("rate-limit window out of range"))?;

            let (count,): (i64,) = sqlx::query_as(COUNT_SQL)
                .bind(key)
                .bind(window_secs)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| {
                    crate::store::pg_retry::note_read_only(&e);
                    A2aError::internal(format!("rate-limit counter: {e}"))
                })?;

            let n = self.counted.fetch_add(1, Ordering::Relaxed);
            if n > 0 && n.is_multiple_of(SWEEP_INTERVAL) {
                self.sweep(window_secs).await;
            }

            Ok(u64::try_from(count).unwrap_or(u64::MAX))
        })
    }
}

#[cfg(test)]
mod tests;
