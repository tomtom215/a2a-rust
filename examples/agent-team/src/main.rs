// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! **Agent Team** — Full-stack dogfood of every a2a-rust SDK capability.
//!
//! This example deploys 4 specialized agents that communicate via A2A:
//!
//! | Agent | Transport | Skills | Features exercised |
//! |-------|-----------|--------|-------------------|
//! | **CodeAnalyzer** | JSON-RPC | file analysis, LOC counting | Streaming, artifacts, multi-part |
//! | **BuildMonitor** | REST | cargo check/test runner | Streaming, cancellation, task lifecycle |
//! | **HealthMonitor** | JSON-RPC | agent health checks | Push notifications, interceptors |
//! | **Coordinator** | REST | orchestration, delegation | A2A client calls, task aggregation, metrics |
//!
//! The binary starts all 4 agent servers, then runs a comprehensive E2E test
//! suite (40 tests) that exercises every major SDK feature.
//!
//! Run with: `cargo run -p agent-team`
//! With logging: `RUST_LOG=debug cargo run -p agent-team --features tracing`

mod cards;
mod executors;
mod helpers;
mod infrastructure;
mod tests;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::push::HttpPushSender;

use cards::{build_monitor_card, code_analyzer_card, coordinator_card, health_monitor_card};
use executors::{
    BuildMonitorExecutor, CodeAnalyzerExecutor, CoordinatorExecutor, HealthMonitorExecutor,
};
use infrastructure::{
    start_jsonrpc_server, start_rest_server, start_webhook_server, AuditInterceptor,
    MetricsForward, TeamMetrics, WebhookReceiver,
};
use tests::{basic, edge_cases, lifecycle, stress, TestContext, TestResult};

