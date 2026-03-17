// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Per-tenant configuration for multi-tenant A2A servers.
//!
//! [`PerTenantConfig`] allows operators to differentiate service levels across
//! tenants by setting per-tenant timeouts, capacity limits, rate limits, and
//! other resource constraints.
//!
//! # Example
//!
//! ```rust
//! use std::time::Duration;
//! use a2a_protocol_server::tenant_config::{PerTenantConfig, TenantLimits};
//!
//! let config = PerTenantConfig::builder()
//!     .default_limits(TenantLimits::builder()
//!         .max_concurrent_tasks(100)
//!         .rate_limit_rps(50)
//!         .build())
//!     .with_override("premium-corp", TenantLimits::builder()
//!         .max_concurrent_tasks(1000)
//!         .executor_timeout(Duration::from_secs(120))
//!         .rate_limit_rps(500)
//!         .build())
//!     .build();
//!
//! // "premium-corp" gets premium limits:
//! assert_eq!(config.get("premium-corp").max_concurrent_tasks, Some(1000));
//!
//! // Unknown tenants get defaults:
//! assert_eq!(config.get("unknown").max_concurrent_tasks, Some(100));
//! ```

use std::collections::HashMap;
use std::time::Duration;

// ── TenantLimits ─────────────────────────────────────────────────────────────

/// Resource limits and configuration for a single tenant.
///
/// All fields default to `None`, meaning "no limit" or "use the handler/store
/// default". Use the [builder](TenantLimits::builder) pattern for ergonomic
/// construction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TenantLimits {
    /// Maximum concurrent tasks for this tenant. `None` = unlimited.
    pub max_concurrent_tasks: Option<usize>,

    /// Executor timeout override. `None` = use handler default.
    pub executor_timeout: Option<Duration>,

    /// Maximum event queue capacity per stream. `None` = use handler default.
    pub event_queue_capacity: Option<usize>,

    /// Maximum tasks stored. `None` = use store default.
    pub max_stored_tasks: Option<usize>,

    /// Rate limit (requests per second). `None` = no tenant-level rate limit.
    pub rate_limit_rps: Option<u32>,
}

impl TenantLimits {
    /// Returns a builder for constructing [`TenantLimits`].
    #[must_use]
    pub fn builder() -> TenantLimitsBuilder {
        TenantLimitsBuilder::default()
    }
}

/// Builder for [`TenantLimits`].
///
/// All fields default to `None` (no limit / use handler default).
#[derive(Debug, Clone, Default)]
pub struct TenantLimitsBuilder {
    max_concurrent_tasks: Option<usize>,
    executor_timeout: Option<Duration>,
    event_queue_capacity: Option<usize>,
    max_stored_tasks: Option<usize>,
    rate_limit_rps: Option<u32>,
}

impl TenantLimitsBuilder {
    /// Sets the maximum concurrent tasks.
    #[must_use]
    pub const fn max_concurrent_tasks(mut self, n: usize) -> Self {
        self.max_concurrent_tasks = Some(n);
        self
    }

    /// Sets the executor timeout.
    #[must_use]
    pub const fn executor_timeout(mut self, d: Duration) -> Self {
        self.executor_timeout = Some(d);
        self
    }

    /// Sets the event queue capacity per stream.
    #[must_use]
    pub const fn event_queue_capacity(mut self, n: usize) -> Self {
        self.event_queue_capacity = Some(n);
        self
    }

    /// Sets the maximum stored tasks.
    #[must_use]
    pub const fn max_stored_tasks(mut self, n: usize) -> Self {
        self.max_stored_tasks = Some(n);
        self
    }

    /// Sets the rate limit in requests per second.
    #[must_use]
    pub const fn rate_limit_rps(mut self, rps: u32) -> Self {
        self.rate_limit_rps = Some(rps);
        self
    }

    /// Builds the [`TenantLimits`].
    #[must_use]
    pub const fn build(self) -> TenantLimits {
        TenantLimits {
            max_concurrent_tasks: self.max_concurrent_tasks,
            executor_timeout: self.executor_timeout,
            event_queue_capacity: self.event_queue_capacity,
            max_stored_tasks: self.max_stored_tasks,
            rate_limit_rps: self.rate_limit_rps,
        }
    }
}

// ── PerTenantConfig ──────────────────────────────────────────────────────────

/// Per-tenant configuration for timeouts, capacity limits, and executor selection.
///
/// Allows operators to differentiate service levels across tenants. Use
/// [`get`](Self::get) to resolve the effective limits for a tenant — it returns
/// the tenant-specific overrides if present, or falls back to the default.
#[derive(Debug, Clone, Default)]
pub struct PerTenantConfig {
    /// Default configuration for tenants without specific overrides.
    pub default: TenantLimits,

