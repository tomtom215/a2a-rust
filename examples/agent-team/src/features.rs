// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The "SDK features exercised" claim table, and the check that keeps it
//! honest.
//!
//! # Why this module exists
//!
//! Until 2026-08-11 the summary this produces was a hardcoded array of strings
//! printed as `[x] <label>` in a loop, with no connection to the test results.
//! A run that failed six batch tests still printed
//! `[x] Batch JSON-RPC (single, multi, empty, mixed, streaming rejection)`.
//! Feature-gated labels were `#[cfg]`'d out of the *array*, so a build without
//! `--features websocket` simply omitted the WebSocket line rather than saying
//! it had not been exercised — absence rendered as completeness.
//!
//! So the checklist was decorative: it could not fail. That is the same defect
//! class this repository has found in five CI gates, and this module is the
//! fix. Every claim now names the tests that prove it, and the claim's marker
//! is computed from their outcomes:
//!
//! | Marker | Meaning |
//! |---|---|
//! | `[x]` | every backing test ran and passed |
//! | `[FAIL]` | at least one backing test ran and failed |
//! | `[ ] NOT RUN` | the backing tests did not run (feature not compiled in) |
//!
//! # The drift check
//!
//! A claim table that is merely *rendered* from results still rots: add a test
//! and forget to claim it, or delete a test and leave the claim, and the
//! summary quietly describes something other than what ran. So
//! [`audit`] checks both directions and the binary exits non-zero on either
//! failure:
//!
//! * every claim must name at least one test that actually ran, unless the
//!   claim is gated on a feature that is off; and
//! * every test that ran must be named by at least one claim.
//!
//! That is what makes this a gate rather than a nicer-looking decoration.

use std::collections::{BTreeSet, HashMap};

use crate::tests::TestResult;

