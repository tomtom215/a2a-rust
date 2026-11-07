// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Where a task's event log begins, and trimming it below a position (ADR
//! 0047): the bodies of `earliest_event_seq` and `trim_event_log`, moved out
//! of `store_impl.rs`, which the trim took past 500 lines.

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::task::TaskId;

use super::{TenantAwarePostgresTaskStore, to_a2a_error};
use crate::store::event_log_sql::{seq_from_i64, seq_to_i64};
use crate::store::tenant::TenantContext;
use crate::store::tenant_event_log as evlog;

impl TenantAwarePostgresTaskStore {
    /// The lowest position the current partition's log of `task_id` holds.
    pub(super) async fn earliest_seq(&self, task_id: &TaskId) -> A2aResult<Option<u64>> {
        let tenant = TenantContext::partition();
        let (min,): (Option<i64>,) = crate::store::pg_retry::retry(&*self.metrics, || {
            sqlx::query_as(evlog::PG_EARLIEST_SEQ)
                .bind(&tenant)
                .bind(task_id.0.as_str())
                .fetch_one(&self.pool)
        })
        .await
        .map_err(|e| to_a2a_error(&e))?;
        min.map(seq_from_i64).transpose()
    }

    /// Drops the current partition's events of `task_id` below `below`.
    pub(super) async fn trim_log(&self, task_id: &TaskId, below: u64) -> A2aResult<u64> {
        let tenant = TenantContext::partition();
        let below = seq_to_i64(below)?;
        let done = crate::store::pg_retry::retry(&*self.metrics, || {
            sqlx::query(evlog::PG_TRIM_BELOW)
                .bind(&tenant)
                .bind(task_id.0.as_str())
                .bind(below)
                .execute(&self.pool)
        })
        .await
        .map_err(|e| to_a2a_error(&e))?;
        Ok(done.rows_affected())
    }
}
