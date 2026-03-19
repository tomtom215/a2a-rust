// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Task storage backend.

pub mod task_store;
pub mod tenant;

#[cfg(feature = "sqlite")]
pub mod migration;
#[cfg(feature = "sqlite")]
pub mod sqlite_store;
#[cfg(feature = "sqlite")]
pub mod tenant_sqlite_store;

#[cfg(feature = "postgres")]
pub mod pg_migration;
#[cfg(feature = "postgres")]
pub mod postgres_store;
#[cfg(feature = "postgres")]
pub mod tenant_postgres_store;

pub use task_store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};
pub use tenant::{TenantAwareInMemoryTaskStore, TenantContext, TenantStoreConfig};

#[cfg(feature = "sqlite")]
pub use migration::{Migration, MigrationRunner};
#[cfg(feature = "sqlite")]
pub use sqlite_store::SqliteTaskStore;
#[cfg(feature = "sqlite")]
pub use tenant_sqlite_store::TenantAwareSqliteTaskStore;

#[cfg(feature = "postgres")]
pub use pg_migration::{PgMigration, PgMigrationRunner};
#[cfg(feature = "postgres")]
pub use postgres_store::PostgresTaskStore;
#[cfg(feature = "postgres")]
pub use tenant_postgres_store::TenantAwarePostgresTaskStore;
