// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Scaffolding the three acts share: serving a handler on a loopback port,
//! building messages, and the fault injectors.
//!
//! Everything here is deliberately plain. The agents are real
//! [`RequestHandler`]s served by the SDK's own [`serve_with_addr`]; the
//! faults are injected by ordinary hyper servers on real sockets, so a passing
//! act proves the SDK rode out a fault it actually met rather than one a stub
//! returned in-process.

pub mod executors;
pub mod injectors;
pub mod metrics;

use std::sync::Arc;

use a2a_protocol_client::{A2aClient, ClientBuilder, ClientError};
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_protocol_server::{CallContext, RequestHandler, ServerInterceptor, serve_with_addr};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::params::{MessageSendParams, SendMessageConfiguration};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::Task;

/// Serves `handler` over JSON-RPC on an ephemeral loopback port and returns
/// its base URL.
///
/// The listener task runs for the life of the process. "Dropping the handler"
/// in Act 1 means dropping every `Arc` this example holds *and* never
/// touching this port again — the spawned acceptor keeps its own clone, which
/// is exactly why the act reopens the database in a second handler on a
/// second port rather than pretending the first one is gone.
pub async fn serve(handler: RequestHandler) -> Result<(Arc<RequestHandler>, String), String> {
    let handler = Arc::new(handler);
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler)))
        .await
        .map_err(|e| format!("binding a loopback port: {e}"))?;
    Ok((handler, format!("http://{addr}")))
}

/// A plain client for `url`.
pub fn client(url: &str) -> Result<A2aClient, String> {
    ClientBuilder::new(url)
        .build()
        .map_err(|e| format!("building the client for {url}: {e}"))
}

pub fn user_message(text: &str) -> Message {
    Message {
        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
        role: MessageRole::User,
        parts: vec![Part::text(text)],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

pub fn send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: user_message(text),
        configuration: None,
        metadata: None,
    }
}

/// `send_params` carrying an inline push config for the task the server is
/// about to create.
///
/// The spec's inline form: `taskId` is left empty and the server fills it in
/// from the task it creates, which is the only way to have a webhook
/// registered before the first event fires.
pub fn send_params_with_push(text: &str, webhook: &str) -> MessageSendParams {
    let mut params = send_params(text);
    params.configuration = Some(SendMessageConfiguration {
        task_push_notification_config: Some(TaskPushNotificationConfig {
            tenant: None,
            id: None,
            task_id: None,
            url: webhook.to_owned(),
            token: None,
            authentication: None,
        }),
        ..SendMessageConfiguration::default()
    });
    params
}

/// The task a blocking `SendMessage` produced, or why it did not.
pub fn expect_task(response: SendMessageResponse) -> Result<Task, String> {
    match response {
        SendMessageResponse::Task(task) => Ok(task),
        SendMessageResponse::Message(m) => Err(format!(
            "expected a Task, got a direct Message: {:?}",
            m.text()
        )),
        // `#[non_exhaustive]`: a variant added upstream is a wrong answer
        // here, not a silent match.
        other => Err(format!("expected a Task, got {other:?}")),
    }
}

/// Every text part of `parts`, joined.
// Read by Act 1 and its unit test; a build without `sqlite` keeps it for the
// test alone.
#[cfg_attr(not(feature = "sqlite"), allow(dead_code))]
pub fn text_of(parts: &[Part]) -> String {
    parts
        .iter()
        .filter_map(|p| match &p.content {
            PartContent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// `true` when the server answered with a protocol-level refusal, as opposed
/// to the request never reaching it.
///
/// Positive list on purpose: `ClientError` is `#[non_exhaustive]`, so a new
/// variant falls through to `false` and surfaces as a failing check naming the
/// error, instead of being counted as a refusal.
pub fn is_refusal(error: &ClientError) -> bool {
    matches!(
        error,
        ClientError::Protocol(_)
            | ClientError::AuthRequired { .. }
            | ClientError::UnexpectedStatus { .. }
    )
}

/// A fresh, empty directory under the system temp dir.
///
/// Per run and removed by the caller, so a later run cannot pass on a row an
/// earlier one left behind.
#[cfg(feature = "sqlite")]
pub fn scratch_dir(tag: &str) -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("a2a-resilient-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Stamps a fixed caller identity on every request.
///
/// The rate limiter keys its buckets by caller identity, falling back to a
/// shared `"anonymous"` key. Act 3's shared counter lives in a table that
/// outlives the process, so a per-run identity is what keeps one run's count
/// from bleeding into the next — the same job an auth interceptor does in a
/// real deployment, minus the authentication.
// Constructed by Act 3 and its unit test; a build without `postgres` keeps
// it for the test alone.
#[cfg_attr(not(feature = "postgres"), allow(dead_code))]
pub struct FixedIdentity(pub String);

impl ServerInterceptor for FixedIdentity {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>,
    > {
        Box::pin(async move {
            ctx.set_caller_identity(self.0.clone());
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
}
