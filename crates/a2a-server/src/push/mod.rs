// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Push notification configuration storage and delivery.

pub mod config_store;
pub mod sender;
pub mod tenant_config_store;

#[cfg(feature = "sqlite")]
pub mod sqlite_config_store;
#[cfg(feature = "sqlite")]
pub mod tenant_sqlite_config_store;

pub use config_store::{InMemoryPushConfigStore, PushConfigStore};
pub use sender::{HttpPushSender, PushRetryPolicy, PushSender};
pub use tenant_config_store::TenantAwareInMemoryPushConfigStore;

#[cfg(feature = "sqlite")]
pub use sqlite_config_store::SqlitePushConfigStore;
#[cfg(feature = "sqlite")]
pub use tenant_sqlite_config_store::TenantAwareSqlitePushConfigStore;
