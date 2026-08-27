// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! End-to-end A2A echo agent example.
//!
//! Serves all four bindings — JSON-RPC (§9), HTTP+JSON (§11), gRPC (§10) and
//! WebSocket (§12 custom) — behind one handler, then drives **every** A2A
//! service method over **every** binding and prints the resulting coverage
//! matrix.
//!
//! # Why the matrix exists
//!
//! `examples/README.md` used to describe this example as demonstrating "the
//! complete request lifecycle". Measured on 2026-08-11 it drove 4 of the 11
//! methods over 2 of the 4 transports, and its card advertised neither push
//! notifications nor an extended card — so seven methods were not merely
//! undriven, they were unavailable on the server it started.
//!
//! Nobody wrote that sentence dishonestly. It was a claim with nothing
//! checking it, which becomes the same thing given time. So the claim is now a
//! computation: each call records itself, and the process exits non-zero if
//! any cell of the matrix that should have been exercised was not.
//!
//! The rows come from `a2a_protocol_types::method::Method::ALL`, which is
//! asserted equal to `service A2AService` in the ratified
//! `proto/a2a_v1/a2a.proto` and cross-checked against the official
//! `a2aproject/a2a-tck` suite in CI. The denominator is therefore the
//! specification's, not this example's.
//!
//! # Beyond the happy path
//!
//! A full matrix only shows the server says yes to everything it should. A
//! second agent, advertising no optional capabilities, is started so the
//! counter-tests can check it says *no* where the spec requires — see
//! [`counter`].
//!
//! Run with: `cargo run -p echo-agent`
//!
//! Exit codes: `0` complete, `1` a call or counter-test failed, `2` the matrix
//! has a gap.

mod agent;
mod serve;

use std::sync::Arc;

use a2a_protocol_client::{resolve_agent_card, ClientBuilder};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_types::agent_card::AgentCapabilities;

use a2a_example_harness::{counter, sweep, Binding, Endpoints, Matrix};
use agent::{make_agent_card, EchoExecutor};

