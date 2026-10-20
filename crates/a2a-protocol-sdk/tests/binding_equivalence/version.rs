// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §3.6, versioning — the three MUSTs the official TCK leaves untested:
//!
//! - `VER-CLIENT-001` (§3.6.1): "Clients MUST send the `A2A-Version` header
//!   with each request."
//! - `VER-CLIENT-002` (§3.6): "Patch version numbers MUST NOT be considered
//!   when clients and servers negotiate protocol versions."
//! - `VER-SERVER-001` (§3.6.2): "Agents MUST process requests using the
//!   semantics of the requested `A2A-Version` (matching `Major.Minor`)."
//!   This server implements one `Major.Minor`, 1.0, so the requirement is
//!   that a request for 1.0 (any patch) is processed and a request for any
//!   other version — 0.3, which an absent version means, or 1.1, or 2.0 —
//!   is refused with `VersionNotSupportedError` rather than processed with
//!   1.0's semantics. Shown on all four bindings.

use std::collections::BTreeSet;
use std::future::Future;

use a2a_protocol_client::error::{ClientError, ClientResult};
use a2a_protocol_client::transport::websocket::{WebSocketTransport, WebSocketTransportConfig};
use a2a_protocol_client::{
    CallInterceptor, ClientBuilder, ClientRequest, ClientResponse, GrpcTransport,
};
use a2a_protocol_types::A2A_VERSION;
use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::params::ListTasksParams;
use serde_json::json;

use crate::functionality::{every_method, sweep};
use crate::harness::{Binding, raw, serve};

/// VER-CLIENT-001: every call this SDK's client makes, on every binding,
/// carries `A2A-Version`, naming `Major.Minor` only (§3.6: patch numbers
/// "SHOULD NOT be used in requests").
#[tokio::test]
async fn ver_client_001_every_call_on_every_binding_carries_a2a_version() {
    assert_eq!(A2A_VERSION, "1.0", "Major.Minor, no patch");
    let agent = serve(false, true).await;
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        sweep(&client)
            .await
            .unwrap_or_else(|e| panic!("{binding:?}: {e}"));
        let seen = agent.take_seen();
        let methods: BTreeSet<String> = seen.iter().map(|(m, _)| m.clone()).collect();
        assert_eq!(methods, every_method(), "{binding:?}: every operation ran");
        for (method, version) in seen {
            assert_eq!(
                version.as_deref(),
                Some(A2A_VERSION),
                "{binding:?} {method} must carry A2A-Version"
            );
        }
    }
}

/// VER-CLIENT-002: interfaces whose card declares `1.0.9` and `1.0.3` are
/// chosen, and spoken to, exactly as ones declaring `1.0` are: the patch
/// changes neither which interface is picked nor the version requested.
#[tokio::test]
async fn ver_client_002_a_patch_on_the_card_does_not_change_negotiation() {
    let agent = serve(false, true).await;
    let card_at = |patch: &str| -> AgentCard {
        serde_json::from_value(json!({
            "name": "patched", "description": "d", "version": "1",
            "supportedInterfaces": [
                {"url": agent.url(Binding::JsonRpc), "protocolBinding": "JSONRPC",
                 "protocolVersion": format!("1.0{patch}")},
                {"url": agent.url(Binding::HttpJson), "protocolBinding": "HTTP+JSON",
                 "protocolVersion": format!("1.0{patch}")}
            ],
            "capabilities": {},
            "defaultInputModes": ["text/plain"], "defaultOutputModes": ["text/plain"],
            "skills": [{"id": "s", "name": "S", "description": "d", "tags": ["t"]}]
        }))
        .expect("card")
    };
    for preferred in ["JSONRPC", "HTTP+JSON"] {
        let mut outcomes = Vec::new();
        for patch in ["", ".9", ".3"] {
            let client = ClientBuilder::from_card_preferring(&card_at(patch), &[preferred.into()])
                .unwrap_or_else(|e| panic!("1.0{patch} via {preferred} is compatible: {e:?}"))
                .build()
                .expect("client");
            client
                .list_tasks(ListTasksParams::default())
                .await
                .unwrap_or_else(|e| panic!("1.0{patch} via {preferred}: {e:?}"));
            outcomes.push(agent.take_seen());
        }
        assert_eq!(
            outcomes[0],
            [("ListTasks".to_owned(), Some("1.0".to_owned()))],
            "{preferred}: the request names Major.Minor"
        );
        assert!(
            outcomes.iter().all(|o| *o == outcomes[0]),
            "{preferred}: the patch changed what was requested: {outcomes:?}"
        );
    }
}

/// Requested versions this server must process, and ones it must refuse.
const PROCESSED: [&str; 3] = ["1.0", "1.0.7", "1.00"];
const REFUSED: [&str; 4] = ["0.3", "1.1", "2.0", "0.9"];

