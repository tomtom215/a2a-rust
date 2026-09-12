// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The bridge: a [`rig_core`] completion model behind an A2A [`AgentExecutor`].

use std::pin::Pin;

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::executor_helpers::{EventEmitter, boxed_future};
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::message::{Part, PartContent};
use a2a_protocol_types::task::TaskState;
use rig_core::completion::{AssistantContent, CompletionModel, Message};

/// Default id for the artifact carrying the model's answer.
const DEFAULT_ARTIFACT_ID: &str = "rig-response";

/// Serves a [`rig_core`] completion model as an A2A agent.
///
/// One A2A message becomes one completion request: the message's first text part
/// is the user turn, and the model's text blocks are concatenated into a single
/// artifact. There is no tool loop and no conversation history — that belongs to
/// rig's own agent layer, and wiring it in is the caller's business, not this
/// bridge's.
///
/// Cancellation is honoured: `execute` races the completion against
/// [`RequestContext::cancellation_token`], so a `tasks/cancel` stops waiting on
/// the provider rather than running to completion and discarding the answer.
///
/// # Example
///
/// ```no_run
/// use a2a_protocol_server::builder::RequestHandlerBuilder;
/// use a2a_rig::{RigExecutor, agent_card};
/// use rig_core::client::CompletionClient;
/// use rig_core::providers::openai;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = openai::CompletionsClient::builder()
///     .api_key(&std::env::var("OPENAI_API_KEY")?)
///     .build()?;
///
/// let executor = RigExecutor::new(client.completion_model("gpt-4o-mini"))
///     .with_preamble("You are concise.");
///
/// let card = agent_card("https://agent.example.com", "gpt-4o-mini").build();
/// let handler = RequestHandlerBuilder::new(executor).with_agent_card(card).build()?;
/// # let _ = handler;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct RigExecutor<M> {
    model: M,
    preamble: Option<String>,
    artifact_id: String,
}

// `Clone` is required by rig: `CompletionModel::completion_request` takes the
// model by value, so each request needs its own copy. Every provider model rig
// ships is `Clone`.
impl<M: CompletionModel + Clone> RigExecutor<M> {
    /// Wraps `model` with no preamble and the default artifact name.
    #[must_use]
    pub fn new(model: M) -> Self {
        Self {
            model,
            preamble: None,
            artifact_id: DEFAULT_ARTIFACT_ID.to_owned(),
        }
    }

    /// Sets the system preamble sent with every completion request.
    #[must_use]
    pub fn with_preamble(mut self, preamble: impl Into<String>) -> Self {
        self.preamble = Some(preamble.into());
        self
    }

    /// Overrides the `artifactId` the answer is emitted under.
    ///
    /// Defaults to `"rig-response"`. This is the A2A artifact *identifier*, which
    /// is what clients correlate chunks on — not a display name.
    #[must_use]
    pub fn with_artifact_id(mut self, id: impl Into<String>) -> Self {
        self.artifact_id = id.into();
        self
    }

    /// Sends one completion request and returns the concatenated text blocks.
    ///
    /// Non-text content in the response (tool calls, reasoning) is skipped: this
    /// bridge is single-turn, so there is no loop to hand a tool call to.
    async fn complete(&self, text: &str) -> Result<String, String> {
        let mut request = self.model.completion_request(text);
        if let Some(preamble) = &self.preamble {
            // `CompletionRequestBuilder::preamble` is documented as a legacy API
            // that funnels into a leading system message anyway; adding it
            // directly is the canonical form rig's own docs point at.
            request = request.message(Message::system(preamble.clone()));
        }
        let response = request.send().await.map_err(|e| e.to_string())?;
        Ok(response
            .choice
            .iter()
            .filter_map(|content| match content {
                AssistantContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect())
    }
}

impl<M> AgentExecutor for RigExecutor<M>
where
    M: CompletionModel + Clone + 'static,
{
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        boxed_future(async move {
            let emit = EventEmitter::new(ctx, queue);

            // A message with no text part is a client error. Prompting the model
            // with "" would bill a request to answer nothing.
            let prompt = ctx
                .message
                .parts
                .iter()
                .find_map(|part| match &part.content {
                    PartContent::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .ok_or_else(|| A2aError::invalid_params("message contains no text part"))?;

            emit.status(TaskState::Working).await?;

            // Race the provider against cancellation. Dropping the future is what
            // releases the in-flight request, so there is nothing else to clean up
            // and `cancel` is left at its default.
            let answer = tokio::select! {
                biased;
                () = ctx.cancellation_token.cancelled() => {
                    emit.status(TaskState::Canceled).await?;
                    return Ok(());
                }
                result = self.complete(prompt) => result,
            };

            match answer {
                Ok(text) => {
                    emit.artifact(&self.artifact_id, vec![Part::text(&text)], None, Some(true))
                        .await?;
                    // Re-check before the terminal transition: a cancel that
                    // arrived while the artifact was being written must not be
                    // overwritten by `Completed`, which would be an invalid
                    // transition away from a terminal state.
                    if emit.is_cancelled() {
                        emit.status(TaskState::Canceled).await?;
                    } else {
                        emit.status(TaskState::Completed).await?;
                    }
                    Ok(())
                }
                Err(message) => Err(A2aError::internal(format!(
                    "rig completion failed: {message}"
                ))),
            }
        })
    }
}
