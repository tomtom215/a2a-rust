// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Terminal truth, end to end, with a **real open-source model** as the agent
//! (adversarial review R1, 2026-11-12).
//!
//! The executor is the tool loop over Qwen3-1.7B served by `llama-server`; the
//! handler is the real one; the client is a streaming reader of the real queue.
//! The task store rejects only the `Completed` write and still answers reads,
//! which is the review's acceptance scenario. What the stream says, what
//! `GetTask` says and what a webhook (through the push outbox, to a real
//! socket) is sent must agree, whatever the model answers:
//!
//! ```bash
//! llama-server -m Qwen3-1.7B-Q4_K_M.gguf --port 11434 --alias qwen3:1.7b --jinja -c 4096 -np 1
//! A2A_LIVE_MODEL_URL=http://127.0.0.1:11434/v1 A2A_LIVE_MODEL=qwen3:1.7b \
//!   cargo test -p a2a-protocol-server --features tool-loop \
//!   --test terminal_truth_live -- --ignored --test-threads=1 --nocapture
//! ```

#![cfg(feature = "tool-loop")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_server::TerminalConfirmation;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::handler::{HandlerLimits, RequestHandler, SendMessageResult};
use a2a_protocol_server::push::{HttpPushSender, InMemoryPushOutbox, PushOutbox, PushRetryPolicy};
use a2a_protocol_server::store::{InMemoryTaskStore, TaskStore};
use a2a_protocol_server::streaming::EventQueueReader;
use a2a_protocol_server::tool_loop::{
    OpenAiChatModel, Tool, ToolError, ToolInvocation, ToolLoop, ToolLoopExecutor, ToolSet,
};
use a2a_protocol_types::Message;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface};
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::params::{
    ListTasksParams, MessageSendParams, SendMessageConfiguration, TaskQueryParams,
};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId, TaskState};
use serde_json::{Value, json};

fn model() -> OpenAiChatModel {
    let base = std::env::var("A2A_LIVE_MODEL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:11434/v1".to_owned());
    let name = std::env::var("A2A_LIVE_MODEL").unwrap_or_else(|_| "qwen3:1.7b".to_owned());
    let mut extra = serde_json::Map::new();
    extra.insert(
        "chat_template_kwargs".to_owned(),
        json!({"enable_thinking": false}),
    );
    extra.insert("max_tokens".to_owned(), json!(120));
    extra.insert("temperature".to_owned(), json!(0));
    extra.insert("seed".to_owned(), json!(7));
    OpenAiChatModel::new(&base, name)
        .expect("a model client")
        .with_timeout(Duration::from_secs(300))
        .with_extra_body(extra)
}

struct Balance;
impl Tool for Balance {
    fn name(&self) -> &str {
        "get_balance"
    }
    fn description(&self) -> &'static str {
        "Looks up the balance of an account."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"account": {"type": "string"}}})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn call<'a>(
        &'a self,
        _invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        Box::pin(async { Ok("balance: 1250 EUR".to_owned()) })
    }
}

/// A store that rejects the `Completed` write, while it answers every read:
/// the review's acceptance scenario.
struct RejectsCompleted {
    inner: InMemoryTaskStore,
    rejected: Arc<AtomicUsize>,
}

impl RejectsCompleted {
    fn refuse(&self, task: &Task) -> A2aResult<()> {
        if task.status.state == TaskState::Completed {
            self.rejected.fetch_add(1, Ordering::SeqCst);
            return Err(A2aError::internal("the store rejected the completion"));
        }
        Ok(())
    }
}

type F<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

impl TaskStore for RejectsCompleted {
    fn save<'a>(&'a self, task: &'a Task) -> F<'a, ()> {
        Box::pin(async move {
            self.refuse(task)?;
            self.inner.save(task).await
        })
    }
    fn save_status_delta<'a>(&'a self, task: &'a Task) -> F<'a, ()> {
        Box::pin(async move {
            self.refuse(task)?;
            self.inner.save_status_delta(task).await
        })
    }
    fn try_save_status_delta<'a>(&'a self, task: &'a Task) -> F<'a, bool> {
        Box::pin(async move {
            self.refuse(task)?;
            self.inner.try_save_status_delta(task).await
        })
    }
    fn get<'a>(&'a self, id: &'a TaskId) -> F<'a, Option<Task>> {
        self.inner.get(id)
    }
    fn list<'a>(&'a self, params: &'a ListTasksParams) -> F<'a, TaskListResponse> {
        self.inner.list(params)
    }
    fn insert_if_absent<'a>(&'a self, task: &'a Task) -> F<'a, bool> {
        self.inner.insert_if_absent(task)
    }
    fn delete<'a>(&'a self, id: &'a TaskId) -> F<'a, ()> {
        self.inner.delete(id)
    }
}