/// Builds the claim table for this build.
///
/// `compiled_in` is evaluated with `cfg!` rather than `#[cfg]` so that a
/// disabled feature still produces a row — printed `[ ] NOT RUN` — instead of
/// vanishing from the summary.
#[allow(clippy::too_many_lines)] // One row per claim; a table, not logic.
pub fn claims() -> Vec<FeatureClaim> {
    let c = |label, backed_by| FeatureClaim {
        label,
        backed_by,
        compiled_in: true,
    };
    let gated = |label, backed_by, compiled_in| FeatureClaim {
        label,
        backed_by,
        compiled_in,
    };

    vec![
        c(
            "AgentExecutor trait (4 implementations)",
            &[
                "sync-jsonrpc-send",
                "build-failure-path",
                "health-orchestration",
                "full-orchestration",
            ],
        ),
        c(
            "RequestHandlerBuilder (all options)",
            &["sync-jsonrpc-send", "timeout-retryable"],
        ),
        c(
            "JsonRpcDispatcher",
            &["sync-jsonrpc-send", "streaming-jsonrpc"],
        ),
        c(
            "RestDispatcher",
            &[
                "sync-rest-send",
                "streaming-rest",
                "get-task-rest",
                "list-tasks-rest",
            ],
        ),
        c(
            "ClientBuilder (JSON-RPC + REST)",
            &["sync-jsonrpc-send", "sync-rest-send", "mixed-transport"],
        ),
        c("Sync SendMessage", &["sync-jsonrpc-send", "sync-rest-send"]),
        c(
            "Streaming SendStreamingMessage",
            &["streaming-jsonrpc", "streaming-rest"],
        ),
        c(
            "EventStream consumer",
            &["stream-completeness", "event-ordering"],
        ),
        c(
            "GetTask",
            &["get-task", "get-task-rest", "get-nonexistent-task"],
        ),
        c(
            "ListTasks (pagination + context + status filters)",
            &[
                "list-tasks",
                "list-tasks-rest",
                "pagination-walk",
                "list-context-filter",
                "list-status-filter",
                "combined-filter",
                "stale-page-token",
            ],
        ),
        c(
            "CancelTask executor override",
            &[
                "cancel-task",
                "cancel-nonexistent",
                "cancel-completed",
                "cancel-already-failed",
                "concurrent-cancels",
            ],
        ),
        c(
            "Push notification config CRUD (JSON-RPC + REST)",
            &[
                "push-config-crud",
                "push-crud-jsonrpc",
                "push-list-regression",
                "push-not-supported",
            ],
        ),
        c(
            "HttpPushSender delivery + event classification",
            &[
                "push-delivery-e2e",
                "push-event-classify",
                "push-global-limit",
                "webhook-url-scheme",
            ],
        ),
        c(
            "Webhook receiver (with snapshot/drain)",
            &["push-delivery-e2e"],
        ),
        c("ServerInterceptor (audit + auth)", &["real-auth-rejection"]),
        c(
            "Custom Metrics observer",
            &[
                "metrics-nonzero",
                "error-metrics-tracked",
                "latency-metrics",
                "queue-depth-metrics",
            ],
        ),
        c(
            "AgentCard discovery (correct URLs via pre-bind)",
            &[
                "agent-card-discovery",
                "agent-card-jsonrpc",
                "card-url-correct",
                "card-skills-valid",
                "card-semantic-valid",
            ],
        ),
        c(
            "Multi-part messages (text + data + file)",
            &["multi-part-message", "file-parts", "empty-parts-rejected"],
        ),
        c(
            "Artifact append mode + multiple artifacts",
            &[
                "multiple-artifacts",
                "artifact-content",
                "include-artifacts",
            ],
        ),
        c(
            "TaskState lifecycle (all states)",
            &["state-transition-order", "executor-error-failed"],
        ),
        c("CancellationToken checking", &["cancel-task"]),
        c("Executor timeout config", &["timeout-retryable"]),
        c("Event queue capacity config", &["backpressure-lagged"]),
        c("Max concurrent streams config", &["concurrent-streams"]),
        c("Agent-to-agent A2A communication", &["agent-to-agent"]),
        c(
            "Multi-level orchestration",
            &["full-orchestration", "health-orchestration"],
        ),
        c("Request metadata", &["message-metadata"]),
        c(
            "SubscribeToTask resubscribe (REST + JSON-RPC)",
            &["resubscribe-rest", "resubscribe-jsonrpc"],
        ),
        c(
            "boxed_future + EventEmitter helpers",
            &["sync-jsonrpc-send"],
        ),
        c(
            "Concurrent streams on same agent",
            &[
                "concurrent-streams",
                "concurrent-requests",
                "high-concurrency",
            ],
        ),
        c("return_immediately mode", &["return-immediately"]),
        c(
            "history_length config",
            &["history-length", "get-task-history"],
        ),
        c(
            "TenantAwareInMemoryTaskStore isolation",
            &["53-tenant-isolation", "55-tenant-count"],
        ),
        c(
            "TenantContext::scope task_local threading",
            &["54-tenant-id-independence"],
        ),
        c(
            "Batch JSON-RPC (single, multi, empty, mixed, streaming rejection)",
            &[
                "batch-single-element",
                "batch-multi-request",
                "batch-empty",
                "batch-mixed",
                "batch-streaming-rejected",
                "batch-subscribe-rejected",
            ],
        ),
        c(
            "Real auth rejection (interceptor short-circuit)",
            &["real-auth-rejection"],
        ),
        c(
            "GetExtendedAgentCard via JSON-RPC (served + §13.3 refusal)",
            &["extended-agent-card", "extended-card-requires-auth"],
        ),
        c(
            "DynamicAgentCardHandler (runtime-generated cards)",
            &["dynamic-agent-card"],
        ),
        c(
            "Agent card HTTP caching (ETag + 304 Not Modified)",
            &["agent-card-caching"],
        ),
        c(
            "Backpressure / lagged event queue (capacity=2)",
            &["backpressure-lagged"],
        ),
        c(
            "State transition validation (streaming)",
            &["state-transition-order"],
        ),
        c(
            "Executor error → Failed propagation",
            &["executor-error-failed"],
        ),
        c(
            "Streaming event completeness verification",
            &["stream-completeness", "event-ordering"],
        ),
        c("Oversized metadata rejection", &["oversized-metadata"]),
        c("Artifact content correctness", &["artifact-content"]),
        c("GetTask history content", &["get-task-history"]),
        c(
            "Rapid sequential request throughput",
            &["rapid-sequential", "large-payload"],
        ),
        c(
            "Cancel terminal-state task",
            &["cancel-completed", "cancel-already-failed"],
        ),
        c("Agent card semantic validation", &["card-semantic-valid"]),
        c(
            "GetTask after streaming (background processor)",
            &["get-after-stream", "stream-with-get-task"],
        ),
        c(
            "Task store durability across requests",
            &["store-durability", "context-continuation"],
        ),
        c(
            "v1.0 wire format (TASK_STATE_*, flat Part oneof, tagged result, AIP-193 errors)",
            &[
                "wire-task-state-format",
                "wire-part-flat-oneof",
                "wire-response-tagged",
                "wire-aip193-error",
            ],
        ),
        gated(
            "Axum A2aRouter (send + stream + card discovery)",
            &["axum-send-message", "axum-streaming", "axum-agent-card"],
            cfg!(feature = "axum"),
        ),
        gated(
            "SqliteTaskStore (send→get→list persistence)",
            &["sqlite-task-store"],
            cfg!(feature = "sqlite"),
        ),
        gated(
            "SqlitePushConfigStore (set→list→delete lifecycle)",
            &["sqlite-push-config"],
            cfg!(feature = "sqlite"),
        ),
        gated(
            "Axum + SQLite combined production stack",
            &["axum-sqlite-combo"],
            cfg!(all(feature = "axum", feature = "sqlite")),
        ),
        gated(
            "WebSocket transport (SendMessage + streaming)",
            &["51-ws-send-message", "52-ws-streaming"],
            cfg!(feature = "websocket"),
        ),
        gated(
            "gRPC transport (SendMessage + streaming + GetTask)",
            &[
                "56-grpc-send-message",
                "57-grpc-streaming",
                "58-grpc-get-task",
            ],
            cfg!(feature = "grpc"),
        ),
        gated(
            "Agent card signing (JWS ES256, sign + verify + tamper detection)",
            &["79_signing_e2e"],
            cfg!(feature = "signing"),
        ),
        gated(
            "OpenTelemetry metrics (OtelMetrics with noop provider)",
            &["80_otel_metrics"],
            cfg!(feature = "otel"),
        ),
    ]
}

