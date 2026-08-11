// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Act 5 — the parts of a deployment that are not the protocol.
//!
//! # Why this act exists
//!
//! Acts 1-3 answer "what is an agent". Act 4 answers "does every method work on
//! every binding". Neither asks the question an operator asks first: *what
//! happens when this is exposed to more than one caller?*
//!
//! The SDK ships tenant isolation, authentication interceptors, rate limiting,
//! a metrics hook, persistent stores, agent-card signing, OpenTelemetry export
//! and graceful shutdown. Until 2026-08-11 **no example in this repository
//! demonstrated any of them end-to-end over a socket**. They were covered by
//! unit and integration tests, which is not the same thing: a reader evaluating
//! the SDK reads the examples, and the examples showed an in-memory,
//! single-tenant, unauthenticated agent.
//!
//! # Each check asserts, it does not narrate
//!
//! Every capability here is exercised *and* checked, with the specific wrong
//! answer named in the failure message. A demonstration that only prints what
//! happened is a demonstration that cannot fail — the defect this repository
//! has spent its time removing. Tenant isolation is checked by *absence*
//! (tenant A must not see tenant B's task) and by *refusal* (a caller
//! authenticated as one tenant must be rejected when it names another), not by
//! printing two task ids and moving on.
//!
//! Capabilities behind Cargo features report `NOT BUILT` rather than vanishing,
//! for the same reason: a narrowed build must look narrower, not identical.

mod access;
mod durability;
mod observability;
mod tenancy;
mod trust;

use std::sync::Arc;

use a2a_protocol_client::interceptor::{CallInterceptor, ClientRequest, ClientResponse};
use a2a_protocol_client::{ClientError, ClientResult};
use a2a_protocol_server::handler::RequestHandler;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard};

use crate::serving::make_card;

/// One hardening capability and what happened to it.
pub struct Check {
    /// What is being demonstrated.
    pub label: &'static str,
    /// The verdict, with detail.
    pub outcome: Outcome,
}

/// Verdict for a single capability.
pub enum Outcome {
    /// Exercised and correct.
    Pass(String),
    /// Exercised and wrong — the detail names the wrong answer.
    Fail(String),
    /// Behind a Cargo feature that is off in this build.
    ///
    /// Unconstructed in the default build, where every feature is on. It exists
    /// so `--no-default-features` reports a narrower run as narrower instead of
    /// silently printing the same "all passed" line over fewer checks.
    #[allow(dead_code)]
    NotCompiled(&'static str),
}

impl Check {
    fn pass(label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            label,
            outcome: Outcome::Pass(detail.into()),
        }
    }

    fn fail(label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            label,
            outcome: Outcome::Fail(detail.into()),
        }
    }

    /// A capability compiled out of this build.
    ///
    /// Only reachable from the `#[cfg(not(feature = ...))]` arms, so the
    /// default build — which turns every feature on — never calls it.
    #[allow(dead_code)]
    const fn skipped(label: &'static str, feature: &'static str) -> Self {
        Self {
            label,
            outcome: Outcome::NotCompiled(feature),
        }
    }

    /// `true` when this check found something wrong.
    const fn failed(&self) -> bool {
        matches!(self.outcome, Outcome::Fail(_))
    }

    /// `true` when the capability was not built into this binary.
    const fn not_compiled(&self) -> bool {
        matches!(self.outcome, Outcome::NotCompiled(_))
    }
}

// ── Shared scaffolding ───────────────────────────────────────────────────────

/// Binds an ephemeral loopback port.
async fn bind() -> (tokio::net::TcpListener, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("binding a loopback port");
    let addr = listener.local_addr().expect("local_addr");
    (listener, format!("http://{addr}"))
}

/// Serves `handler` over JSON-RPC on `listener` for the life of the process.
fn serve(listener: tokio::net::TcpListener, handler: Arc<RequestHandler>) {
    let dispatcher = Arc::new(a2a_protocol_server::dispatch::JsonRpcDispatcher::new(
        handler,
    ));
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            let dispatcher = Arc::clone(&dispatcher);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let dispatcher = Arc::clone(&dispatcher);
                    async move { Ok::<_, std::convert::Infallible>(dispatcher.dispatch(req).await) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await;
            });
        }
    });
}