#[tokio::main]
async fn main() {
    #[cfg(feature = "tracing")]
    {
        use tracing_subscriber::EnvFilter;
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .init();
    }

    let total_start = Instant::now();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     A2A Agent Team — Full SDK Dogfood & E2E Test Suite     ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // ── Start webhook receiver for push notifications ────────────────────
    let webhook_receiver = WebhookReceiver::new();
    let webhook_addr = start_webhook_server(webhook_receiver.clone()).await;
    println!("Webhook receiver listening on http://{webhook_addr}\n");

    // ── Build shared metrics ─────────────────────────────────────────────
    let analyzer_metrics = Arc::new(TeamMetrics::new("CodeAnalyzer"));
    let build_metrics = Arc::new(TeamMetrics::new("BuildMonitor"));
    let health_metrics = Arc::new(TeamMetrics::new("HealthMonitor"));
    let coordinator_metrics = Arc::new(TeamMetrics::new("Coordinator"));

    // ── Agent 1: Code Analyzer (JSON-RPC) ────────────────────────────────
    let analyzer_handler = Arc::new(
        RequestHandlerBuilder::new(CodeAnalyzerExecutor)
            .with_agent_card(code_analyzer_card("http://placeholder"))
            .with_interceptor(AuditInterceptor::new("CodeAnalyzer"))
            .with_metrics(MetricsForward(Arc::clone(&analyzer_metrics)))
            .with_executor_timeout(std::time::Duration::from_secs(30))
            .with_event_queue_capacity(128)
            .build()
            .expect("build code analyzer handler"),
    );
    let analyzer_addr = start_jsonrpc_server(Arc::clone(&analyzer_handler)).await;
    let analyzer_url = format!("http://{analyzer_addr}");
    println!("Agent [CodeAnalyzer]  JSON-RPC on {analyzer_url}");

    // ── Agent 2: Build Monitor (REST) ────────────────────────────────────
    let build_handler = Arc::new(
        RequestHandlerBuilder::new(BuildMonitorExecutor)
            .with_agent_card(build_monitor_card("http://placeholder"))
            .with_interceptor(AuditInterceptor::new("BuildMonitor").with_token("build-secret"))
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            .with_metrics(MetricsForward(Arc::clone(&build_metrics)))
            .with_executor_timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("build build monitor handler"),
    );
    let build_addr = start_rest_server(Arc::clone(&build_handler)).await;
    let build_url = format!("http://{build_addr}");
    println!("Agent [BuildMonitor]  REST     on {build_url}");

    // ── Agent 3: Health Monitor (JSON-RPC) ───────────────────────────────
    let health_handler = Arc::new(
        RequestHandlerBuilder::new(HealthMonitorExecutor)
            .with_agent_card(health_monitor_card("http://placeholder"))
            .with_interceptor(AuditInterceptor::new("HealthMonitor"))
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            .with_metrics(MetricsForward(Arc::clone(&health_metrics)))
            .build()
            .expect("build health monitor handler"),
    );
    let health_addr = start_jsonrpc_server(Arc::clone(&health_handler)).await;
    let health_url = format!("http://{health_addr}");
    println!("Agent [HealthMonitor] JSON-RPC on {health_url}");

    // ── Agent 4: Coordinator (REST) ──────────────────────────────────────
    let mut agent_urls = HashMap::new();
    agent_urls.insert("code_analyzer".into(), analyzer_url.clone());
    agent_urls.insert("build_monitor".into(), build_url.clone());
    agent_urls.insert("health_monitor".into(), health_url.clone());

    let coord_handler = Arc::new(
        RequestHandlerBuilder::new(CoordinatorExecutor::new(agent_urls))
            .with_agent_card(coordinator_card("http://placeholder"))
            .with_interceptor(AuditInterceptor::new("Coordinator"))
            .with_metrics(MetricsForward(Arc::clone(&coordinator_metrics)))
            .with_max_concurrent_streams(50)
            .build()
            .expect("build coordinator handler"),
    );
    let coord_addr = start_rest_server(Arc::clone(&coord_handler)).await;
    let coordinator_url = format!("http://{coord_addr}");
    println!("Agent [Coordinator]   REST     on {coordinator_url}");
    println!();

    // ── Build test context ───────────────────────────────────────────────
    let ctx = TestContext {
        analyzer_url,
        build_url,
        health_url,
        coordinator_url,
        webhook_addr,
        webhook_receiver: webhook_receiver.clone(),
        analyzer_metrics: Arc::clone(&analyzer_metrics),
        build_metrics: Arc::clone(&build_metrics),
        health_metrics: Arc::clone(&health_metrics),
        coordinator_metrics: Arc::clone(&coordinator_metrics),
    };

    // ── Run E2E test suite ───────────────────────────────────────────────
    let mut results: Vec<TestResult> = Vec::new();

    // Tests 1-10: Core send/stream/REST/JSON-RPC paths
    results.push(basic::test_sync_jsonrpc_send(&ctx).await);
    results.push(basic::test_streaming_jsonrpc(&ctx).await);
    results.push(basic::test_sync_rest_send(&ctx).await);
    results.push(basic::test_streaming_rest(&ctx).await);
    results.push(basic::test_build_failure_path(&ctx).await);
    results.push(basic::test_get_task(&ctx).await);
    results.push(basic::test_list_tasks(&ctx).await);
    results.push(basic::test_push_config_crud(&ctx).await);
    results.push(basic::test_multi_part_message(&ctx).await);
    results.push(basic::test_agent_to_agent(&ctx).await);

    // Tests 11-20: Orchestration, metadata, cancel, agent cards
    results.push(lifecycle::test_full_orchestration(&ctx).await);
    results.push(lifecycle::test_health_orchestration(&ctx).await);
    results.push(lifecycle::test_message_metadata(&ctx).await);
    results.push(lifecycle::test_cancel_task(&ctx).await);
    results.push(lifecycle::test_get_nonexistent_task(&ctx).await);
    results.push(lifecycle::test_pagination_walk(&ctx).await);
    results.push(lifecycle::test_agent_card_rest(&ctx).await);
    results.push(lifecycle::test_agent_card_jsonrpc(&ctx).await);
    results.push(lifecycle::test_push_not_supported(&ctx).await);
    results.push(lifecycle::test_cancel_completed(&ctx).await);

    // Tests 21-30: Error paths, concurrency, metrics, CRUD
    results.push(edge_cases::test_cancel_nonexistent(&ctx).await);
    results.push(edge_cases::test_return_immediately(&ctx).await);
    results.push(edge_cases::test_concurrent_requests(&ctx).await);
    results.push(edge_cases::test_empty_parts_rejected(&ctx).await);
    results.push(edge_cases::test_get_task_rest(&ctx).await);
    results.push(edge_cases::test_list_tasks_rest(&ctx).await);
    results.push(edge_cases::test_push_crud_jsonrpc(&ctx).await);
    results.push(edge_cases::test_resubscribe_rest(&ctx).await);
    results.push(edge_cases::test_metrics_nonzero(&ctx).await);
    results.push(edge_cases::test_error_metrics_tracked(&ctx).await);

    // Tests 31-40: Stress, durability, observability, event ordering
    results.push(stress::test_high_concurrency(&ctx).await);
    results.push(stress::test_mixed_transport_concurrent(&ctx).await);
    results.push(stress::test_context_continuation(&ctx).await);
    results.push(stress::test_large_payload(&ctx).await);
    results.push(stress::test_stream_with_get_task(&ctx).await);
    results.push(stress::test_push_delivery_e2e(&ctx).await);
    results.push(stress::test_list_tasks_status_filter(&ctx).await);
    results.push(stress::test_graceful_shutdown_semantics(&ctx).await);
    results.push(stress::test_queue_depth_metrics(&ctx).await);
    results.push(stress::test_event_ordering(&ctx).await);

    // ── Report ───────────────────────────────────────────────────────────
    let total_duration = total_start.elapsed();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.iter().filter(|r| !r.passed).count();
    let total = results.len();

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                      TEST RESULTS                          ║");
    println!("╠══════════════════════════════════════════════════════════════╣");

    for r in &results {
        let icon = if r.passed { "PASS" } else { "FAIL" };
        println!(
            "║ [{icon}] {:30} {:>6}ms  {}",
            r.name,
            r.duration_ms,
            if r.details.len() > 30 {
                format!("{}...", &r.details[..27])
            } else {
                r.details.clone()
            }
        );
    }

    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║ Total: {total} | Passed: {passed} | Failed: {failed} | Time: {}ms",
        total_duration.as_millis()
    );
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║                    AGENT METRICS                           ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ {}", analyzer_metrics.summary());
    println!("║ {}", build_metrics.summary());
    println!("║ {}", health_metrics.summary());
    println!("║ {}", coordinator_metrics.summary());

    // Push notification summary.
    let push_events = webhook_receiver.drain().await;
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ Push notifications received: {}", push_events.len());
    for (kind, _value) in &push_events {
        println!("║   - {kind}");
    }

    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║                SDK FEATURES EXERCISED                      ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    let features = [
        "AgentExecutor trait (4 implementations)",
        "RequestHandlerBuilder (all options)",
        "JsonRpcDispatcher",
        "RestDispatcher",
        "ClientBuilder (JSON-RPC + REST)",
        "Sync SendMessage",
        "Streaming SendStreamingMessage",
        "EventStream consumer",
        "GetTask",
        "ListTasks with pagination",
        "CancelTask executor override",
        "Push notification config CRUD",
        "HttpPushSender delivery",
        "Webhook receiver",
        "ServerInterceptor (audit + auth)",
        "Custom Metrics observer",
        "AgentCard discovery",
        "Multi-part messages (text + data)",
        "Artifact append mode",
        "TaskState lifecycle (all states)",
        "CancellationToken checking",
        "Executor timeout config",
        "Event queue capacity config",
        "Max concurrent streams config",
        "Agent-to-agent A2A communication",
        "Multi-level orchestration",
        "Request metadata",
    ];
    for f in &features {
        println!("║   [x] {f}");
    }

    println!("╚══════════════════════════════════════════════════════════════╝");

    if failed > 0 {
        std::process::exit(1);
    }

    println!(
        "\nAll {passed} tests passed in {}ms. SDK fully verified.",
        total_duration.as_millis()
    );
}
