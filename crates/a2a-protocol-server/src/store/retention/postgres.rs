// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `PostgreSQL` half of [`super`].

use sqlx::PgPool;

use super::{PurgeReport, RetentionPolicy, parked_state_labels, terminal_state_labels};

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
/// `claim_scope` is the SQL expression, over the alias `t`, for the scope a
/// task's turn claim is filed under: `''` for `tasks`, `t.tenant_id` for
/// `tenant_tasks`. Only the parked sweep reads it (see [`purge_parked`]).
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
    claim_scope: &'static str,
    key_sweep: &'static str,
    policy: &RetentionPolicy,
) -> Result<PurgeReport, sqlx::Error> {
    let labels = terminal_state_labels();
    // `$2::interval` is evaluated against the database clock, not the
    // application's, for the same reason the SQLite side uses `strftime`.
    let interval = format!("{} seconds", policy.terminal_max_age.as_secs());
    let batch = i64::from(policy.effective_batch_size());

    // `FOR UPDATE SKIP LOCKED`: two sweeps that start together would otherwise
    // pick the same rows from the same snapshot. The second `DELETE` then waits
    // for the first to commit, finds its rows gone, deletes nothing, and reads
    // as "no more work" while eligible rows remain. Skipping a row another
    // sweep has locked makes it take the next ones instead; a sweep that finds
    // only locked rows deletes nothing and stops, which is right, because the
    // sweep holding them is deleting them.
    let sql = format!(
        "DELETE FROM {table} WHERE ctid IN ( \
             SELECT ctid FROM {table} \
              WHERE state = ANY($1) \
                AND updated_at < now() - $2::interval \
              LIMIT $3 \
                FOR UPDATE SKIP LOCKED \
         )"
    );

    let mut report = PurgeReport::default();
    loop {
        if policy.max_batches.is_some_and(|max| report.batches >= max) {
            report.complete = false;
            break;
        }
        // `table` is `&'static str`; the cut-off and batch are binds.
        let deleted = crate::store::pg_retry::retry(&crate::metrics::NoopMetrics, || {
            sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(&labels)
                .bind(&interval)
                .bind(batch)
                .execute(pool)
        })
        .await?
        .rows_affected();
        if deleted == 0 {
            report.complete = true;
            break;
        }
        report.tasks_deleted += deleted;
        report.batches += 1;
    }

    // Parked tasks, only when the policy opts in, after the terminal ones for
    // the reason the keys come last: a sweep short of batches spends them on
    // what an operator asked for first.
    if !purge_parked(pool, table, claim_scope, policy, &mut report).await? {
        return Ok(report);
    }

    expire_keys(pool, key_sweep, policy, batch, &mut report).await?;
    Ok(report)
}

/// Deletes expired idempotency keys, in batches, updating `report`. A no-op
/// when the policy keeps keys for ever.
async fn expire_keys(
    pool: &PgPool,
    key_sweep: &'static str,
    policy: &RetentionPolicy,
    batch: i64,
    report: &mut PurgeReport,
) -> Result<(), sqlx::Error> {
    let Some(max_age) = policy.effective_idempotency_key_max_age() else {
        return Ok(());
    };
    let key_interval = format!("{} seconds", max_age.as_secs());
    loop {
        if policy.max_batches.is_some_and(|max| report.batches >= max) {
            report.complete = false;
            return Ok(());
        }
        let deleted = crate::store::pg_retry::retry(&crate::metrics::NoopMetrics, || {
            sqlx::query(key_sweep)
                .bind(&key_interval)
                .bind(batch)
                .execute(pool)
        })
        .await?
        .rows_affected();
        if deleted == 0 {
            return Ok(());
        }
        report.idempotency_keys_deleted += deleted;
        report.batches += 1;
    }
}

/// Deletes parked tasks (`input-required`, `auth-required`) not updated for
/// [`RetentionPolicy::parked_max_age`] from `table`, in batches, counting them
/// in [`PurgeReport::parked_tasks_deleted`]; nothing when the policy has no
/// such age. `false` when [`RetentionPolicy::max_batches`] stopped it with
/// work left (and the report says the sweep is not complete).
///
/// The statement is the terminal sweep's, with one more condition: **no live
/// turn claim on the task**. A terminal task is never written again, so the
/// sweep cannot race a writer; a parked task is continued by the next message,
/// and the continuation takes the task's claim before it writes. A task whose
/// claim has not expired is being continued now and is skipped, however old
/// its last update. The claim is read in the same statement as the delete,
/// and the row is locked (`FOR UPDATE OF t SKIP LOCKED`, as above) before the
/// delete, so a continuation that has written meanwhile holds the lock and the
/// sweep passes over it.
///
/// What is not closed is the instant between a continuation reading the task
/// and taking its claim: a sweep landing exactly there deletes the task, and
/// the continuation's write then creates it again from what it read, without
/// the event log that went with it. A parked task is at least `max_age` old,
/// which an operator sets in hours or days.
async fn purge_parked(
    pool: &PgPool,
    table: &'static str,
    claim_scope: &'static str,
    policy: &RetentionPolicy,
    report: &mut PurgeReport,
) -> Result<bool, sqlx::Error> {
    let Some(max_age) = policy.parked_max_age else {
        return Ok(true);
    };
    let labels = parked_state_labels();
    let interval = format!("{} seconds", max_age.as_secs());
    let batch = i64::from(policy.effective_batch_size());
    let sql = parked_sql(table, claim_scope);
    loop {
        if policy.max_batches.is_some_and(|max| report.batches >= max) {
            report.complete = false;
            return Ok(false);
        }
        let deleted = crate::store::pg_retry::retry(&crate::metrics::NoopMetrics, || {
            sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(&labels)
                .bind(&interval)
                .bind(batch)
                .execute(pool)
        })
        .await?
        .rows_affected();
        if deleted == 0 {
            return Ok(true);
        }
        report.parked_tasks_deleted += deleted;
        report.batches += 1;
    }
}

/// The parked sweep's statement: `$1` the state labels, `$2` the age as an
/// interval string, `$3` the batch size. A function of its two names so that a
/// test can run the very text the sweep runs, inside a transaction of its own,
/// where `now()` is pinned and an age can be made exactly the policy's.
pub(super) fn parked_sql(table: &str, claim_scope: &str) -> String {
    format!(
        "DELETE FROM {table} WHERE ctid IN ( \
             SELECT t.ctid FROM {table} t \
              WHERE t.state = ANY($1) \
                AND t.updated_at < now() - $2::interval \
                AND NOT EXISTS (SELECT 1 FROM a2a_task_claims c \
                                 WHERE c.scope = {claim_scope} AND c.task_id = t.id \
                                   AND c.expires_at > now()) \
              LIMIT $3 \
                FOR UPDATE OF t SKIP LOCKED \
         )"
    )
}
