// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Surviving a restart, and stopping cleanly.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::builder::RequestHandlerBuilder;

use super::{bind, plain_card, serve, Check};
use crate::agents::LogSearchExecutor;
use crate::{send_params, user_message};

// ── SQLite-backed persistence ────────────────────────────────────────────────

const SQLITE_LABEL: &str = "SQLite persistence (survives a handler restart)";

/// A task written through one handler must be readable through a *different*
/// handler over the same database file.
///
/// The in-memory store passes any check that reuses one handler, so the second
/// handler is the whole point: it is what a restart does. The database lives in
/// a per-run temporary directory that is removed afterwards, so a later run
/// cannot pass on a row left behind by an earlier one.
#[cfg(feature = "sqlite")]
pub(super) async fn sqlite_persistence() -> Check {
    let dir = std::env::temp_dir().join(format!("a2a-incident-harden-{}", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Check::fail(SQLITE_LABEL, format!("creating {}: {e}", dir.display()));
    }
    let db_url = format!("sqlite://{}/tasks.db?mode=rwc", dir.display());

    // Cleanup happens on every path, including the failing ones, so a failed
    // run does not leave a database behind that a later run could read.
    let outcome = round_trip_through_sqlite(&db_url).await;
    let _ = std::fs::remove_dir_all(&dir);

    match outcome {
        Ok(task_id) => Check::pass(
            SQLITE_LABEL,
            format!("task {task_id} written by one handler, read back by another"),
        ),
        Err(detail) => Check::fail(SQLITE_LABEL, detail),
    }
}

/// Writes a task through one handler and reads it back through another.
///
/// Returns the task id on success, or a message naming what went wrong.
#[cfg(feature = "sqlite")]
async fn round_trip_through_sqlite(db_url: &str) -> Result<String, String> {
    use a2a_protocol_server::store::SqliteTaskStore;
    use a2a_protocol_types::params::TaskQueryParams;
    use a2a_protocol_types::responses::SendMessageResponse;

    /// Builds a served handler over `db_url` and a client for it.
    async fn endpoint(
        db_url: &str,
        name: &'static str,
    ) -> Result<a2a_protocol_client::A2aClient, String> {
        let (listener, url) = bind().await;
        let store = SqliteTaskStore::with_migrations(db_url)
            .await
            .map_err(|e| format!("{name}: opening {db_url}: {e}"))?;
        let handler = RequestHandlerBuilder::new(LogSearchExecutor)
            .with_agent_card(plain_card(&url, name))
            .with_task_store(store)
            .build()
            .map_err(|e| format!("{name}: building the handler: {e}"))?;
        serve(listener, Arc::new(handler));
        ClientBuilder::new(&url)
            .build()
            .map_err(|e| format!("{name}: building the client: {e}"))
    }

    let writer = endpoint(db_url, "Durable Agent A").await?;
    let task_id = match writer
        .send_message(send_params(user_message("payments-api")))
        .await
        .map_err(|e| format!("writing through handler A: {e}"))?
    {
        SendMessageResponse::Task(task) => task.id.0.clone(),
        other => return Err(format!("expected a Task from handler A, got {other:?}")),
    };

    let reader = endpoint(db_url, "Durable Agent B").await?;
    let found = reader
        .get_task(TaskQueryParams {
            tenant: None,
            id: task_id.clone(),
            history_length: None,
        })
        .await
        .map_err(|e| format!("task {task_id} did not survive the handler change: {e}"))?;

    if found.id.0 == task_id {
        Ok(task_id)
    } else {
        Err(format!(
            "read back the wrong task: wanted {task_id}, got {}",
            found.id.0
        ))
    }
}

#[cfg(not(feature = "sqlite"))]
pub(super) async fn sqlite_persistence() -> Check {
    Check::skipped(SQLITE_LABEL, "sqlite")
}

// ── Graceful shutdown ────────────────────────────────────────────────────────

/// `RequestHandler::shutdown` must complete rather than hang.
///
/// A shutdown that never returns is the failure worth catching, and a bare
/// `.await` on it would hang this example instead of reporting it — so the call
/// is wrapped in a timeout and the timeout *is* the assertion. The handler is
/// given real work first: shutting down a handler that has never served a
/// request exercises none of the draining logic.
pub(super) async fn graceful_shutdown() -> Check {
    const LABEL: &str = "Graceful shutdown (drains in-flight work, does not hang)";
    const BUDGET: Duration = Duration::from_secs(5);

    let (listener, url) = bind().await;
    let handler = match RequestHandlerBuilder::new(LogSearchExecutor)
        .with_agent_card(plain_card(&url, "Shutdown Agent"))
        .build()
    {
        Ok(handler) => Arc::new(handler),
        Err(e) => return Check::fail(LABEL, format!("building the handler: {e}")),
    };
    serve(listener, Arc::clone(&handler));

    let client = match ClientBuilder::new(&url).build() {
        Ok(client) => client,
        Err(e) => return Check::fail(LABEL, format!("building the client: {e}")),
    };
    if let Err(e) = client
        .send_message(send_params(user_message("payments-api")))
        .await
    {
        return Check::fail(LABEL, format!("the pre-shutdown request failed: {e}"));
    }

    let started = std::time::Instant::now();
    match tokio::time::timeout(BUDGET, handler.shutdown()).await {
        Ok(()) => Check::pass(
            LABEL,
            format!("completed in {:?} (budget {BUDGET:?})", started.elapsed()),
        ),
        Err(_) => Check::fail(
            LABEL,
            format!("shutdown did not return within {BUDGET:?} — it is hanging, not draining"),
        ),
    }
}
