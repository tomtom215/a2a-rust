// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! TCK test runner — executes all conformance tests against a target server.

use crate::tests;

/// Every binding name the runner knows how to drive.
///
/// The single source of truth shared with `parse_args` in `main`, so a
/// binding accepted on the command line and a binding named in a [`Scope`]
/// cannot drift apart.
pub const BINDINGS: &[&str] = &["jsonrpc", "rest", "websocket"];

/// Which bindings a check is meaningful for.
///
/// A check that cannot apply to a binding is reported `N/A` **with its
/// reason** — never silently `Ok`. A silent pass is indistinguishable from a
/// check that ran and found nothing wrong, which is the same defect as a gate
/// that cannot fail: the reader sees green and believes something was
/// verified.
#[derive(Clone, Copy)]
pub enum Scope {
    /// Applies to every binding in [`BINDINGS`].
    All,
    /// Applies only to `bindings`; `why` explains the exclusion and is
    /// printed verbatim next to the `N/A`.
    Only {
        bindings: &'static [&'static str],
        why: &'static str,
    },
}

impl Scope {
    /// Whether this check should run against `binding`.
    pub fn covers(self, binding: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only { bindings, .. } => bindings.contains(&binding),
        }
    }

    /// The reason this check does not apply, for the bindings it excludes.
    pub fn why(self) -> &'static str {
        match self {
            Self::All => "",
            Self::Only { why, .. } => why,
        }
    }
}

/// Checks that ride a JSON-RPC envelope.
///
/// §11's HTTP+JSON binding carries bare resources over REST verbs, so there
/// is no envelope to inspect; §12's WebSocket binding carries the identical
/// envelope the §9 JSON-RPC binding does, only over a socket.
const ENVELOPE_ONLY: Scope = Scope::Only {
    bindings: &["jsonrpc", "websocket"],
    why: "the HTTP+JSON binding carries bare resources, not JSON-RPC envelopes (§11)",
};

/// Checks that assert something about an HTTP request body.
///
/// A §12 WebSocket request is a text frame: the frame header has no
/// `Content-Type`, and the only HTTP message in the exchange is the upgrade
/// handshake, which has no body. There is nothing to negotiate a media type
/// on, so the check is not applicable rather than passing or failing.
const HTTP_BODY_ONLY: Scope = Scope::Only {
    bindings: &["jsonrpc", "rest"],
    why: "a §12 text frame carries no Content-Type; the media type is an HTTP-body concern",
};

/// Result of a single conformance test.
pub struct TestResult {
    /// Test name (e.g., "agent_card_discovery").
    pub name: String,
    /// What the check concluded.
    pub outcome: Outcome,
    /// Human-readable message (error details on failure, "ok" on success,
    /// the exclusion reason when not applicable).
    pub message: String,
}

/// What a check concluded.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The check ran and the target conformed.
    Pass,
    /// The check ran and the target did not conform.
    Fail,
    /// The check does not apply to this binding; nothing was verified.
    NotApplicable,
}

impl TestResult {
    pub fn pass(name: &str) -> Self {
        Self {
            name: name.to_string(),
            outcome: Outcome::Pass,
            message: "ok".to_string(),
        }
    }

    pub fn fail(name: &str, msg: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            outcome: Outcome::Fail,
            message: msg.into(),
        }
    }

    pub fn not_applicable(name: &str, why: &str) -> Self {
        Self {
            name: name.to_string(),
            outcome: Outcome::NotApplicable,
            message: why.to_string(),
        }
    }

    /// Whether the check ran at all. A not-applicable check is *not* graded —
    /// counting it as a pass would inflate the score with work never done.
    pub fn graded(&self) -> bool {
        self.outcome != Outcome::NotApplicable
    }

    pub fn passed(&self) -> bool {
        self.outcome == Outcome::Pass
    }
}