/// A webhook sink so push configs point somewhere real.
///
/// A config accepted against a dead URL proves storage, not delivery, and this
/// example should not imply the latter from the former.
async fn start_webhook() -> String {
    let (listener, addr) = serve::bind_listener().await;
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let io = hyper_util::rt::TokioIo::new(stream);
            tokio::spawn(async move {
                let service = hyper::service::service_fn(|_req| async {
                    Ok::<_, std::convert::Infallible>(hyper::Response::new(
                        http_body_util::Full::new(bytes::Bytes::from_static(b"ok")),
                    ))
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });
    format!("http://{addr}/webhook")
}

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

    // ── Server-only mode (for the in-repo TCK / CI) ──────────────────────
    if let Ok(bind_addr) = std::env::var("A2A_BIND_ADDR") {
        server_only(&bind_addr).await;
        return;
    }

    println!("=== A2A Echo Agent — full-surface demo ===\n");

    // Pre-bind every listener so the card names real addresses.
    let (jsonrpc_l, jsonrpc_a) = serve::bind_listener().await;
    let (rest_l, rest_a) = serve::bind_listener().await;
    let (grpc_l, grpc_a) = serve::bind_listener().await;
    let (ws_probe, ws_a) = serve::bind_listener().await;
    drop(ws_probe); // the WebSocket dispatcher binds its own listener

    let endpoints = Endpoints {
        jsonrpc: format!("http://{jsonrpc_a}"),
        rest: format!("http://{rest_a}"),
        grpc: grpc_a.to_string(),
        websocket: format!("ws://{ws_a}"),
    };

    let webhook_url = start_webhook().await;

    let handler = Arc::new(
        RequestHandlerBuilder::new(EchoExecutor)
            .with_agent_card(make_agent_card(&endpoints))
            .with_push_config_store(a2a_protocol_server::push::InMemoryPushConfigStore::new())
            .with_push_sender(a2a_protocol_server::push::HttpPushSender::new().allow_private_urls())
            // Spec §13.3 requires the extended card to be authenticated. This
            // example ships no authenticator, so the opt-in is explicit rather
            // than implied — and `counter` checks the refusal on an agent that
            // has neither.
            .allow_unauthenticated_extended_card()
            .build()
            .expect("build handler"),
    );

    serve::serve_jsonrpc(jsonrpc_l, Arc::clone(&handler));
    serve::serve_rest(rest_l, Arc::clone(&handler));
    serve::serve_grpc(grpc_l, Arc::clone(&handler));
    let ws_bound = serve::serve_websocket(&ws_a.to_string(), Arc::clone(&handler)).await;

    println!("JSON-RPC  {}", endpoints.jsonrpc);
    println!("HTTP+JSON {}", endpoints.rest);
    println!("gRPC      {}", endpoints.grpc);
    println!("WebSocket ws://{ws_bound}");
    println!("webhook   {webhook_url}\n");

    // Discovery, once — the card is shared by every binding.
    match resolve_agent_card(&endpoints.jsonrpc).await {
        Ok(card) => println!(
            "Discovered '{}' v{} — {} interface(s), streaming={:?} push={:?} extended={:?}\n",
            card.name,
            card.version,
            card.supported_interfaces.len(),
            card.capabilities.streaming,
            card.capabilities.push_notifications,
            card.capabilities.extended_agent_card,
        ),
        Err(e) => {
            eprintln!("agent card discovery failed: {e}");
            std::process::exit(1);
        }
    }

    let mut matrix = Matrix::new();
    let mut failures: Vec<String> = Vec::new();

    for binding in Binding::ALL {
        let client = match build_client(*binding, &endpoints).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("could not build a {} client: {e}", binding.label());
                std::process::exit(1);
            }
        };
        println!("--- {} ---", binding.label());
        let outcome = sweep(
            &client,
            *binding,
            &webhook_url,
            agent::SLOW_PREFIX,
            &mut matrix,
        )
        .await;
        for l in &outcome.lines {
            println!("{l}");
        }
        failures.extend(outcome.failures);
        println!();
    }

    // ── Counter-tests ────────────────────────────────────────────────────
    println!("--- counter-tests (calls that must be refused) ---");
    let restricted_url = start_restricted_agent().await;
    let main_client = ClientBuilder::new(&endpoints.jsonrpc)
        .build()
        .expect("client");
    let restricted_client = ClientBuilder::new(&restricted_url)
        .build()
        .expect("restricted client");
    let counter = counter::run(&main_client, &restricted_client).await;
    for l in &counter.lines {
        println!("{l}");
    }
    failures.extend(counter.failures);
    println!();

    // ── Report ───────────────────────────────────────────────────────────
    println!("=== Coverage: every A2A method over every binding ===\n");
    let missing = matrix.report();

    if !failures.is_empty() {
        println!("\n{} call(s) failed:", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
        std::process::exit(1);
    }
    if !missing.is_empty() {
        println!("\n{} matrix cell(s) never ran:", missing.len());
        for (m, b) in &missing {
            println!("  - {} over {}", m.wire_name(), b.label());
        }
        std::process::exit(2);
    }

    println!("\nEvery A2A method was exercised over every binding, and every");
    println!("counter-test was refused as the specification requires.");
}

/// Builds a client speaking `binding`.
async fn build_client(
    binding: Binding,
    ep: &Endpoints,
) -> Result<a2a_protocol_client::A2aClient, String> {
    match binding {
        Binding::JsonRpc => ClientBuilder::new(&ep.jsonrpc)
            .build()
            .map_err(|e| e.to_string()),
        Binding::HttpJson => ClientBuilder::new(&ep.rest)
            .with_protocol_binding("HTTP+JSON")
            .build()
            .map_err(|e| e.to_string()),
        Binding::Grpc => {
            let url = format!("http://{}", ep.grpc);
            let transport = a2a_protocol_client::transport::grpc::GrpcTransport::connect(&url)
                .await
                .map_err(|e| format!("gRPC connect: {e}"))?;
            ClientBuilder::new(&url)
                .with_custom_transport(transport)
                .without_tls()
                .build()
                .map_err(|e| e.to_string())
        }
        Binding::WebSocket => {
            let transport =
                a2a_protocol_client::transport::WebSocketTransport::connect(ep.websocket.clone())
                    .await
                    .map_err(|e| format!("WebSocket connect: {e}"))?;
            ClientBuilder::new(&ep.websocket)
                .with_custom_transport(transport)
                .without_tls()
                .build()
                .map_err(|e| e.to_string())
        }
    }
}

