// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One agent, one `RequestHandler`, served on all four bindings at once, and
//! this SDK's client for each.

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_client::error::ClientResult;
use a2a_protocol_client::token_provider::{BearerAuthInterceptor, TokenProvider};
use a2a_protocol_client::transport::grpc::GrpcTransport;
use a2a_protocol_client::transport::websocket::{WebSocketTransport, WebSocketTransportConfig};
use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{
    BearerTokenAuthInterceptor, CallContext, EventEmitter, GrpcConfig, GrpcDispatcher,
    HttpPushSender, InMemoryPushConfigStore, RequestHandler, RequestHandlerBuilder,
    ServerInterceptor, WebSocketDispatcher, agent_executor,
};
use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::{Part, PartContent};
use a2a_protocol_types::task::TaskState;
use serde_json::json;

/// The bearer token the secured agent accepts.
pub const TOKEN: &str = "bind-equiv-token";

/// A message with this text parks its task at `WORKING` until it is
/// canceled; any other text is echoed back as an artifact and completes.
pub const HOLD: &str = "hold";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    JsonRpc,
    HttpJson,
    Grpc,
    WebSocket,
}

impl Binding {
    pub const ALL: [Self; 4] = [Self::JsonRpc, Self::HttpJson, Self::Grpc, Self::WebSocket];
}