/// One human-readable capability claim, and the tests that evidence it.
pub struct FeatureClaim {
    /// What the summary line says.
    pub label: &'static str,
    /// Names of tests whose passing makes the claim true. Never empty — see
    /// [`audit`], which rejects an unbacked claim.
    pub backed_by: &'static [&'static str],
    /// `false` when this claim's tests are compiled out by a disabled Cargo
    /// feature. Such a claim renders `[ ] NOT RUN` instead of being hidden.
    pub compiled_in: bool,
}

/// Verdict for a single claim.
#[derive(PartialEq, Eq)]
pub enum ClaimStatus {
    /// Every backing test ran and passed.
    Proven,
    /// At least one backing test ran and failed.
    Failed,
    /// Backing tests were not compiled in.
    NotRun,
}

impl ClaimStatus {
    /// The marker printed in the summary.
    pub const fn marker(&self) -> &'static str {
        match self {
            Self::Proven => "[x]",
            Self::Failed => "[FAIL]",
            Self::NotRun => "[ ] NOT RUN —",
        }
    }
}

/// Scores one claim against the results of the run.
pub fn status_of(claim: &FeatureClaim, by_name: &HashMap<&str, bool>) -> ClaimStatus {
    if !claim.compiled_in {
        return ClaimStatus::NotRun;
    }
    let mut saw_any = false;
    for name in claim.backed_by {
        match by_name.get(name) {
            Some(true) => saw_any = true,
            Some(false) => return ClaimStatus::Failed,
            None => {}
        }
    }
    if saw_any {
        ClaimStatus::Proven
    } else {
        ClaimStatus::NotRun
    }
}

/// Problems found by [`audit`], each of which fails the run.
pub struct AuditReport {
    /// Claims naming no test that ran, while being compiled in.
    pub unbacked_claims: Vec<&'static str>,
    /// Claims naming a test that does not exist in this build at all.
    pub unknown_tests: Vec<(&'static str, &'static str)>,
    /// Tests that ran but which no claim mentions.
    pub unclaimed_tests: Vec<String>,
}

impl AuditReport {
    /// `true` when the claim table faithfully describes the run.
    pub fn is_clean(&self) -> bool {
        self.unbacked_claims.is_empty()
            && self.unknown_tests.is_empty()
            && self.unclaimed_tests.is_empty()
    }
}

/// Cross-checks the claim table against the results, in both directions.
pub fn audit(claims: &[FeatureClaim], results: &[TestResult]) -> AuditReport {
    let ran: BTreeSet<&str> = results.iter().map(|r| r.name.as_str()).collect();

    let mut unbacked_claims = Vec::new();
    let mut unknown_tests = Vec::new();
    for claim in claims {
        let any_ran = claim.backed_by.iter().any(|n| ran.contains(n));
        if claim.compiled_in && !any_ran {
            unbacked_claims.push(claim.label);
        }
        // A claim naming a test nobody registered is a typo or a deletion; it
        // would otherwise silently weaken the claim's evidence to nothing.
        // Only meaningful for compiled-in claims — a gated-off claim's tests
        // legitimately do not exist in this build.
        if claim.compiled_in {
            for name in claim.backed_by {
                if !ran.contains(name) {
                    unknown_tests.push((claim.label, *name));
                }
            }
        }
    }

    let claimed: BTreeSet<&str> = claims
        .iter()
        .flat_map(|c| c.backed_by.iter().copied())
        .collect();
    let unclaimed_tests: Vec<String> = ran
        .iter()
        .filter(|n| !claimed.contains(*n))
        .map(|n| (*n).to_owned())
        .collect();

    AuditReport {
        unbacked_claims,
        unknown_tests,
        unclaimed_tests,
    }
}