    /// Per-tenant overrides keyed by tenant ID.
    pub overrides: HashMap<String, TenantLimits>,
}

impl PerTenantConfig {
    /// Returns a builder for constructing [`PerTenantConfig`].
    #[must_use]
    pub fn builder() -> PerTenantConfigBuilder {
        PerTenantConfigBuilder::default()
    }

    /// Returns the effective limits for the given tenant.
    ///
    /// If the tenant has a specific override, that is returned. Otherwise the
    /// default limits are returned.
    #[must_use]
    pub fn get(&self, tenant_id: &str) -> &TenantLimits {
        self.overrides.get(tenant_id).unwrap_or(&self.default)
    }
}

/// Builder for [`PerTenantConfig`].
#[derive(Debug, Clone, Default)]
pub struct PerTenantConfigBuilder {
    default: TenantLimits,
    overrides: HashMap<String, TenantLimits>,
}

impl PerTenantConfigBuilder {
    /// Sets the default tenant limits applied when no override matches.
    #[must_use]
    pub const fn default_limits(mut self, limits: TenantLimits) -> Self {
        self.default = limits;
        self
    }

    /// Adds a per-tenant override.
    #[must_use]
    pub fn with_override(mut self, tenant_id: impl Into<String>, limits: TenantLimits) -> Self {
        self.overrides.insert(tenant_id.into(), limits);
        self
    }

    /// Builds the [`PerTenantConfig`].
    #[must_use]
    pub fn build(self) -> PerTenantConfig {
        PerTenantConfig {
            default: self.default,
            overrides: self.overrides,
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits_are_all_none() {
        let limits = TenantLimits::default();
        assert_eq!(limits.max_concurrent_tasks, None);
        assert_eq!(limits.executor_timeout, None);
        assert_eq!(limits.event_queue_capacity, None);
        assert_eq!(limits.max_stored_tasks, None);
        assert_eq!(limits.rate_limit_rps, None);
    }

    #[test]
    fn builder_sets_all_fields() {
        let limits = TenantLimits::builder()
            .max_concurrent_tasks(10)
            .executor_timeout(Duration::from_secs(30))
            .event_queue_capacity(256)
            .max_stored_tasks(1000)
            .rate_limit_rps(100)
            .build();

        assert_eq!(limits.max_concurrent_tasks, Some(10));
        assert_eq!(limits.executor_timeout, Some(Duration::from_secs(30)));
        assert_eq!(limits.event_queue_capacity, Some(256));
        assert_eq!(limits.max_stored_tasks, Some(1000));
        assert_eq!(limits.rate_limit_rps, Some(100));
    }

    #[test]
    fn per_tenant_config_returns_override() {
        let config = PerTenantConfig::builder()
            .default_limits(TenantLimits::builder().max_concurrent_tasks(10).build())
            .with_override(
                "premium",
                TenantLimits::builder().max_concurrent_tasks(1000).build(),
            )
            .build();

        assert_eq!(config.get("premium").max_concurrent_tasks, Some(1000));
    }

    #[test]
    fn per_tenant_config_falls_back_to_default() {
        let config = PerTenantConfig::builder()
            .default_limits(TenantLimits::builder().rate_limit_rps(50).build())
            .build();

        assert_eq!(config.get("unknown-tenant").rate_limit_rps, Some(50));
    }

    #[test]
    fn per_tenant_config_default_is_empty() {
        let config = PerTenantConfig::default();
        let limits = config.get("any");
        assert_eq!(*limits, TenantLimits::default());
    }

    #[test]
    fn multiple_overrides() {
        let config = PerTenantConfig::builder()
            .default_limits(TenantLimits::default())
            .with_override("a", TenantLimits::builder().rate_limit_rps(10).build())
            .with_override("b", TenantLimits::builder().rate_limit_rps(20).build())
            .build();

        assert_eq!(config.get("a").rate_limit_rps, Some(10));
        assert_eq!(config.get("b").rate_limit_rps, Some(20));
        assert_eq!(config.get("c").rate_limit_rps, None);
    }

    #[test]
    fn tenant_limits_builder_returns_functional_builder() {
        // Verifies TenantLimits::builder() returns a real builder (not Default::default()).
        let limits = TenantLimits::builder().max_concurrent_tasks(42).build();
        assert_eq!(limits.max_concurrent_tasks, Some(42));
    }

    #[test]
    fn per_tenant_config_builder_returns_functional_builder() {
        // Verifies PerTenantConfig::builder() returns a real builder (not Default::default()).
        let config = PerTenantConfig::builder()
            .default_limits(TenantLimits::builder().rate_limit_rps(99).build())
            .build();
        assert_eq!(config.get("any").rate_limit_rps, Some(99));
    }
}
