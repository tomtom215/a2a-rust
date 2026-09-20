// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `SQLite` half of [`super`].

use sqlx::SqlitePool;

use super::{PurgeReport, RetentionPolicy, terminal_state_labels};

/// Deletes terminal tasks older than `policy` from `table`, in batches.
///
/// `orphan_sweeps` are anti-join `DELETE`s, one per side table hanging off
/// `table`, run once after the task rows are gone. Each is written beside the
/// schema it cleans rather than assembled here, because the tenant-aware
/// tables are keyed on two columns and the others on one.
///
/// The batch is chosen *inside* the `DELETE`, by a `rowid` subquery, rather
/// than selected first and then bound back as a list of ids. That keeps the
/// number of bound parameters at six however large a batch is — an id list
/// would run into `SQLite`'s variable limit (999 on builds before 3.32) at
/// exactly the batch sizes an operator would reach for on a first sweep
/// through years of backlog.
///
/// `rowid` and not `id`: `tenant_tasks` is keyed on `(tenant_id, id)`, so a
/// delete matching `id` alone would take that task id from every tenant.
pub async fn purge(
    pool: &SqlitePool,
    table: &'static str,
    orphan_sweeps: &[&'static str],
    policy: &RetentionPolicy,
) -> Result<PurgeReport, sqlx::Error> {
    let labels = terminal_state_labels();
    // Evaluated by SQLite against its own clock rather than formatted here
    // from the process clock: a host running fast would otherwise delete work
    // that is younger than the policy allows.
    let cutoff = format!("-{} seconds", policy.terminal_max_age.as_secs());
    let batch = i64::from(policy.effective_batch_size());

    let delete_sql = format!(
        "DELETE FROM {table} WHERE rowid IN ( \
             SELECT rowid FROM {table} \
              WHERE state IN (?1, ?2, ?3, ?4) \
                AND updated_at < strftime('%Y-%m-%d %H:%M:%f', 'now', ?5) \
              LIMIT ?6 \
         )"
    );

    let mut report = PurgeReport::default();
    loop {
        if policy.max_batches.is_some_and(|max| report.batches >= max) {
            report.complete = false;
            break;
        }
        let mut query = sqlx::query(&delete_sql);
        for label in &labels {
            query = query.bind(label);
        }
        let deleted = query
            .bind(&cutoff)
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

    // Side-table rows are cleared by anti-join once, after the task rows are
    // gone, rather than per batch. `journal.rs` explains why this cannot be
    // left to `ON DELETE CASCADE`: `from_pool` takes a caller's pool and
    // cannot assume `foreign_keys=ON`, and a cascade that silently does not
    // fire leaves parts behind that would be spliced onto — or, for the event
    // log, replayed to — the next task to reuse the id.
    //
    // A row can only be an orphan because its task was deleted — the foreign
    // key means it could not have been written before the task existed — so
    // there is no race with a concurrent writer here.
    if report.tasks_deleted > 0 {
        for sweep in orphan_sweeps {
            report.orphan_rows_deleted += sqlx::query(sweep).execute(pool).await?.rows_affected();
        }
    }

    Ok(report)
}