/// Echoes the message's text as an artifact, or holds until canceled.
pub struct Echo;
agent_executor!(Echo, |ctx, queue| async {
    let text = ctx
        .message
        .parts
        .iter()
        .find_map(|p| match &p.content {
            PartContent::Text(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    if text == HOLD {
        // The default `cancel` writes `CANCELED` once the token fires.
        ctx.cancellation_token.cancelled().await;
        return Ok(());
    }
    emit.artifact("echo", vec![Part::text(text)], None, Some(true))
        .await?;
    emit.status(TaskState::Completed).await
});

/// What the agent saw of each call: the method and its `A2A-Version`.
pub type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// Records every call's method and `A2A-Version` header.
pub struct Recorder(pub Seen);

impl ServerInterceptor for Recorder {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        let version = ctx
            .http_headers()
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("a2a-version"))
            .map(|(_, v)| v.clone());
        self.0
            .lock()
            .expect("lock")
            .push((ctx.method().to_owned(), version));
        Box::pin(async { Ok(()) })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

/// The card: every optional capability declared, and, when `secured`, a
/// bearer scheme required of every call.
pub fn card(secured: bool) -> AgentCard {
    let mut card = json!({
        "name": "equivalence", "description": "one agent, four bindings", "version": "1",
        "supportedInterfaces": [
            {"url": "http://127.0.0.1:1", "protocolBinding": "JSONRPC", "protocolVersion": "1.0"}
        ],
        "capabilities": {"streaming": true, "pushNotifications": true, "extendedAgentCard": true},
        "defaultInputModes": ["text/plain"], "defaultOutputModes": ["text/plain"],
        "skills": [{"id": "echo", "name": "Echo", "description": "echoes", "tags": ["echo"]}]
    });
    if secured {
        card["securitySchemes"] =
            json!({"bearer": {"httpAuthSecurityScheme": {"scheme": "bearer"}}});
        card["securityRequirements"] = json!([{"schemes": {"bearer": {}}}]);
    }
    serde_json::from_value(card).expect("card")
}

/// The agent. `extended` configures the extended card the public card
/// declares; without it `GetExtendedAgentCard` must answer
/// `ExtendedAgentCardNotConfiguredError` (spec §3.1.11).
pub fn handler(secured: bool, extended: bool, seen: &Seen) -> Arc<RequestHandler> {
    let mut builder = RequestHandlerBuilder::new(Echo)
        .with_agent_card(card(secured))
        .with_push_config_store(InMemoryPushConfigStore::new())
        .with_push_sender(HttpPushSender::new())
        .with_interceptor(Recorder(Arc::clone(seen)));
    if extended {
        builder = builder.with_extended_agent_card(card(secured));
    }
    builder = if secured {
        builder
            .with_interceptor(BearerTokenAuthInterceptor::new([TOKEN]))
            .share_tasks_across_callers()
    } else {
        builder.allow_unauthenticated_extended_card()
    };
    Arc::new(builder.build().expect("handler"))
}

/// Where each binding of one agent listens.
pub struct Agent {
    pub seen: Seen,
    jsonrpc: SocketAddr,
    http_json: SocketAddr,
    grpc: SocketAddr,
    websocket: SocketAddr,
}

/// Serves one handler on JSON-RPC, HTTP+JSON, gRPC and WebSocket.
pub async fn serve(secured: bool, extended: bool) -> Agent {
    let seen = Seen::default();
    let handler = handler(secured, extended, &seen);
    let jsonrpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("JSON-RPC");
    let http_json = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("HTTP+JSON");
    let grpc = GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default())
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("gRPC");
    let websocket = Arc::new(WebSocketDispatcher::new(Arc::clone(&handler)))
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("WebSocket");
    Agent {
        seen,
        jsonrpc,
        http_json,
        grpc,
        websocket,
    }
}

/// Always answers the one token it was given.
struct Static(String);

impl TokenProvider for Static {
    fn access_token(&self) -> Pin<Box<dyn Future<Output = ClientResult<String>> + Send + '_>> {
        Box::pin(async move { Ok(self.0.clone()) })
    }
}

impl Agent {
    /// The base URL `binding` is served at.
    pub fn url(&self, binding: Binding) -> String {
        match binding {
            Binding::JsonRpc => format!("http://{}", self.jsonrpc),
            Binding::HttpJson => format!("http://{}", self.http_json),
            Binding::Grpc => format!("http://{}", self.grpc),
            Binding::WebSocket => format!("ws://{}", self.websocket),
        }
    }

    /// This SDK's client for `binding`, presenting `token` when given one.
    pub async fn client(&self, binding: Binding, token: Option<&str>) -> A2aClient {
        let builder = match binding {
            Binding::JsonRpc => {
                ClientBuilder::new(self.url(binding)).with_protocol_binding("JSONRPC")
            }
            Binding::HttpJson => {
                ClientBuilder::new(self.url(binding)).with_protocol_binding("HTTP+JSON")
            }
            Binding::Grpc => {
                let url = self.url(binding);
                let transport = GrpcTransport::connect(&url).await.expect("gRPC connect");
                ClientBuilder::new(&url).with_custom_transport(transport)
            }
            Binding::WebSocket => {
                // Binding spec §6: the credential rides the upgrade request.
                let url = self.url(binding);
                let headers: HashMap<String, String> = token
                    .map(|t| ("authorization".to_owned(), format!("Bearer {t}")))
                    .into_iter()
                    .collect();
                let transport = WebSocketTransport::connect_with_config(
                    &url,
                    WebSocketTransportConfig::default().with_extra_headers(headers),
                )
                .await
                .expect("WebSocket connect");
                ClientBuilder::new(&url).with_custom_transport(transport)
            }
        };
        let builder = match token {
            Some(t) => builder
                .with_interceptor(BearerAuthInterceptor::new(
                    Arc::new(Static(t.to_owned())) as Arc<dyn TokenProvider>
                )),
            None => builder,
        };
        builder.build().expect("client")
    }

    /// Forgets the calls recorded so far, returning them.
    pub fn take_seen(&self) -> Vec<(String, Option<String>)> {
        std::mem::take(&mut *self.seen.lock().expect("lock"))
    }
}

/// A request sent as raw HTTP, so the test, not a client, decides what is
/// on the wire: `version` is the `A2A-Version` header, absent when `None`.
/// Answers the status and the JSON body.
pub async fn raw(
    url: String,
    method: &str,
    body: Option<serde_json::Value>,
    version: Option<&str>,
) -> (u16, serde_json::Value) {
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http::<Full<Bytes>>();
    let mut req = hyper::Request::builder()
        .method(method)
        .uri(url)
        .header("content-type", "application/json");
    if let Some(v) = version {
        req = req.header("a2a-version", v);
    }
    let body = body.map_or_else(Bytes::new, |b| Bytes::from(b.to_string()));
    let resp = client
        .request(req.body(Full::new(body)).expect("request"))
        .await
        .expect("response");
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, serde_json::from_slice(&bytes).expect("a JSON body"))
}