/// `true` when the server refused, rather than the example failing to reach it.
///
/// Three checks here assert that something *must not* succeed, and every one of
/// them would otherwise pass on a connection error — which is the vacuous-pass
/// shape this repository has spent its time removing. A refusal check that a
/// dead server satisfies is not a refusal check: kill the agent between the
/// setup and the assertion and it still reports "correctly refused".
///
/// The list is deliberately positive rather than a list of transport errors to
/// exclude. `ClientError` is `#[non_exhaustive]`, so a variant added upstream
/// falls through to `false` here and surfaces as a failing check that names the
/// error, instead of being silently counted as a refusal.
fn is_refusal(error: &ClientError) -> bool {
    matches!(
        error,
        ClientError::Protocol(_)
            | ClientError::AuthRequired { .. }
            | ClientError::UnexpectedStatus { .. }
    )
}

/// A minimal single-binding card — Act 4 already measures the full matrix, so
/// these agents advertise only what they actually serve.
fn plain_card(url: &str, name: &str) -> AgentCard {
    let mut card = make_card(url, "", "", name, "hardening demo", "harden");
    card.capabilities = AgentCapabilities::none();
    card.supported_interfaces.truncate(1);
    card
}

/// Injects a fixed header on every outbound call.
///
/// This is the client half of header-derived multi-tenancy: the SDK's
/// [`CallInterceptor`] hook exists so a caller can attach the credentials or
/// routing headers its deployment uses without the SDK having to know about
/// them. In production the value comes from a gateway or an auth library; here
/// it is a literal so the two tenants are visibly distinct.
struct HeaderInterceptor {
    name: &'static str,
    value: String,
}

impl HeaderInterceptor {
    fn new(name: &'static str, value: impl Into<String>) -> Self {
        Self {
            name,
            value: value.into(),
        }
    }
}

impl CallInterceptor for HeaderInterceptor {
    #[allow(clippy::manual_async_fn)]
    fn before<'a>(
        &'a self,
        req: &'a mut ClientRequest,
    ) -> impl std::future::Future<Output = ClientResult<()>> + Send + 'a {
        async move {
            req.extra_headers
                .insert(self.name.to_owned(), self.value.clone());
            Ok(())
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn after<'a>(
        &'a self,
        _resp: &'a ClientResponse,
    ) -> impl std::future::Future<Output = ClientResult<()>> + Send + 'a {
        async move { Ok(()) }
    }
}

// ── Runner ───────────────────────────────────────────────────────────────────

/// Runs every hardening check, in order.
///
/// The checks are independent — each builds its own handler on its own port —
/// so a failure in one does not mask the rest. They run sequentially rather
/// than concurrently because the OpenTelemetry check installs a process-global
/// meter provider, and interleaving it with the others would make the
/// datapoint it collects depend on scheduling.
pub async fn run() -> Vec<Check> {
    vec![
        tenancy::isolation().await,
        access::bearer_auth().await,
        access::rate_limiting().await,
        trust::card_signing(),
        durability::sqlite_persistence().await,
        durability::graceful_shutdown().await,
        observability::metrics_hook().await,
        observability::otel_export().await,
    ]
}

/// Prints the checks and returns how many failed.
pub fn report(checks: &[Check]) -> usize {
    for check in checks {
        match &check.outcome {
            Outcome::Pass(detail) => println!("  [ok]        {:<52} {detail}", check.label),
            Outcome::Fail(detail) => println!("  [FAIL]      {:<52} {detail}", check.label),
            Outcome::NotCompiled(feature) => println!(
                "  [NOT BUILT] {:<52} needs --features {feature}",
                check.label
            ),
        }
    }
    let failed = checks.iter().filter(|c| c.failed()).count();
    let not_built = checks.iter().filter(|c| c.not_compiled()).count();
    println!();
    println!(
        "  {} passed, {failed} failed, {not_built} not compiled into this build",
        checks.len() - failed - not_built
    );
    if not_built > 0 {
        println!("  Rerun with --all-features to exercise every capability.");
    }
    failed
}
