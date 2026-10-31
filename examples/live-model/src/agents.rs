// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The three agents the checks drive, all on loopback:
//!
//! * the **worker**: `ToolLoopExecutor` over the real model and the tools in
//!   `tools.rs`, signing what it serves, on JSON-RPC, HTTP+JSON, gRPC and
//!   WebSocket at once;
//! * the **coordinator**: delegates each message to the worker with
//!   `Delegation::start_budgeted`, granting it what is left of the budget it
//!   was given, and relays the worker's answer;
//! * the **approver**: asks the model for a one-line plan, holds the task for
//!   two approvers (`ApprovalGate::with_quorum(2)`), and completes naming
//!   them.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::client::delegation::{Delegation, Outcome};
use a2a_protocol_sdk::client::transport::grpc::GrpcTransport;
use a2a_protocol_sdk::client::transport::websocket::WebSocketTransport;
use a2a_protocol_sdk::client::{A2aClient, ClientBuilder};
use a2a_protocol_sdk::server::approval::ApprovalGate;
use a2a_protocol_sdk::server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_sdk::server::executor::AgentExecutor;
use a2a_protocol_sdk::server::request_context::RequestContext;
use a2a_protocol_sdk::server::serve::serve_with_addr;
use a2a_protocol_sdk::server::streaming::EventQueueWriter;
use a2a_protocol_sdk::server::tool_loop::{
    ANSWER_ARTIFACT_ID, ChatMessage, ChatRequest, OpenAiChatModel, ToolLoop, ToolLoopExecutor,
    ToolModel,
};
use a2a_protocol_sdk::server::{
    BearerTokenAuthInterceptor, EventEmitter, GrpcConfig, GrpcDispatcher, RequestHandler,
    RequestHandlerBuilder, WebSocketDispatcher,
};
use a2a_protocol_sdk::types::approval::{ApprovalRequest, action_digest};
use a2a_protocol_sdk::types::budget::Budget;
use a2a_protocol_sdk::types::error::{A2aError, A2aResult};
use a2a_protocol_sdk::types::provenance::ContentSigner;
use a2a_protocol_sdk::types::signing::Jwks;
use a2a_protocol_sdk::types::{Message, MessageSendParams, Part, TaskQueryParams, TaskState};

use crate::tools::{self, Restarts};

/// What the worker is told before every turn: `tool-loop-agent`'s preamble.
pub const PREAMBLE: &str = "You are an operations agent with tools for checking and restarting services. \
Call the tools yourself to answer; never ask the user to call a tool. \
If a tool returns an error that suggests another tool, call that tool, then answer. \
Report values exactly as the tools returned them.";

/// How long one run may take: a few model turns on a busy CPU.
const EXECUTOR_TIMEOUT: Duration = Duration::from_secs(900);

type Fut<'a> = Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

/// The four bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    JsonRpc,
    HttpJson,
    Grpc,
    WebSocket,
}

impl Binding {
    pub const ALL: [Self; 4] = [Self::JsonRpc, Self::HttpJson, Self::Grpc, Self::WebSocket];

    pub const fn name(self) -> &'static str {
        match self {
            Self::JsonRpc => "JSONRPC",
            Self::HttpJson => "HTTP+JSON",
            Self::Grpc => "GRPC",
            Self::WebSocket => "WEBSOCKET",
        }
    }
}

/// The worker, served on all four bindings.
pub struct Worker {
    pub handler: Arc<RequestHandler>,
    pub restarts: Restarts,
    /// The public key of what it signs.
    pub keys: Jwks,
    addrs: [SocketAddr; 4],
}

impl Worker {
    /// Serves a worker over `model`.
    pub async fn serve(model: OpenAiChatModel) -> Result<Self, Box<dyn std::error::Error>> {
        let restarts = Restarts::default();
        let pkcs8 =
            ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                .map_err(|_| "could not generate an Ed25519 key")?;
        let signer = ContentSigner::ed25519("live-worker", pkcs8.as_ref())?;
        let keys = Jwks::new(vec![signer.public_jwk()]);
        let tool_loop = ToolLoop::new(model, tools::catalogue(&restarts)?)
            .with_preamble(PREAMBLE)
            .with_agent_name("live-worker");
        let handler = Arc::new(
            RequestHandlerBuilder::new(ToolLoopExecutor::new(tool_loop).with_drafts())
                .without_agent_card()
                .with_content_signer(signer)
                .with_executor_timeout(EXECUTOR_TIMEOUT)
                .build()?,
        );
        let jsonrpc =
            serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler))).await?;
        let http_json =
            serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::clone(&handler))).await?;
        let grpc = GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default())
            .serve_with_addr("127.0.0.1:0")
            .await?;
        let websocket = Arc::new(WebSocketDispatcher::new(Arc::clone(&handler)))
            .serve_with_addr("127.0.0.1:0")
            .await?;
        Ok(Self {
            handler,
            restarts,
            keys,
            addrs: [jsonrpc, http_json, grpc, websocket],
        })
    }

    /// A client for `binding` that verifies what it receives against `keys`.
    pub async fn client(
        &self,
        binding: Binding,
        keys: Jwks,
    ) -> Result<A2aClient, Box<dyn std::error::Error>> {
        let builder = match binding {
            Binding::JsonRpc => ClientBuilder::new(format!("http://{}", self.addrs[0]))
                .with_protocol_binding("JSONRPC"),
            Binding::HttpJson => ClientBuilder::new(format!("http://{}", self.addrs[1]))
                .with_protocol_binding("HTTP+JSON"),
            Binding::Grpc => {
                let url = format!("http://{}", self.addrs[2]);
                let transport = GrpcTransport::connect(&url).await?;
                ClientBuilder::new(&url).with_custom_transport(transport)
            }
            Binding::WebSocket => {
                let url = format!("ws://{}", self.addrs[3]);
                let transport = WebSocketTransport::connect(&url).await?;
                ClientBuilder::new(&url).with_custom_transport(transport)
            }
        };
        Ok(builder
            .with_timeout(Duration::from_secs(120))
            .with_content_keys(keys)
            .build()?)
    }
}

