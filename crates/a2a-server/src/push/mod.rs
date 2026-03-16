// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Push notification configuration storage and delivery.

pub mod config_store;
pub mod sender;

#[cfg(feature = "sqlite")]
pub mod sqlite_config_store;

pub use config_store::{InMemoryPushConfigStore, PushConfigStore};
pub use sender::{HttpPushSender, PushRetryPolicy, PushSender};

#[cfg(feature = "sqlite")]
pub use sqlite_config_store::SqlitePushConfigStore;
