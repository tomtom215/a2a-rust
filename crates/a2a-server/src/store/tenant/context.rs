// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tenant identity propagation via task-local storage.

use std::future::Future;

tokio::task_local! {
    static CURRENT_TENANT: String;
}

/// Thread-safe tenant context for scoping store operations.
///
/// Uses `tokio::task_local!` to propagate tenant identity into store calls
/// without changing the `TaskStore` trait.
pub struct TenantContext;

impl TenantContext {
    /// Runs a future within a tenant scope.
    ///
    /// All `TenantAwareInMemoryTaskStore` operations executed within `f`
    /// will be scoped to the given tenant.
    pub async fn scope<F, R>(tenant: impl Into<String>, f: F) -> R
    where
        F: Future<Output = R>,
    {
        CURRENT_TENANT.scope(tenant.into(), f).await
    }

    /// Returns the current tenant ID, or `""` if no tenant context is set.
    #[must_use]
    pub fn current() -> String {
        CURRENT_TENANT
            .try_with(std::clone::Clone::clone)
            .unwrap_or_default()
    }
}
