// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for [`HandlerLimits`](super::HandlerLimits).
//!
//! Split from `mod.rs` when it crossed the 500-line limit `CONTRIBUTING.md`
//! sets, following the same shape as `trace_context/tests.rs` and
//! `failure/tests.rs`.

use super::*;

#[test]
fn default_values() {
    let limits = HandlerLimits::default();
    assert_eq!(limits.max_id_length, 1024);
    assert_eq!(limits.max_metadata_size, 1_048_576);
    assert_eq!(limits.max_cancellation_tokens, 10_000);
    assert_eq!(limits.max_token_age, Duration::from_secs(3600));
    assert_eq!(limits.push_delivery_timeout, Duration::from_secs(5));
    assert_eq!(limits.push_delivery_budget, Duration::from_secs(30));
    assert_eq!(limits.executor_drain_timeout, Duration::from_secs(5));
    assert_eq!(limits.max_artifacts_per_task, 1000);
    assert_eq!(limits.max_context_locks, 10_000);
    assert_eq!(limits.max_push_configs_per_task, 100);
    assert_eq!(limits.max_parts_per_artifact, 10_000);
    assert_eq!(limits.max_total_push_configs, 100_000);
    assert_eq!(
        limits.subscribe_reattach_interval,
        Duration::from_millis(250)
    );
    assert_eq!(limits.subscribe_max_idle, Duration::from_secs(300));
}

#[test]
fn with_push_delivery_budget_and_executor_drain_timeout_set_their_values() {
    let limits = HandlerLimits::default()
        .with_push_delivery_budget(Duration::from_secs(90))
        .with_executor_drain_timeout(Duration::from_millis(750));
    assert_eq!(limits.push_delivery_budget, Duration::from_secs(90));
    assert_eq!(limits.executor_drain_timeout, Duration::from_millis(750));
}

#[test]
fn with_max_id_length_sets_value() {
    let limits = HandlerLimits::default().with_max_id_length(2048);
    assert_eq!(limits.max_id_length, 2048);
}

#[test]
fn with_max_metadata_size_sets_value() {
    let limits = HandlerLimits::default().with_max_metadata_size(2_097_152);
    assert_eq!(limits.max_metadata_size, 2_097_152);
}

#[test]
fn with_max_cancellation_tokens_sets_value() {
    let limits = HandlerLimits::default().with_max_cancellation_tokens(5_000);
    assert_eq!(limits.max_cancellation_tokens, 5_000);
}

#[test]
fn with_max_token_age_sets_value() {
    let limits = HandlerLimits::default().with_max_token_age(Duration::from_secs(7200));
    assert_eq!(limits.max_token_age, Duration::from_secs(7200));
}

#[test]
fn with_push_delivery_timeout_sets_value() {
    let limits = HandlerLimits::default().with_push_delivery_timeout(Duration::from_secs(10));
    assert_eq!(limits.push_delivery_timeout, Duration::from_secs(10));
}

#[test]
fn builder_chaining() {
    let limits = HandlerLimits::default()
        .with_max_id_length(512)
        .with_max_metadata_size(500_000)
        .with_max_cancellation_tokens(1_000)
        .with_max_token_age(Duration::from_secs(1800))
        .with_push_delivery_timeout(Duration::from_secs(15));

    assert_eq!(limits.max_id_length, 512);
    assert_eq!(limits.max_metadata_size, 500_000);
    assert_eq!(limits.max_cancellation_tokens, 1_000);
    assert_eq!(limits.max_token_age, Duration::from_secs(1800));
    assert_eq!(limits.push_delivery_timeout, Duration::from_secs(15));
}

#[test]
fn with_max_artifacts_per_task_sets_value() {
    let limits = HandlerLimits::default().with_max_artifacts_per_task(500);
    assert_eq!(limits.max_artifacts_per_task, 500);
}

#[test]
fn debug_format() {
    let limits = HandlerLimits::default();
    let debug = format!("{limits:?}");
    assert!(debug.contains("HandlerLimits"));
    assert!(debug.contains("max_id_length"));
    assert!(debug.contains("max_metadata_size"));
    assert!(debug.contains("max_cancellation_tokens"));
    assert!(debug.contains("max_token_age"));
    assert!(debug.contains("push_delivery_timeout"));
    assert!(debug.contains("push_delivery_budget"));
    assert!(debug.contains("executor_drain_timeout"));
    assert!(debug.contains("max_artifacts_per_task"));
    assert!(debug.contains("max_context_locks"));
}
