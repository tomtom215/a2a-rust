// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `PostgreSQL` half of [`super`].

use sqlx::PgPool;

use super::{PurgeReport, RetentionPolicy, terminal_state_labels};

/// Deletes terminal tasks older than `policy` from `table`, in batches.
///
/// `ctid` is `PostgreSQL`'s physical row address and the standard way to bound a
/// bulk delete: the subquery picks the batch, the outer delete removes exactly
/// those rows, and nothing has to be round-tripped to the client in between.
/// It is only stable within a statement, which is all it needs to be here.
///
/// Batching matters more here than on `SQLite`. One `DELETE` covering years of
/// backlog holds row locks and keeps a transaction open for its whole run,
/// which on a busy database means bloat and blocked writers; a thousand small
/// deletes let everything else through in between.
///
/// Side tables get no sweep on this backend, so
/// [`PurgeReport::orphan_rows_deleted`] is structurally zero here — this
/// function contains no statement that could raise it, rather than running one
/// that finds nothing. `PostgreSQL` has no per-session equivalent of
/// `SQLite`'s `foreign_keys=OFF`, so a declared `ON DELETE CASCADE` always
/// fires and a sweep would have nothing to do. The one case that leaves
/// uncovered — a caller's database where the side table already existed
/// *without* the foreign key, which `CREATE TABLE IF NOT EXISTS` will not
/// correct — is recorded on [`PurgeReport::orphan_rows_deleted`] itself.
///
/// `key_sweep` deletes expired idempotency keys, taking the age as an interval
/// string and the batch size. Unlike the side tables there *is* a statement
/// for it here: an expired key is not an orphan waiting on a cascade that
/// might not have fired, it is a row whose time is up, and no foreign key
/// removes it on either backend — the key table deliberately has none, so
/// that a retention sweep cannot free a key and let its send run twice.
pub async fn purge(
    pool: &PgPool,
    table: &'static str,
    key_sweep: &'static str,
    policy: &RetentionPolicy,
) -> Result<PurgeReport, sqlx::Error> {
    let labels = terminal_state_labels();
    // `$2::interval` is evaluated against the database clock, not the
    // application's, for the same reason the SQLite side uses `strftime`.
    let interval = format!("{} seconds", policy.terminal_max_age.as_secs());
    let batch = i64::from(policy.effective_batch_size());

    let sql = format!(
        "DELETE FROM {table} WHERE ctid IN ( \
             SELECT ctid FROM {table} \
              WHERE state = ANY($1) \
                AND updated_at < now() - $2::interval \
              LIMIT $3 \
         )"
    );

    let mut report = PurgeReport::default();
    loop {
        if policy.max_batches.is_some_and(|max| report.batches >= max) {
            report.complete = false;
            break;
        }
        let deleted = sqlx::query(&sql)
            .bind(&labels)
            .bind(&interval)
            .bind(batch)
            .execute(pool)
            .await?
            .rows_affected();
        if deleted == 0 {
            report.complete = true;
            break;
        }
        report.tasks_deleted += deleted;
        report.batches += 1;
    }

    if let Some(max_age) = policy.effective_idempotency_key_max_age() {
        let key_interval = format!("{} seconds", max_age.as_secs());
        loop {
            if policy.max_batches.is_some_and(|max| report.batches >= max) {
                report.complete = false;
                return Ok(report);
            }
            let deleted = sqlx::query(key_sweep)
                .bind(&key_interval)
                .bind(batch)
                .execute(pool)
                .await?
                .rows_affected();
            if deleted == 0 {
                break;
            }
            report.idempotency_keys_deleted += deleted;
            report.batches += 1;
        }
    }
    Ok(report)
}
