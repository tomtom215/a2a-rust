// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the per-tenant configuration types. Split out of the module
//! on 2026-09-10 when the setter-pinning test took the file over the
//! 500-line limit; the tests are unchanged.

use super::*;

#[test]
fn default_limits_are_all_none() {
    let limits = TenantLimits::default();
    assert_eq!(limits.max_concurrent_tasks, None);
    assert_eq!(limits.executor_timeout, None);
    assert_eq!(limits.event_queue_capacity, None);
    assert_eq!(limits.rate_limit_rps, None);
}

#[test]
fn builder_sets_all_fields() {
    let limits = TenantLimits::builder()
        .max_concurrent_tasks(10)
        .executor_timeout(Duration::from_secs(30))
        .event_queue_capacity(256)
        .rate_limit_rps(100)
        .build();

    assert_eq!(limits.max_concurrent_tasks, Some(10));
    assert_eq!(limits.executor_timeout, Some(Duration::from_secs(30)));
    assert_eq!(limits.event_queue_capacity, Some(256));
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

/// Every `TenantLimits` setter writes its own field (all default to
/// `None`), and every `PerTenantConfig` setter writes its own.
#[test]
#[allow(deprecated)]
fn every_setter_sets_its_field() {
    let limits = TenantLimits::default()
        .with_max_concurrent_tasks(Some(1))
        .with_executor_timeout(Some(Duration::from_secs(2)))
        .with_event_queue_capacity(Some(3))
        .with_max_stored_tasks(Some(4))
        .with_rate_limit_rps(Some(5));
    assert_eq!(limits.max_concurrent_tasks, Some(1));
    assert_eq!(limits.executor_timeout, Some(Duration::from_secs(2)));
    assert_eq!(limits.event_queue_capacity, Some(3));
    assert_eq!(limits.max_stored_tasks, Some(4));
    assert_eq!(limits.rate_limit_rps, Some(5));

    let mut overrides = HashMap::new();
    overrides.insert(
        "a".to_owned(),
        TenantLimits::default().with_rate_limit_rps(Some(9)),
    );
    let cfg = PerTenantConfig::default()
        .with_default(limits)
        .with_overrides(overrides)
        .with_override("b", TenantLimits::default().with_rate_limit_rps(Some(8)));
    assert_eq!(cfg.default.max_concurrent_tasks, Some(1));
    assert_eq!(
        cfg.overrides.len(),
        2,
        "with_overrides replaces, with_override adds"
    );
    assert_eq!(cfg.overrides["a"].rate_limit_rps, Some(9));
    assert_eq!(cfg.overrides["b"].rate_limit_rps, Some(8));
}