fn list_tasks_rpc() -> serde_json::Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "ListTasks", "params": {}})
}

/// Overrides the `A2A-Version` a gRPC call carries (the transport sets its
/// own first; caller metadata replaces it).
struct Requests(&'static str);

impl CallInterceptor for Requests {
    #[allow(clippy::manual_async_fn)]
    fn before<'a>(
        &'a self,
        req: &'a mut ClientRequest,
    ) -> impl Future<Output = ClientResult<()>> + Send + 'a {
        async move {
            req.extra_headers
                .insert("a2a-version".to_owned(), self.0.to_owned());
            Ok(())
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn after<'a>(
        &'a self,
        _resp: &'a ClientResponse,
    ) -> impl Future<Output = ClientResult<()>> + Send + 'a {
        async { Ok(()) }
    }
}

/// VER-SERVER-001 on the two HTTP bindings, from raw requests.
#[tokio::test]
async fn ver_server_001_http_bindings_process_only_the_requested_major_minor() {
    let agent = serve(false, true).await;
    let rpc = agent.url(Binding::JsonRpc);
    let rest = format!("{}/tasks", agent.url(Binding::HttpJson));
    let refusal = ErrorCode::VersionNotSupported;

    for v in PROCESSED {
        let (_, answer) = raw(rpc.clone(), "POST", Some(list_tasks_rpc()), Some(v)).await;
        assert!(answer.get("result").is_some(), "JSON-RPC {v}: {answer}");
        let (status, answer) = raw(rest.clone(), "GET", None, Some(v)).await;
        assert_eq!(status, 200, "HTTP+JSON {v}: {answer}");
        let seen = agent.take_seen();
        assert_eq!(
            seen,
            [
                ("ListTasks".to_owned(), Some(v.to_owned())),
                ("ListTasks".to_owned(), Some(v.to_owned()))
            ],
            "{v} reached the handler as requested"
        );
    }
    // `None`: §3.6.2, "Agents MUST interpret empty value as 0.3 version."
    for v in REFUSED.map(Some).into_iter().chain([None]) {
        let (_, answer) = raw(rpc.clone(), "POST", Some(list_tasks_rpc()), v).await;
        assert_eq!(
            answer["error"]["code"].as_i64(),
            Some(i64::from(refusal.as_i32())),
            "JSON-RPC {v:?}: {answer}"
        );
        let (status, answer) = raw(rest.clone(), "GET", None, v).await;
        assert_eq!(status, refusal.http_status(), "HTTP+JSON {v:?}: {answer}");
        assert!(
            agent.take_seen().is_empty(),
            "{v:?} must not reach the handler"
        );
    }
}

/// VER-SERVER-001 on gRPC (per call) and WebSocket (per connection, at the
/// upgrade, which is where that binding carries the version).
#[tokio::test]
async fn ver_server_001_grpc_and_websocket_process_only_the_requested_major_minor() {
    let agent = serve(false, true).await;
    let grpc = |v: &'static str| {
        let url = agent.url(Binding::Grpc);
        async move {
            ClientBuilder::new(&url)
                .with_custom_transport(GrpcTransport::connect(&url).await.expect("connect"))
                .with_interceptor(Requests(v))
                .build()
                .expect("client")
                .list_tasks(ListTasksParams::default())
                .await
        }
    };
    let websocket = |v: &'static str| {
        let url = agent.url(Binding::WebSocket);
        async move {
            let headers = [("A2A-Version".to_owned(), v.to_owned())].into();
            WebSocketTransport::connect_with_config(
                &url,
                WebSocketTransportConfig::default().with_extra_headers(headers),
            )
            .await
            .map(|transport| {
                ClientBuilder::new(&url)
                    .with_custom_transport(transport)
                    .build()
                    .expect("client")
            })
        }
    };

    for v in PROCESSED {
        grpc(v).await.unwrap_or_else(|e| panic!("gRPC {v}: {e:?}"));
        websocket(v)
            .await
            .unwrap_or_else(|e| panic!("WebSocket {v}: {e:?}"))
            .list_tasks(ListTasksParams::default())
            .await
            .unwrap_or_else(|e| panic!("WebSocket {v}: {e:?}"));
        assert_eq!(
            agent.take_seen(),
            [
                ("ListTasks".to_owned(), Some(v.to_owned())),
                ("ListTasks".to_owned(), Some(v.to_owned()))
            ],
            "{v} reached the handler as requested"
        );
    }
    for v in REFUSED {
        match grpc(v).await {
            Err(ClientError::Protocol(e)) if e.code == ErrorCode::VersionNotSupported => {}
            other => panic!("gRPC {v} must be VersionNotSupported: {other:?}"),
        }
        assert!(
            websocket(v).await.is_err(),
            "WebSocket {v}: the upgrade must be refused"
        );
        assert!(
            agent.take_seen().is_empty(),
            "{v} must not reach the handler"
        );
    }
}
