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

// ── Tenant-partitioned persistence ───────────────────────────────────────────

const TENANT_SQLITE_LABEL: &str = "Tenant-partitioned SQLite (isolation survives a restart)";

/// Tenant isolation and durability are shown separately elsewhere; this shows
/// they hold *together*.
///
/// The combination is where the interesting bug lives. A store that partitions
/// correctly in memory but writes every tenant's rows to one undifferentiated
/// table passes the isolation check (the in-memory map is right) and passes the
/// persistence check (the row comes back) — and leaks across tenants the moment
/// the process restarts, which is the only time the table is the source of
/// truth. Nothing short of writing as two tenants, dropping the handlers, and
/// reading back as both catches it.
#[cfg(feature = "sqlite")]
pub(super) async fn tenant_sqlite() -> Check {
    let dir = std::env::temp_dir().join(format!("a2a-incident-tenant-{}", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Check::fail(
            TENANT_SQLITE_LABEL,
            format!("creating {}: {e}", dir.display()),
        );
    }
    let db_url = format!("sqlite://{}/tenants.db?mode=rwc", dir.display());
    let outcome = tenant_round_trip(&db_url).await;
    let _ = std::fs::remove_dir_all(&dir);

    match outcome {
        Ok(detail) => Check::pass(TENANT_SQLITE_LABEL, detail),
        Err(detail) => Check::fail(TENANT_SQLITE_LABEL, detail),
    }
}

/// Writes one task per tenant, replaces the handler, and reads back as both.
#[cfg(feature = "sqlite")]
async fn tenant_round_trip(db_url: &str) -> Result<String, String> {
    use a2a_protocol_client::A2aClient;
    use a2a_protocol_server::store::TenantAwareSqliteTaskStore;
    use a2a_protocol_server::tenant_resolver::HeaderTenantResolver;
    use a2a_protocol_types::params::ListTasksParams;
    use a2a_protocol_types::responses::SendMessageResponse;

    use super::HeaderInterceptor;

    const TENANTS: [&str; 2] = ["acme", "globex"];

    /// Serves a tenant-partitioned store over `db_url` and returns one client
    /// per tenant, each stamping its own `x-tenant-id`.
    async fn endpoints(db_url: &str, name: &'static str) -> Result<Vec<A2aClient>, String> {
        let (listener, url) = bind().await;
        let store = TenantAwareSqliteTaskStore::new(db_url)
            .await
            .map_err(|e| format!("{name}: opening {db_url}: {e}"))?;
        let handler = RequestHandlerBuilder::new(LogSearchExecutor)
            .with_agent_card(plain_card(&url, name))
            .with_task_store(store)
            .with_tenant_resolver(HeaderTenantResolver::default())
            .build()
            .map_err(|e| format!("{name}: building the handler: {e}"))?;
        serve(listener, Arc::new(handler));

        let mut clients = Vec::new();
        for tenant in TENANTS {
            clients.push(
                ClientBuilder::new(&url)
                    .with_interceptor(HeaderInterceptor::new("x-tenant-id", tenant))
                    .build()
                    .map_err(|e| format!("{name}: building the {tenant} client: {e}"))?,
            );
        }
        Ok(clients)
    }

    let writers = endpoints(db_url, "Partitioned Agent A").await?;
    let mut owned = Vec::new();
    for (tenant, client) in TENANTS.iter().zip(&writers) {
        match client
            .send_message(send_params(user_message("payments-api")))
            .await
            .map_err(|e| format!("{tenant}: writing: {e}"))?
        {
            SendMessageResponse::Task(task) => owned.push((*tenant, task.id.0.clone())),
            other => return Err(format!("{tenant}: expected a Task, got {other:?}")),
        }
    }

    // A second handler over the same file — what a restart leaves behind.
    let readers = endpoints(db_url, "Partitioned Agent B").await?;
    for ((tenant, own_id), client) in owned.iter().zip(&readers) {
        let listed = client
            .list_tasks(ListTasksParams::default())
            .await
            .map_err(|e| format!("{tenant}: listing after the restart: {e}"))?;
        let seen: Vec<&str> = listed.tasks.iter().map(|t| t.id.0.as_str()).collect();
        if !seen.contains(&own_id.as_str()) {
            return Err(format!(
                "{tenant}'s task {own_id} did not survive the restart — the partition is not durable"
            ));
        }
        for (other, other_id) in owned.iter().filter(|(t, _)| t != tenant) {
            if seen.contains(&other_id.as_str()) {
                return Err(format!(
                    "after the restart {tenant} can see {other}'s task {other_id} \
                     — the partitions share a table"
                ));
            }
        }
    }
    Ok(format!(
        "{} tenants written, handler replaced, each still sees only its own",
        owned.len()
    ))
}

#[cfg(not(feature = "sqlite"))]
pub(super) async fn tenant_sqlite() -> Check {
    Check::skipped(TENANT_SQLITE_LABEL, "sqlite")
}

// ── PostgreSQL-backed persistence ────────────────────────────────────────────

const POSTGRES_LABEL: &str = "PostgreSQL persistence (survives a handler restart)";

/// The environment variable naming a database to run the Postgres check
/// against.
///
/// Same name `ci.yml`'s `test-postgres` job already sets, so a developer who
/// has configured one for the server's integration tests gets this for free.
#[cfg(feature = "postgres")]
const POSTGRES_URL_ENV: &str = "A2A_TEST_POSTGRES_URL";

/// The SQLite round-trip against a real PostgreSQL server.
///
/// Unlike every other check here this one needs a service the example cannot
/// start, so it reports `[NOT RUN]` — naming the variable to set — when none is
/// configured. That is deliberately *not* the same as passing: the two SQL
/// stores are separate implementations of the same trait and share no query
/// text, so a green SQLite check says nothing about Postgres.
#[cfg(feature = "postgres")]
pub(super) async fn postgres_persistence() -> Check {
    let Ok(url) = std::env::var(POSTGRES_URL_ENV) else {
        return Check::unavailable(
            POSTGRES_LABEL,
            format!("set {POSTGRES_URL_ENV} to a PostgreSQL URL to exercise this"),
        );
    };
    match postgres_round_trip(&url).await {
        Ok(task_id) => Check::pass(
            POSTGRES_LABEL,
            format!("task {task_id} written by one handler, read back by another"),
        ),
        Err(detail) => Check::fail(POSTGRES_LABEL, detail),
    }
}

/// Writes a task through one handler and reads it back through another.
#[cfg(feature = "postgres")]
async fn postgres_round_trip(url: &str) -> Result<String, String> {
    use a2a_protocol_server::store::PostgresTaskStore;
    use a2a_protocol_types::params::TaskQueryParams;
    use a2a_protocol_types::responses::SendMessageResponse;

    async fn endpoint(
        url: &str,
        name: &'static str,
    ) -> Result<a2a_protocol_client::A2aClient, String> {
        let (listener, agent_url) = bind().await;
        let store = PostgresTaskStore::with_migrations(url)
            .await
            .map_err(|e| format!("{name}: connecting: {e}"))?;
        let handler = RequestHandlerBuilder::new(LogSearchExecutor)
            .with_agent_card(plain_card(&agent_url, name))
            .with_task_store(store)
            .build()
            .map_err(|e| format!("{name}: building the handler: {e}"))?;
        serve(listener, Arc::new(handler));
        ClientBuilder::new(&agent_url)
            .build()
            .map_err(|e| format!("{name}: building the client: {e}"))
    }

    let writer = endpoint(url, "Postgres Agent A").await?;
    let task_id = match writer
        .send_message(send_params(user_message("payments-api")))
        .await
        .map_err(|e| format!("writing through handler A: {e}"))?
    {
        SendMessageResponse::Task(task) => task.id.0.clone(),
        other => return Err(format!("expected a Task from handler A, got {other:?}")),
    };

    let reader = endpoint(url, "Postgres Agent B").await?;
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

#[cfg(not(feature = "postgres"))]
pub(super) async fn postgres_persistence() -> Check {
    Check::skipped(POSTGRES_LABEL, "postgres")
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
