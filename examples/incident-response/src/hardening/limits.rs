// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Bounds on what one caller can make the handler allocate.

use std::sync::Arc;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::handler::HandlerLimits;
use a2a_protocol_types::task::ContextId;

use super::{bind, is_refusal, plain_card, serve, Check};
use crate::agents::LogSearchExecutor;
use crate::{send_params, user_message};

const LABEL: &str = "Handler limits (oversized ids refused, normal ones pass)";

/// A client-supplied identifier longer than the configured bound must be
/// refused, and one within it must not be.
///
/// `context_id` is chosen because it is the one an untrusted caller fully
/// controls: it arrives on the message, is stored, and is used as a map key.
/// Without a bound, a caller mints arbitrarily long keys and the server
/// allocates them — which is why [`HandlerLimits::max_id_length`] exists and
/// why an example that never sets it leaves a reader assuming there is no
/// such control.
///
/// Both directions are asserted. A handler that rejected every `context_id`
/// would satisfy a check that only sent the oversized one, and "nothing works"
/// is not the property being demonstrated.
pub(super) async fn handler_limits() -> Check {
    const MAX_ID: usize = 32;

    let (listener, url) = bind().await;
    let handler = match RequestHandlerBuilder::new(LogSearchExecutor)
        .with_agent_card(plain_card(&url, "Bounded Agent"))
        .with_handler_limits(HandlerLimits::default().with_max_id_length(MAX_ID))
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

    // Within the bound — must be accepted.
    let mut ok = user_message("payments-api");
    ok.context_id = Some(ContextId::new("c".repeat(MAX_ID)));
    if let Err(e) = client.send_message(send_params(ok)).await {
        return Check::fail(
            LABEL,
            format!("a context_id of exactly {MAX_ID} chars was refused: {e}"),
        );
    }

    // One character over — must be refused, by the server rather than by the
    // connection failing.
    let oversized = MAX_ID + 1;
    let mut too_long = user_message("payments-api");
    too_long.context_id = Some(ContextId::new("c".repeat(oversized)));
    match client.send_message(send_params(too_long)).await {
        Ok(_) => Check::fail(
            LABEL,
            format!(
                "a {oversized}-char context_id was accepted with max_id_length {MAX_ID} \
                 — the bound is not enforced"
            ),
        ),
        Err(e) if !is_refusal(&e) => Check::fail(
            LABEL,
            format!("the oversized call never reached the server, so nothing refused it: {e}"),
        ),
        Err(_) => Check::pass(
            LABEL,
            format!("max_id_length {MAX_ID}: {MAX_ID} chars accepted, {oversized} refused"),
        ),
    }
}
