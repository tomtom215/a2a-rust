// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The Rust worker: the same echo agent the four `itk/agents/` workers are,
//! written with this SDK.
//!
//! The coordinator in `main.rs` shows the *client* half of the SDK — building
//! clients, fanning out, parsing replies. Until this module existed the
//! example had no *server* half a reader could run: every worker was in
//! another language, so "what does a worker look like in Rust" had no answer
//! in the package that most needed one.
//!
//! It is deliberately the smallest agent that satisfies the coordinator's
//! contract, which `call_worker` fixes as: a `SendMessage` reply that is a
//! task whose first artifact's text is the answer. The other workers reply
//! `[Python Echo] <text>`, `[JS Echo] <text>`, and so on; this one replies
//! `[Rust Echo] <text>`, so the coordinator renders it the same way without
//! knowing which language answered.
//!
//! Shared by two targets: the coordinator binary compiles it under
//! `#[cfg(test)]` so the fan-out can be tested against a real worker
//! in-process, and `src/bin/rust-worker.rs` compiles it as the binary a reader
//! starts next to the other four.

use std::net::SocketAddr;
use std::sync::Arc;

use a2a_protocol_server::agent_executor;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::executor_helpers::EventEmitter;
use a2a_protocol_server::handler::RequestHandler;
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::TaskState;

/// Where the binary listens unless `RUST_WORKER_ADDR` says otherwise.
///
/// Port 9104 follows the other four (9100–9103); the coordinator's worker
/// table dials this address, and a test asserts the two agree.
pub const DEFAULT_ADDR: &str = "127.0.0.1:9104";

/// The prefix every reply carries, in the `[<Language> Echo] ` form the
/// `itk/agents/` workers use.
pub const REPLY_PREFIX: &str = "[Rust Echo] ";

/// The worker. It has no state: the reply depends only on the message.
pub struct RustWorker;

agent_executor!(RustWorker, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;

    // Every text part, in order, the way the coordinator's own `extract_text`
    // reads a reply — a message with no text part echoes an empty string
    // rather than failing, which is what the other workers do.
    let text = ctx.message.texts().collect::<Vec<_>>().join(" ");
    let reply = Part::text(format!("{REPLY_PREFIX}{text}"));

    emit.artifact("echo", vec![reply], None, Some(true)).await?;
    emit.status(TaskState::Completed).await?;
    Ok(())
});

/// The card the coordinator's startup probe fetches from
/// `/.well-known/agent-card.json`.
///
/// Same shape as the other workers' cards — one `echo` skill, text in and
/// out — with one honest difference: they advertise push notifications and
/// this one does not, because no push store is configured here and a card
/// should not claim what the agent behind it refuses.
pub fn make_worker_card(url: &str) -> AgentCard {
    AgentCard {
        url: None,
        name: "Rust Echo Agent".into(),
        description: "A2A echo worker agent implemented in Rust".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: a2a_protocol_types::A2A_VERSION.into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "echo".into(),
            name: "Echo".into(),
            description: "Echoes the input message back".into(),
            tags: vec!["echo".into(), "test".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none().with_streaming(true),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

/// Serves the worker over JSON-RPC on `addr` and returns the bound address.
///
/// Returns as soon as the listener is up; the accept loop runs on a spawned
/// task, so the caller decides how long the worker lives (the binary waits
/// for Ctrl+C, a test drops the runtime).
///
/// `addr` may name port 0. The card has to carry the port that was actually
/// bound, and the handler that serves the card is built before the socket
/// is, so the port is learned with a probe bind first — the same trick
/// `surface.rs` uses for the WebSocket listener.
///
/// # Errors
///
/// The address cannot be bound, or the handler refuses its configuration.
pub async fn start(addr: &str) -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let probe = tokio::net::TcpListener::bind(addr).await?;
    let addr = probe.local_addr()?;
    drop(probe);

    let handler: Arc<RequestHandler> = Arc::new(
        RequestHandlerBuilder::new(RustWorker)
            .with_agent_card(make_worker_card(&format!("http://{addr}")))
            .build()?,
    );
    Ok(serve_with_addr(addr, JsonRpcDispatcher::new(handler)).await?)
}
