// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task storage backend.

pub mod task_store;
pub mod tenant;

#[cfg(feature = "sqlite")]
pub mod migration;
#[cfg(feature = "sqlite")]
pub mod sqlite_store;
#[cfg(feature = "sqlite")]
pub mod tenant_sqlite_store;

pub use task_store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};
pub use tenant::{TenantAwareInMemoryTaskStore, TenantContext, TenantStoreConfig};

#[cfg(feature = "sqlite")]
pub use migration::{Migration, MigrationRunner};
#[cfg(feature = "sqlite")]
pub use sqlite_store::SqliteTaskStore;
#[cfg(feature = "sqlite")]
pub use tenant_sqlite_store::TenantAwareSqliteTaskStore;
