// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Writing rows into the PostgreSQL outbox: the bound per webhook, the insert,
//! and how a reserved row is marked.

use super::PostgresPushOutbox;
use crate::push::outbox::NewDelivery;

/// How a row being inserted was reserved, when it was.
pub(super) struct Reserve<'a> {
    pub(super) id: &'a str,
    pub(super) log_seq: i64,
    pub(super) hold_secs: f64,
}

impl PostgresPushOutbox {
    /// Makes room under `per_webhook` for one more row of `d`'s webhook: how many
    /// rows it dropped, or `None` when there is nothing to drop and `d` does not
    /// end a turn, so `d` is the row not kept.
    pub(super) async fn make_room(
        tx: &mut sqlx::PgConnection,
        d: &NewDelivery,
        per_webhook: usize,
    ) -> Result<Option<usize>, sqlx::Error> {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM a2a_push_outbox
             WHERE tenant = $1 AND caller IS NOT DISTINCT FROM $2
               AND task_id = $3 AND config_id = $4",
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
        let removed = sqlx::query(
            "DELETE FROM a2a_push_outbox WHERE seq = (
                SELECT seq FROM a2a_push_outbox
                WHERE tenant = $1 AND caller IS NOT DISTINCT FROM $2
                  AND task_id = $3 AND config_id = $4 AND NOT ends_turn
                  AND armed AND (lease_until IS NULL OR lease_until <= now())
                ORDER BY seq LIMIT 1)",
        )
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

    /// Records `deliveries` with their stored `bodies` inside `tx`.
    pub(super) async fn record_in(
        tx: &mut sqlx::PgConnection,
        deliveries: &[NewDelivery],
        bodies: &[String],
        per_webhook: usize,
        reserve: Option<&Reserve<'_>>,
    ) -> Result<usize, sqlx::Error> {
        let mut dropped = 0;
        for (d, event) in deliveries.iter().zip(bodies) {
            // Recorded already (an id is a row's identity): left as it is.
            let present: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM a2a_push_outbox WHERE delivery_id = $1")
                    .bind(&d.delivery_id)
                    .fetch_one(&mut *tx)
                    .await?;
            if present > 0 {
                continue;
            }
            let Some(made) = Self::make_room(&mut *tx, d, per_webhook).await? else {
                // Nothing to drop, and this row does not end a turn: it is the one dropped.
                dropped += 1;
                continue;
            };
            dropped += made;
            sqlx::query(
                "INSERT INTO a2a_push_outbox
                    (delivery_id, tenant, caller, task_id, config_id, event, ends_turn,
                     armed, reservation, log_seq, hold_until)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                         CASE WHEN $11::float8 IS NULL THEN NULL
                              ELSE now() + make_interval(secs => $11::float8) END)
                 ON CONFLICT (delivery_id) DO NOTHING",
            )
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
            .bind(reserve.map(|r| r.hold_secs))
            .execute(&mut *tx)
            .await?;
        }
        Ok(dropped)
    }
}
