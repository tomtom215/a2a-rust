// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Steering messages in PostgreSQL (ADR 0036): the table, the statements that
//! hand a message to the replica holding a claim, and the `LISTEN` that tells
//! that replica one arrived.
//!
//! The table is created the first time a replica uses steering, not with the
//! task store's schema: a deployment that never takes steering gets no table,
//! and an older replica, which does not know of it, is not affected.

use std::sync::Arc;

use a2a_protocol_types::error::{A2aError, A2aResult};
use sqlx::postgres::{PgListener, PgPool};
use tokio::sync::{OnceCell, broadcast};

use super::{Handed, PendingSteering};

const CREATE_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS a2a_task_steering (
    seq      BIGSERIAL PRIMARY KEY,
    token    TEXT NOT NULL,
    message  TEXT NOT NULL
)";

/// Reads and deletes go by claim.
const CREATE_INDEX_SQL: &str =
    "CREATE INDEX IF NOT EXISTS idx_a2a_task_steering_token ON a2a_task_steering(token, seq)";

/// The channel a stored message is announced on, with no payload: a signal
/// to look, which a handler that missed it makes up at its next poll.
const CHANNEL: &str = "a2a_steering";

/// How long the listener waits before reconnecting after an error.
const RECONNECT_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

fn failed(what: &str, e: &sqlx::Error) -> A2aError {
    A2aError::internal(format!("turn claims steering {what}: {e}"))
}

/// The table, created once per store: a failure is retried by the next use.
pub(super) async fn ensure(pool: &PgPool, ready: &OnceCell<()>) -> A2aResult<()> {
    ready
        .get_or_try_init(|| async {
            crate::store::pg_schema::apply(pool, &[CREATE_TABLE_SQL, CREATE_INDEX_SQL])
                .await
                .map_err(|e| failed("migrate", &e))
        })
        .await?;
    Ok(())
}

/// Stores `message` for the live claim on `task_id`, unless that claim's queue
/// holds `capacity` messages. The claim is locked `FOR KEY SHARE` until the
/// transaction ends, so a release or a takeover waits for it and no message
/// is stored for a claim that ended first; the queue is counted under an
/// advisory lock on the claim's token, so concurrent senders cannot together
/// go past `capacity`.
pub(super) async fn hand(
    pool: &PgPool,
    metrics: &dyn crate::metrics::Metrics,
    scope: &str,
    task_id: &str,
    message: &str,
    capacity: usize,
) -> A2aResult<Handed> {
    let mut tx = crate::store::pg_retry::retry(metrics, || pool.begin())
        .await
        .map_err(|e| failed("hand", &e))?;
    let claim: Option<(String,)> = sqlx::query_as(
        "SELECT token FROM a2a_task_claims
         WHERE scope = $1 AND task_id = $2 AND expires_at > now() FOR KEY SHARE",
    )
    .bind(scope)
    .bind(task_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| failed("hand", &e))?;
    let Some((token,)) = claim else {
        return Ok(Handed::NoClaim);
    };
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(&token)
        .execute(&mut *tx)
        .await
        .map_err(|e| failed("hand", &e))?;
    let capacity = i64::try_from(capacity).unwrap_or(i64::MAX);
    let stored: Option<(i64,)> = sqlx::query_as(
        "INSERT INTO a2a_task_steering (token, message)
         SELECT $1::text, $2::text
         WHERE (SELECT count(*) FROM a2a_task_steering WHERE token = $1::text) < $3::bigint
         RETURNING seq",
    )
    .bind(&token)
    .bind(message)
    .bind(capacity)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| failed("hand", &e))?;
    if stored.is_none() {
        return Ok(Handed::Full);
    }
    sqlx::query("SELECT pg_notify($1, '')")
        .bind(CHANNEL)
        .execute(&mut *tx)
        .await
        .map_err(|e| failed("hand", &e))?;
    tx.commit().await.map_err(|e| failed("hand", &e))?;
    Ok(Handed::Queued)
}

