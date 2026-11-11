// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Writing rows into the SQLite outbox: the bound per webhook, the insert, and
//! how a reserved row is marked.

use super::SqlitePushOutbox;
use crate::push::outbox::NewDelivery;

/// How a row being inserted was reserved, when it was.
pub(super) struct Reserve<'a> {
    pub(super) id: &'a str,
    pub(super) log_seq: i64,
    pub(super) hold_ms: i64,
}

impl SqlitePushOutbox {
    /// Makes room under `per_webhook` for one more row of `d`'s webhook: how
    /// many rows it dropped, or `None` when there is nothing to drop and `d`
    /// does not end a turn, so `d` is the row not kept.
    pub(super) async fn make_room(
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
    pub(super) async fn record_one(
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
}
