// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Who may call, and how often.

use std::sync::Arc;

use a2a_protocol_client::{
    AuthInterceptor, ClientBuilder, CredentialsStore, InMemoryCredentialsStore, SessionId,
};
use a2a_protocol_server::auth::BearerTokenAuthInterceptor;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::rate_limit::{RateLimitConfig, RateLimitInterceptor};

use super::{bind, is_refusal, plain_card, serve, Check};
use crate::agents::LogSearchExecutor;
use crate::{send_params, user_message};

// ── Bearer-token authentication ──────────────────────────────────────────────

/// An authenticated agent must refuse an anonymous caller *and* accept a valid
/// one.
///
/// Both halves matter. A server that refused every request would pass a check
/// that only asserted the refusal, and that failure mode is not hypothetical:
/// an interceptor with an inverted comparison, or one that rejects before
/// reading the header, looks exactly like a working one from the anonymous
/// side alone.
///
/// The client side deliberately uses the SDK's own [`AuthInterceptor`] and
/// [`InMemoryCredentialsStore`] rather than setting an `Authorization` header
/// by hand, so this is a demonstration of the shipped API rather than of HTTP.
pub(super) async fn bearer_auth() -> Check {
    const LABEL: &str = "Bearer-token auth (anonymous refused, token accepted)";
    const TOKEN: &str = "s3cret-incident-token";

    let (listener, url) = bind().await;
    let handler = match RequestHandlerBuilder::new(LogSearchExecutor)
        .with_agent_card(plain_card(&url, "Secured Agent"))
        .with_interceptor(BearerTokenAuthInterceptor::new([TOKEN.to_owned()]))
        .build()
    {
        Ok(handler) => Arc::new(handler),
        Err(e) => return Check::fail(LABEL, format!("building the handler: {e}")),
    };
    serve(listener, handler);

    // No credentials — must be refused, and refused *by the server*: a
    // connection error here would otherwise satisfy the assertion.
    let anonymous = match ClientBuilder::new(&url).build() {
        Ok(client) => client,
        Err(e) => return Check::fail(LABEL, format!("building the anonymous client: {e}")),
    };
    match anonymous
        .send_message(send_params(user_message("payments-api")))
        .await
    {
        Ok(response) => {
            return Check::fail(
                LABEL,
                format!(
                    "an unauthenticated request SUCCEEDED ({response:?}) — auth is not enforced"
                ),
            )
        }
        Err(e) if !is_refusal(&e) => {
            return Check::fail(
                LABEL,
                format!("the anonymous call never reached the server, so nothing refused it: {e}"),
            )
        }
        Err(_) => {}
    }

    // With the token — must be accepted.
    let store = Arc::new(InMemoryCredentialsStore::new());
    let session = SessionId::from("incident-response");
    store.set(session.clone(), "bearer", TOKEN.to_owned());
    let authenticated = match ClientBuilder::new(&url)
        .with_interceptor(AuthInterceptor::new(store, session))
        .build()
    {
        Ok(client) => client,
        Err(e) => return Check::fail(LABEL, format!("building the authenticated client: {e}")),
    };
    match authenticated
        .send_message(send_params(user_message("payments-api")))
        .await
    {
        Ok(_) => Check::pass(LABEL, "anonymous refused, bearer token accepted"),
        Err(e) => Check::fail(
            LABEL,
            format!("a correctly authenticated request was refused: {e}"),
        ),
    }
}

// ── Rate limiting ────────────────────────────────────────────────────────────

/// Traffic past the configured limit must be refused, and traffic under it must
/// not be.
///
/// The window is deliberately long (60s) and the limit small, so the outcome
/// does not depend on how fast the machine running this is: all `LIMIT + 2`
/// calls land inside one window on any hardware.
pub(super) async fn rate_limiting() -> Check {
    const LABEL: &str = "Rate limiting (over-limit calls refused, under-limit pass)";
    const LIMIT: u64 = 3;
    const ATTEMPTS: u64 = LIMIT + 2;

    let (listener, url) = bind().await;
    let limiter = match RateLimitInterceptor::new(RateLimitConfig {
        requests_per_window: LIMIT,
        window_secs: 60,
        ..Default::default()
    }) {
        Ok(limiter) => limiter,
        Err(e) => return Check::fail(LABEL, format!("building the limiter: {e}")),
    };

    let handler = match RequestHandlerBuilder::new(LogSearchExecutor)
        .with_agent_card(plain_card(&url, "Limited Agent"))
        .with_interceptor(limiter)
        .build()
    {
        Ok(handler) => Arc::new(handler),
        Err(e) => return Check::fail(LABEL, format!("building the handler: {e}")),
    };
    serve(listener, handler);

    let client = match ClientBuilder::new(&url).build() {
        Ok(client) => client,
        Err(e) => return Check::fail(LABEL, format!("building the client: {e}")),
    };

    let mut accepted = 0_u64;
    let mut refused = 0_u64;
    for n in 0..ATTEMPTS {
        match client
            .send_message(send_params(user_message("payments-api")))
            .await
        {
            Ok(_) => accepted += 1,
            Err(e) if is_refusal(&e) => refused += 1,
            // Counting a connection error as a refusal would let a dead agent
            // satisfy the limit, which is the opposite of what is being shown.
            Err(e) => {
                return Check::fail(
                    LABEL,
                    format!("call {} of {ATTEMPTS} never reached the server: {e}", n + 1),
                )
            }
        }
    }

    if refused == 0 {
        return Check::fail(
            LABEL,
            format!("all {ATTEMPTS} calls were accepted with a limit of {LIMIT} — not enforcing"),
        );
    }
    if accepted == 0 {
        return Check::fail(
            LABEL,
            format!("all {ATTEMPTS} calls were refused — the limiter passes no traffic at all"),
        );
    }
    if accepted > LIMIT {
        return Check::fail(
            LABEL,
            format!("{accepted} calls were accepted with a limit of {LIMIT} — the limit is loose"),
        );
    }
    Check::pass(
        LABEL,
        format!("limit {LIMIT}/60s: {accepted} accepted, {refused} refused"),
    )
}