/// What is stored for the live claims among `tokens` that `owner` holds.
pub(super) async fn peek(
    pool: &PgPool,
    metrics: &dyn crate::metrics::Metrics,
    owner: &str,
    tokens: &[String],
    limit: usize,
) -> A2aResult<Vec<PendingSteering>> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let rows: Vec<(i64, String, String)> = crate::store::pg_retry::retry(metrics, || {
        sqlx::query_as(
            "SELECT s.seq, s.token, s.message
             FROM a2a_task_steering s JOIN a2a_task_claims c ON c.token = s.token
             WHERE c.owner = $1 AND c.token = ANY($2) AND c.expires_at > now()
             ORDER BY s.seq LIMIT $3",
        )
        .bind(owner)
        .bind(tokens)
        .bind(limit)
        .fetch_all(pool)
    })
    .await
    .map_err(|e| failed("peek", &e))?;
    Ok(rows
        .into_iter()
        .map(|(seq, token, message)| PendingSteering {
            seq,
            token,
            message,
        })
        .collect())
}

/// Deletes the messages `seqs` names.
pub(super) async fn ack(
    pool: &PgPool,
    metrics: &dyn crate::metrics::Metrics,
    seqs: &[i64],
) -> A2aResult<()> {
    crate::store::pg_retry::retry(metrics, || {
        sqlx::query("DELETE FROM a2a_task_steering WHERE seq = ANY($1)")
            .bind(seqs)
            .execute(pool)
    })
    .await
    .map_err(|e| failed("ack", &e))?;
    Ok(())
}

/// Deletes what is stored for `token`, a claim that ended.
pub(super) async fn forget(
    pool: &PgPool,
    metrics: &dyn crate::metrics::Metrics,
    token: &str,
) -> A2aResult<()> {
    crate::store::pg_retry::retry(metrics, || {
        sqlx::query("DELETE FROM a2a_task_steering WHERE token = $1")
            .bind(token)
            .execute(pool)
    })
    .await
    .map_err(|e| failed("release", &e))?;
    Ok(())
}

/// Deletes what is stored for claims that no longer exist: a replica that
/// died holding one, or a message that arrived as its claim ended.
pub(super) async fn sweep(pool: &PgPool, metrics: &dyn crate::metrics::Metrics) -> A2aResult<u64> {
    let done = crate::store::pg_retry::retry(metrics, || {
        sqlx::query(
            "DELETE FROM a2a_task_steering s
             WHERE NOT EXISTS (SELECT 1 FROM a2a_task_claims c WHERE c.token = s.token)",
        )
        .execute(pool)
    })
    .await
    .map_err(|e| failed("sweep", &e))?;
    Ok(done.rows_affected())
}

/// The listener's task, aborted when the last store clone is dropped.
#[derive(Debug)]
pub(super) struct Listener(tokio::task::JoinHandle<()>);

impl Drop for Listener {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A signal for each notification on the channel, from a listener started the
/// first time this is asked for; a signal is also sent after the listener
/// reconnects, since notifications sent while it was away are lost.
pub(super) fn signals(
    pool: &PgPool,
    slot: &std::sync::OnceLock<(broadcast::Sender<()>, Arc<Listener>)>,
) -> Option<broadcast::Receiver<()>> {
    tokio::runtime::Handle::try_current().ok()?;
    let (sender, _) = slot.get_or_init(|| {
        let sender = broadcast::channel(16).0;
        let task = tokio::spawn(listen(pool.clone(), sender.clone()));
        (sender, Arc::new(Listener(task)))
    });
    Some(sender.subscribe())
}

async fn listen(pool: PgPool, signals: broadcast::Sender<()>) {
    loop {
        let mut listener = match PgListener::connect_with(&pool).await {
            Ok(listener) => listener,
            Err(_e) => {
                trace_warn!(error = %_e, "steering listener: cannot connect; retrying");
                tokio::time::sleep(RECONNECT_DELAY).await;
                continue;
            }
        };
        if let Err(_e) = listener.listen(CHANNEL).await {
            trace_warn!(error = %_e, "steering listener: LISTEN failed; retrying");
            tokio::time::sleep(RECONNECT_DELAY).await;
            continue;
        }
        // Whatever was stored before this LISTEN took effect is looked at now.
        let _ = signals.send(());
        loop {
            match listener.try_recv().await {
                // A notification, or a connection lost and re-established:
                // either way, look.
                Ok(_) => {
                    let _ = signals.send(());
                }
                Err(_e) => {
                    trace_warn!(error = %_e, "steering listener: receive failed; reconnecting");
                    tokio::time::sleep(RECONNECT_DELAY).await;
                    break;
                }
            }
        }
    }
}