/// Starts a second agent advertising no optional capabilities.
///
/// One agent cannot both support and refuse a feature, so the capability
/// refusals in [`counter`] are unobservable without this. Returns its URL.
async fn start_restricted_agent() -> String {
    let (listener, addr) = serve::bind_listener().await;
    let url = format!("http://{addr}");
    let mut card = make_agent_card(&Endpoints {
        jsonrpc: url.clone(),
        rest: url.clone(),
        grpc: addr.to_string(),
        websocket: format!("ws://{addr}"),
    });
    card.name = "Restricted Echo Agent".into();
    card.capabilities = AgentCapabilities::none();
    card.supported_interfaces.truncate(1);

    let handler = Arc::new(
        RequestHandlerBuilder::new(EchoExecutor)
            .with_agent_card(card)
            .build()
            .expect("build restricted handler"),
    );
    serve::serve_jsonrpc(listener, handler);
    url
}

/// Long-running server mode used by the in-repo TCK.
async fn server_only(bind_addr: &str) {
    let url = format!("http://{bind_addr}");
    let ws_addr = std::env::var("A2A_WS_BIND_ADDR").ok();
    let grpc_addr = std::env::var("A2A_GRPC_BIND_ADDR").ok();

    let mut endpoints = Endpoints {
        jsonrpc: url.clone(),
        rest: url.clone(),
        grpc: grpc_addr.clone().unwrap_or_default(),
        websocket: ws_addr
            .clone()
            .map(|a| format!("ws://{a}"))
            .unwrap_or_default(),
    };
    if endpoints.grpc.is_empty() {
        endpoints.grpc = String::new();
    }

    let mut card = make_agent_card(&endpoints);
    card.url = Some(url.clone());
    // Only advertise what is actually listening. A card naming a port nothing
    // answers on turns a config error into what looks like a conformance
    // failure of that binding.
    card.supported_interfaces
        .retain(|i| match i.protocol_binding.as_str() {
            "WEBSOCKET" => ws_addr.is_some(),
            "GRPC" => grpc_addr.is_some(),
            _ => true,
        });

    let handler = Arc::new(
        RequestHandlerBuilder::new(EchoExecutor)
            .with_agent_card(card)
            .with_push_config_store(a2a_protocol_server::push::InMemoryPushConfigStore::new())
            .with_push_sender(a2a_protocol_server::push::HttpPushSender::new().allow_private_urls())
            .allow_unauthenticated_extended_card()
            .build()
            .expect("build handler"),
    );

    // One socket answering both HTTP bindings: the in-repo TCK points its
    // jsonrpc and rest legs at the same A2A_BIND_ADDR.
    let bound = serve::serve_combined(bind_addr, Arc::clone(&handler)).await;
    println!("Echo agent listening on http://{bound} (JSON-RPC + REST)");

    if let Some(a) = grpc_addr {
        let (l, _) = (
            tokio::net::TcpListener::bind(&a)
                .await
                .unwrap_or_else(|e| panic!("bind grpc {a}: {e}")),
            (),
        );
        let bound = serve::serve_grpc(l, Arc::clone(&handler));
        println!("Echo agent gRPC listening on {bound}");
    }
    if let Some(a) = ws_addr {
        let bound = serve::serve_websocket(&a, Arc::clone(&handler)).await;
        println!("Echo agent WebSocket listening on ws://{bound}");
    }

    std::future::pending::<()>().await;
}