fn card() -> AgentCard {
    AgentCard::new(
        "Live Agent",
        "1.0.0",
        AgentInterface::new("https://agent.example.com/rpc", "JSONRPC")
            .with_protocol_version("1.0.0"),
    )
    .with_description("A real model behind the tool loop")
    .with_input_modes(vec!["text/plain"])
    .with_output_modes(vec!["text/plain"])
    .with_skill(a2a_protocol_types::AgentSkill::new("t", "T", "t").with_tags(["t"]))
    .with_capabilities(
        AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true),
    )
}

/// A webhook on loopback that records every body it is sent and answers `200`.
async fn webhook() -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = Arc::clone(&bodies);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let n = stream.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if body.len() >= length {
                            break;
                        }
                    }
                }
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf).into_owned());
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            });
        }
    });
    (url, bodies)
}

/// What a client saw on the stream, and what the server holds afterwards.
struct Observed {
    /// Every state a status frame showed, in order.
    stream_states: Vec<TaskState>,
    /// The stream ended with an error rather than closing cleanly.
    stream_error: Option<String>,
    /// The answer artifact's text, if one was sent.
    answer: Option<String>,
    /// `GetTask` afterwards.
    stored: TaskState,
    rejected: usize,
    /// Whether any webhook delivery carried the `Completed` state.
    webhook_said_completed: bool,
    /// How many webhook deliveries arrived.
    webhooks: usize,
}

async fn run(store_rejects_completion: bool, limits: HandlerLimits) -> Observed {
    let rejected = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolSet::new();
    tools.add(Balance).unwrap();
    let executor = ToolLoopExecutor::new(ToolLoop::new(model(), tools).with_max_turns(4));
    let (hook, delivered) = webhook().await;
    let mut builder = RequestHandlerBuilder::new(executor)
        .with_agent_card(card())
        .with_push_sender(
            HttpPushSender::new()
                .allow_private_urls()
                .with_retry_policy(PushRetryPolicy::default().with_max_attempts(1)),
        )
        .with_push_outbox(Arc::new(InMemoryPushOutbox::new()) as Arc<dyn PushOutbox>)
        .with_handler_limits(limits);
    builder = if store_rejects_completion {
        builder.with_task_store(RejectsCompleted {
            inner: InMemoryTaskStore::new(),
            rejected: Arc::clone(&rejected),
        })
    } else {
        builder.with_task_store(InMemoryTaskStore::new())
    };
    let handler: RequestHandler = builder.build().unwrap();

    let mut configuration = SendMessageConfiguration::new();
    configuration.task_push_notification_config = Some(TaskPushNotificationConfig::for_url(hook));
    let mut reader = match handler
        .on_send_message(
            MessageSendParams::new(Message::user_text(
                "m-1",
                "What is the balance of account 4417? Use the tool, then answer in one sentence.",
            ))
            .with_configuration(configuration),
            true,
            None,
        )
        .await
    {
        Ok(SendMessageResult::Stream(reader)) => reader,
        other => panic!("a stream: {other:?}"),
    };
    let (mut stream_states, mut stream_error, mut answer, mut task_id) =
        (Vec::new(), None, None, None);
    loop {
        let next = tokio::time::timeout(Duration::from_secs(300), reader.read())
            .await
            .expect("the stream must end");
        match next {
            None => break,
            Some(Err(e)) => {
                stream_error = Some(format!("{e:?}"));
                break;
            }
            Some(Ok(frame)) => match frame.event {
                StreamResponse::StatusUpdate(u) => {
                    task_id = Some(u.task_id.clone());
                    stream_states.push(u.status.state);
                }
                StreamResponse::ArtifactUpdate(u) => {
                    task_id = Some(u.task_id.clone());
                    if u.artifact.id.0 == "answer" {
                        answer = u
                            .artifact
                            .parts
                            .first()
                            .and_then(|p| p.text_content())
                            .map(str::to_owned);
                    }
                }
                StreamResponse::Task(t) => task_id = Some(t.id),
                _ => {}
            },
        }
    }
    // Let the background processor finish settling, and the push worker deliver.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let id = task_id.expect("the stream carried a task id");
    let stored = handler
        .on_get_task(TaskQueryParams::new(id.0.clone()), None)
        .await
        .expect("GetTask answers")
        .status
        .state;
    let delivered = delivered.lock().unwrap();
    Observed {
        stream_states,
        stream_error,
        answer,
        stored,
        rejected: rejected.load(Ordering::SeqCst),
        webhook_said_completed: delivered.iter().any(|b| b.contains("TASK_STATE_COMPLETED")),
        webhooks: delivered.len(),
    }
}