/// Runs all TCK conformance tests against the given server.
///
/// `card_url` is always the agent's HTTP origin: §5 discovery is served over
/// HTTPS regardless of which binding carries the RPCs, so the card checks do
/// not move when the binding does. `rpc_url` is the endpoint the selected
/// binding speaks to — the same origin for `jsonrpc`/`rest`, and the
/// `ws(s)://` endpoint for `websocket`.
pub async fn run_all(card_url: &str, rpc_url: &str, binding: &str) -> Vec<TestResult> {
    let mut results = Vec::new();

    // ── Agent Card Discovery ──────────────────────────────────────────────
    run_test(
        &mut results,
        "agent_card_discovery",
        Scope::All,
        binding,
        async { tests::agent_card::test_agent_card_discovery(card_url).await },
    )
    .await;

    run_test(
        &mut results,
        "agent_card_required_fields",
        Scope::All,
        binding,
        async { tests::agent_card::test_agent_card_required_fields(card_url).await },
    )
    .await;

    run_test(
        &mut results,
        "agent_card_content_type",
        Scope::All,
        binding,
        async { tests::agent_card::test_agent_card_content_type(card_url).await },
    )
    .await;

    // ── SendMessage ───────────────────────────────────────────────────────
    run_test(
        &mut results,
        "send_message_basic",
        Scope::All,
        binding,
        async { tests::messaging::test_send_message_basic(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "send_message_returns_task",
        Scope::All,
        binding,
        async { tests::messaging::test_send_message_returns_task(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "send_message_context_id",
        Scope::All,
        binding,
        async { tests::messaging::test_send_message_context_id(rpc_url, binding).await },
    )
    .await;

    // ── GetTask ───────────────────────────────────────────────────────────
    run_test(
        &mut results,
        "get_task_existing",
        Scope::All,
        binding,
        async { tests::task_ops::test_get_task_existing(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "get_task_not_found",
        Scope::All,
        binding,
        async { tests::task_ops::test_get_task_not_found(rpc_url, binding).await },
    )
    .await;

    // ── ListTasks ─────────────────────────────────────────────────────────
    run_test(
        &mut results,
        "list_tasks_basic",
        Scope::All,
        binding,
        async { tests::task_ops::test_list_tasks_basic(rpc_url, binding).await },
    )
    .await;

    // ── CancelTask ────────────────────────────────────────────────────────
    run_test(&mut results, "cancel_task", Scope::All, binding, async {
        tests::task_ops::test_cancel_task(rpc_url, binding).await
    })
    .await;

    // ── Streaming ─────────────────────────────────────────────────────────
    run_test(
        &mut results,
        "streaming_send_message",
        Scope::All,
        binding,
        async { tests::streaming::test_streaming_send_message(rpc_url, binding).await },
    )
    .await;

    // ── Push Notification Config ──────────────────────────────────────────
    run_test(
        &mut results,
        "push_config_create",
        Scope::All,
        binding,
        async { tests::push_config::test_create_push_config(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "push_config_get",
        Scope::All,
        binding,
        async { tests::push_config::test_get_push_config(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "push_config_list",
        Scope::All,
        binding,
        async { tests::push_config::test_list_push_configs(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "push_config_delete",
        Scope::All,
        binding,
        async { tests::push_config::test_delete_push_config(rpc_url, binding).await },
    )
    .await;

    // ── Error Handling ────────────────────────────────────────────────────
    run_test(
        &mut results,
        "invalid_method_returns_error",
        Scope::All,
        binding,
        async { tests::errors::test_invalid_method_returns_error(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "invalid_params_returns_error",
        Scope::All,
        binding,
        async { tests::errors::test_invalid_params_returns_error(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "get_unknown_task_returns_error",
        Scope::All,
        binding,
        async { tests::errors::test_get_unknown_task_returns_error(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "malformed_body_returns_error",
        Scope::All,
        binding,
        async { tests::errors::test_malformed_body_returns_error(rpc_url, binding).await },
    )
    .await;

    // ── Wire Format ───────────────────────────────────────────────────────
    run_test(
        &mut results,
        "jsonrpc_envelope_format",
        ENVELOPE_ONLY,
        binding,
        async { tests::wire_format::test_jsonrpc_envelope_format(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "task_state_values",
        Scope::All,
        binding,
        async { tests::wire_format::test_task_state_values(rpc_url, binding).await },
    )
    .await;

    run_test(
        &mut results,
        "a2a_media_type_accepted",
        HTTP_BODY_ONLY,
        binding,
        async { tests::wire_format::test_a2a_media_type_accepted(rpc_url, binding).await },
    )
    .await;

    results
}

async fn run_test<F>(
    results: &mut Vec<TestResult>,
    name: &str,
    scope: Scope,
    binding: &str,
    test: F,
) where
    F: std::future::Future<Output = Result<(), String>>,
{
    if !scope.covers(binding) {
        results.push(TestResult::not_applicable(name, scope.why()));
        println!("  [N/A ] {name} — {}", scope.why());
        return;
    }
    let status_icon = match test.await {
        Ok(()) => {
            results.push(TestResult::pass(name));
            "PASS"
        }
        Err(msg) => {
            results.push(TestResult::fail(name, &msg));
            "FAIL"
        }
    };
    println!("  [{status_icon}] {name}");
}

#[cfg(test)]
mod tests_runner {
    use super::{Scope, BINDINGS, ENVELOPE_ONLY, HTTP_BODY_ONLY};

    /// Every `Scope` in the runner, so the drift guard below sees all of them.
    const ALL_SCOPES: &[(&str, Scope)] = &[
        ("ENVELOPE_ONLY", ENVELOPE_ONLY),
        ("HTTP_BODY_ONLY", HTTP_BODY_ONLY),
    ];

    /// A `Scope` naming a binding the CLI does not accept excludes that check
    /// from *every* run while looking like it covers something — the typo
    /// `"websockets"` would silently drop a check from all three legs. Anchor
    /// the names to [`BINDINGS`], which `parse_args` also validates against.
    #[test]
    fn every_scope_names_only_known_bindings() {
        for (label, scope) in ALL_SCOPES {
            let Scope::Only { bindings, .. } = scope else {
                continue;
            };
            for b in *bindings {
                assert!(
                    BINDINGS.contains(b),
                    "{label} names unknown binding {b:?}; known: {BINDINGS:?}"
                );
            }
        }
    }

    /// A narrowed `Scope` that excludes nothing is a scope that does nothing;
    /// a scope that excludes everything deletes the check. Either is a bug
    /// worth catching at compile-test time rather than in a green CI run.
    #[test]
    fn every_narrowed_scope_excludes_something_but_not_everything() {
        for (label, scope) in ALL_SCOPES {
            let covered = BINDINGS.iter().filter(|b| scope.covers(b)).count();
            assert!(
                covered > 0,
                "{label} covers no binding — the check can never run"
            );
            assert!(
                covered < BINDINGS.len(),
                "{label} covers every binding — use Scope::All instead"
            );
        }
    }

    /// The reason is printed next to `N/A` and is the only thing telling a
    /// reader why nothing was verified. An empty one turns the report into
    /// an unexplained gap.
    #[test]
    fn every_narrowed_scope_explains_itself() {
        for (label, scope) in ALL_SCOPES {
            assert!(
                !scope.why().trim().is_empty(),
                "{label} excludes a binding without saying why"
            );
        }
    }

    #[test]
    fn scope_all_covers_every_binding() {
        for b in BINDINGS {
            assert!(Scope::All.covers(b), "Scope::All must cover {b}");
        }
    }

    #[test]
    fn scope_only_rejects_bindings_it_does_not_name() {
        assert!(ENVELOPE_ONLY.covers("jsonrpc"));
        assert!(ENVELOPE_ONLY.covers("websocket"));
        assert!(!ENVELOPE_ONLY.covers("rest"));
        assert!(HTTP_BODY_ONLY.covers("rest"));
        assert!(!HTTP_BODY_ONLY.covers("websocket"));
    }
}
