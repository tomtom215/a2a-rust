// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tenant-scoped task store implementations.
//!
//! Provides [`TenantContext`] for threading tenant identity through store
//! operations and [`TenantAwareInMemoryTaskStore`] for full tenant isolation
//! without changing the [`TaskStore`](super::TaskStore) trait signature.
//!
//! # Architecture
//!
//! The A2A `TaskStore` trait doesn't accept a tenant parameter — tenant
//! identity flows through request params at the handler level. To achieve
//! tenant isolation without a breaking trait change, we use `tokio::task_local!`
//! to propagate the current tenant ID into store operations.
//!
//! The handler sets the tenant context before calling store methods:
//!
//! ```rust,no_run
//! use a2a_protocol_server::store::tenant::TenantContext;
//!
//! # async fn example(store: &impl a2a_protocol_server::store::TaskStore) {
//! let tenant = "acme-corp";
//! TenantContext::scope(tenant, async {
//!     // All store operations here are scoped to "acme-corp"
//!     // store.save(task).await;
//! }).await;
//! # }
//! ```
//!
//! # Isolation guarantees
//!
//! - Each tenant's tasks live in a separate `InMemoryTaskStore` instance
//! - Tenant A cannot read, list, or delete tenant B's tasks
//! - Per-tenant eviction configuration is supported
//! - Operations without a tenant context use the `""` (default) partition

mod context;
mod store;

pub use context::TenantContext;
pub use store::{TenantAwareInMemoryTaskStore, TenantStoreConfig};