/// Delegates its message to the worker, granting it all of the "tokens"
/// meter left in the budget it was given, and relays the worker's answer.
struct Coordinator {
    worker: Arc<A2aClient>,
}

impl AgentExecutor for Coordinator {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Fut<'a> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            let budget = ctx
                .budget()
                .ok_or_else(|| A2aError::invalid_params("send a budget to delegate with"))?;
            // A meter the request does not name is granted as 0, so ask for
            // all of "tokens"; the grant is capped at what is left.
            let grant = budget
                .grant(&Budget::new().with_meter("tokens", u64::MAX)?)
                .map_err(|e| A2aError::internal(e.to_string()))?;
            let text = ctx.message.text().unwrap_or_default().to_owned();
            let params =
                MessageSendParams::new(Message::user_text(uuid::Uuid::new_v4().to_string(), text));
            let child = Delegation::start_budgeted(Arc::clone(&self.worker), params, grant, None)
                .await
                .map_err(|e| A2aError::internal(e.to_string()))?;
            let done = child.wait(ctx.cancellation_token.cancelled()).await;
            let status = match done.outcome {
                Outcome::Completed(_) => None,
                Outcome::Failed { status, .. } | Outcome::Canceled(status) => Some(status),
                other => {
                    return Err(A2aError::internal(format!(
                        "the worker did not complete: {other:?}"
                    )));
                }
            };
            if let Some(status) = status {
                let why = status
                    .message
                    .as_ref()
                    .and_then(Message::text)
                    .unwrap_or_default()
                    .to_owned();
                return Err(A2aError::internal(format!(
                    "the worker's task ended {}: {why}",
                    status.state
                )));
            }
            let id = done
                .task_id
                .ok_or_else(|| A2aError::internal("the worker's task was never named"))?;
            let task = self
                .worker
                .get_task(TaskQueryParams::new(id.clone()))
                .await
                .map_err(|e| A2aError::internal(e.to_string()))?;
            let answer = task
                .artifacts
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|a| a.id.0 == ANSWER_ARTIFACT_ID)
                .and_then(|a| a.parts.iter().find_map(Part::text_content))
                .unwrap_or_default()
                .to_owned();
            let relay = format!("worker task {id}: {answer}");
            emit.artifact("relay", vec![Part::text(relay)], None, Some(true))
                .await?;
            emit.status(TaskState::Completed).await
        })
    }
}

/// Serves the coordinator over JSON-RPC, delegating to `worker`.
pub async fn serve_coordinator(worker: A2aClient) -> Result<String, Box<dyn std::error::Error>> {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Coordinator {
            worker: Arc::new(worker),
        })
        .without_agent_card()
        .with_executor_timeout(EXECUTOR_TIMEOUT)
        .build()?,
    );
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler)).await?;
    Ok(format!("http://{addr}"))
}

/// The action the approver asks about, and so the digest approvers confirm.
pub const ACTION: &str = "restart payments-api";

/// Asks the model for a plan, holds the task for approval, and on the
/// continuation that met the quorum completes naming the approvers.
struct Approver {
    model: OpenAiChatModel,
}

impl AgentExecutor for Approver {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Fut<'a> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            if let Some(approval) = ctx.approval() {
                if !approval.is_approved() {
                    return emit
                        .fail(
                            a2a_protocol_sdk::types::failure::FailureClass::PolicyRefusal,
                            format!("denied by {}", approval.approver),
                        )
                        .await;
                }
                let text = format!(
                    "{} approved by [{}] of {} required: {}",
                    ACTION,
                    approval.approvers.join(", "),
                    approval.required,
                    approval.request.summary
                );
                emit.artifact("approved", vec![Part::text(text)], None, Some(true))
                    .await?;
                return emit.status(TaskState::Completed).await;
            }
            let ask = [ChatMessage::User(
                "In one sentence of at most 25 words, state a plan to restart the degraded \
                 payments-api service safely."
                    .to_owned(),
            )];
            let plan = self
                .model
                .complete(ChatRequest::new("", &ask, &[]))
                .await
                .map_err(|e| A2aError::internal(e.to_string()))?
                .text;
            let digest = action_digest(&serde_json::json!({ "action": ACTION }))?;
            let request = ApprovalRequest::new("restart-1", plan.trim(), digest);
            emit.request_approval(request, "Two operators must approve this plan.")
                .await?;
            Ok(())
        })
    }
}

/// The approver's bearer tokens and the identities they authenticate as.
pub const TOKENS: [(&str, &str); 3] = [
    ("t-requester", "requester"),
    ("t-bob", "bob"),
    ("t-carol", "carol"),
];

/// Serves the approver over JSON-RPC with a quorum of two.
pub async fn serve_approver(model: OpenAiChatModel) -> Result<String, Box<dyn std::error::Error>> {
    let handler = Arc::new(
        RequestHandlerBuilder::new(Approver { model })
            .without_agent_card()
            .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens(TOKENS))
            .with_approval_gate(ApprovalGate::new().with_quorum(2))
            .share_tasks_across_callers()
            .with_executor_timeout(EXECUTOR_TIMEOUT)
            .build()?,
    );
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler)).await?;
    Ok(format!("http://{addr}"))
}
