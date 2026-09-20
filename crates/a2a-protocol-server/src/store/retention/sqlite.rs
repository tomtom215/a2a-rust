// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `SQLite` half of [`super`].

use sqlx::SqlitePool;

use super::{PurgeReport, RetentionPolicy, terminal_state_labels};

/// Deletes terminal tasks older than `policy` from `table`, in batches.
///
/// `orphan_sweeps` are anti-join `DELETE`s, one per side table hanging off
/// `table`, run after the task rows are gone. Each is written beside the
/// schema it cleans rather than assembled here, because the tenant-aware
/// tables are keyed on two columns and the others on one, and each takes the
/// batch size as its single bound parameter.
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
///
/// `key_sweep` is the statement that deletes expired idempotency keys, taking
/// the cutoff modifier and the batch size. It is separate from
/// `orphan_sweeps` because it is not an anti-join and not a repair: an orphan
/// row is something that should not exist, while an expired key is one whose
/// time is simply up, and the report counts them apart for that reason.
pub async fn purge(
    pool: &SqlitePool,
    table: &'static str,
    orphan_sweeps: &[&'static str],
    key_sweep: &'static str,
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

    // Side-table rows are cleared by anti-join after the task rows are gone,
    // rather than per batch. `journal.rs` explains why this cannot be left to
    // `ON DELETE CASCADE`: `from_pool` takes a caller's pool and cannot assume
    // `foreign_keys=ON`, and a cascade that silently does not fire leaves
    // parts behind that would be spliced onto — or, for the event log,
    // replayed to — the next task to reuse the id.
    //
    // This runs on every sweep, and until 0.13 it ran only `if
    // report.tasks_deleted > 0`. That guard assumed orphans can only appear
    // during the purge that made them, which is the one thing that is not true
    // in the configuration the sweep exists for: a purge that failed part way
    // has already committed its earlier batches, so the rows it stranded
    // outlive it, and the next sweep — finding nothing newly eligible —
    // skipped the cleanup and left them to be replayed to whoever next reused
    // the task id.
    //
    // The comment that stood here argued there was no race with a concurrent
    // writer, "because the foreign key means a row could not have been written
    // before its task existed". That reasoning holds only where the foreign
    // key is enforced, and where it is enforced the cascade already removed
    // the rows and this sweep has nothing to do. It was circular: it assumed
    // away the only configuration in which it runs.
    //
    // What is actually true is narrower, and enough. The anti-join deletes a
    // row only when no task with its id exists *at the moment the statement
    // runs*. A writer inserting a side-table row for a task it has already
    // created cannot lose that row, because the task is there to be found.
    // What is genuinely racy is a writer that inserts a side-table row for a
    // task it has not created yet — with `foreign_keys=OFF` nothing stops it —
    // and that row is indistinguishable from an orphan and may be swept. No
    // path in this crate writes in that order: `save` creates the task row,
    // and the journal and the event log are written against a task that
    // already exists.
    for sweep in orphan_sweeps {
        loop {
            if policy.max_batches.is_some_and(|max| report.batches >= max) {
                report.complete = false;
                return Ok(report);
            }
            // Batched for the reason the task delete above is batched: one
            // `DELETE` covering a large backlog holds the write lock for as
            // long as it runs, and a function this careful about that for task
            // rows should not undo it on the side tables. Each statement's own
            // doc comment says what its batch counts — rows are not always the
            // unit, because two of these tables are `WITHOUT ROWID`.
            let deleted = sqlx::query(sweep)
                .bind(batch)
                .execute(pool)
                .await?
                .rows_affected();
            if deleted == 0 {
                break;
            }
            report.orphan_rows_deleted += deleted;
            report.batches += 1;
        }
    }

    // Expired idempotency keys, last. Ordering is not load-bearing — the key
    // table has no foreign key to `tasks`, deliberately, so nothing here
    // depends on the task rows being gone first — but a sweep that ran out of
    // batches should spend them on task rows, which are the larger table and
    // the reason an operator called this at all.
    expire_keys(pool, key_sweep, policy, batch, &mut report).await?;

    Ok(report)
}

/// Deletes expired idempotency keys, in batches, updating `report`.
///
/// Separate from [`purge`] to keep that function inside the 60-line bound
/// `clippy::too_many_lines` sets, and the seam is a real one: this is the only
/// pass whose subject is not a task or a row hanging off one.
///
/// A no-op when the policy keeps keys for ever, which is what every release
/// before this one did.
async fn expire_keys(
    pool: &SqlitePool,
    key_sweep: &'static str,
    policy: &RetentionPolicy,
    batch: i64,
    report: &mut PurgeReport,
) -> Result<(), sqlx::Error> {
    let Some(max_age) = policy.effective_idempotency_key_max_age() else {
        return Ok(());
    };
    // Evaluated by `SQLite` against its own clock, as the task cutoff is: a
    // host running fast would delete keys younger than the policy allows, and
    // a key deleted early is a send that can execute twice.
    let cutoff = format!("-{} seconds", max_age.as_secs());
    loop {
        if policy.max_batches.is_some_and(|max| report.batches >= max) {
            report.complete = false;
            return Ok(());
        }
        let deleted = sqlx::query(key_sweep)
            .bind(&cutoff)
            .bind(batch)
            .execute(pool)
            .await?
            .rows_affected();
        if deleted == 0 {
            return Ok(());
        }
        report.idempotency_keys_deleted += deleted;
        report.batches += 1;
    }
}