/// Control: the store takes every write. Stream and store agree on `Completed`
/// and the model's answer arrived.
#[tokio::test]
#[ignore = "needs a llama-server (A2A_LIVE_MODEL_URL)"]
async fn a_healthy_store_streams_and_stores_completed() {
    let seen = run(false, HandlerLimits::default()).await;
    println!(
        "states {:?}, answer {:?}, stored {:?}, webhooks {} (Completed among them: {})",
        seen.stream_states, seen.answer, seen.stored, seen.webhooks, seen.webhook_said_completed
    );
    assert_eq!(seen.stream_states.last(), Some(&TaskState::Completed));
    assert_eq!(seen.stored, TaskState::Completed);
    assert!(seen.stream_error.is_none(), "{:?}", seen.stream_error);
    assert!(
        seen.answer.as_deref().is_some_and(|a| !a.is_empty()),
        "the model gave no answer"
    );
    assert!(
        seen.webhook_said_completed,
        "the webhook was never told Completed ({} deliveries)",
        seen.webhooks
    );
}

/// The review's scenario: the `Completed` write is rejected and reads work.
/// By default the stream must not say `Completed`, and agrees with the store.
#[tokio::test]
#[ignore = "needs a llama-server (A2A_LIVE_MODEL_URL)"]
async fn a_rejected_completion_is_never_streamed_as_completed() {
    let seen = run(true, HandlerLimits::default()).await;
    println!(
        "states {:?}, error {:?}, answer {:?}, stored {:?}, completion writes rejected {}, webhooks {} (Completed among them: {})",
        seen.stream_states,
        seen.stream_error,
        seen.answer,
        seen.stored,
        seen.rejected,
        seen.webhooks,
        seen.webhook_said_completed
    );
    assert!(
        seen.rejected > 0,
        "the completion was never written, so nothing was tested"
    );
    assert!(
        !seen.stream_states.contains(&TaskState::Completed),
        "the stream told the client Completed: {:?}",
        seen.stream_states
    );
    assert!(
        seen.stream_error.is_some(),
        "the client must be told the completion was not confirmed"
    );
    assert_ne!(
        seen.stored,
        TaskState::Completed,
        "the store holds what the stream did not claim"
    );
    assert!(
        !seen.webhook_said_completed,
        "a webhook was told Completed for a task the store does not hold as completed"
    );
}

/// The earlier behaviour, kept as an explicit choice: the same run under
/// `BestEffort` shows the disagreement the default removes.
#[tokio::test]
#[ignore = "needs a llama-server (A2A_LIVE_MODEL_URL)"]
async fn best_effort_keeps_the_disagreement_the_default_removes() {
    let limits =
        HandlerLimits::default().with_terminal_confirmation(TerminalConfirmation::BestEffort);
    let seen = run(true, limits).await;
    println!(
        "BestEffort: states {:?}, stored {:?}, rejected {}, webhooks {} (Completed among them: {})",
        seen.stream_states, seen.stored, seen.rejected, seen.webhooks, seen.webhook_said_completed
    );
    assert!(seen.rejected > 0);
    assert_eq!(seen.stream_states.last(), Some(&TaskState::Completed));
    assert_ne!(seen.stored, TaskState::Completed);
}
