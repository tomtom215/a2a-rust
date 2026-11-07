// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A delegation continues a child whose turn was handed back, within its
//! policy (ADR 0054). The transport is scripted and the clock is paused.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageRole, Part};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskStatus};

use super::*;
use crate::ClientBuilder;
use crate::delegation::{DelegationOptions, Outcome, ResumePolicy};
use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

/// Answers the n-th streaming send with the n-th script (the last one once
/// they run out), records every send's params, and answers `CancelTask` with
/// a canceled task.
struct Scripted {
    scripts: Vec<Vec<StreamResponse>>,
    sends: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl Transport for Scripted {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        let task = Task::new("t", "c", TaskStatus::new(TaskState::Canceled));
        Box::pin(async move { serde_json::to_value(task).map_err(ClientError::Serialization) })
    }

    fn send_streaming_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(async move {
            if method != "SendStreamingMessage" {
                return Err(ClientError::Transport("no subscriptions here".into()));
            }
            let n = {
                let mut sends = self.sends.lock().unwrap();
                sends.push(params);
                (sends.len() - 1).min(self.scripts.len() - 1)
            };
            let (tx, rx) = tokio::sync::mpsc::channel(16);
            for ev in self.scripts[n].clone() {
                tx.try_send(Ok(ev)).unwrap();
            }
            Ok(EventStream::from_event_channel(rx))
        })
    }
}

fn status(state: TaskState, handed_back: bool) -> StreamResponse {
    let mut status = TaskStatus::new(state);
    if handed_back {
        let mut note = Message::new(
            a2a_protocol_types::message::MessageId::new("note"),
            MessageRole::Agent,
            vec![Part::text("handed back")],
        );
        note.metadata = Some(serde_json::json!({ HAND_OFF_METADATA_KEY: true }));
        status.message = Some(note);
    }
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t"),
        ContextId::new("c"),
        status,
    ))
}

async fn run(
    scripts: Vec<Vec<StreamResponse>>,
    options: DelegationOptions,
) -> (Outcome, Vec<serde_json::Value>, Duration) {
    let sends = Arc::new(Mutex::new(Vec::new()));
    let client = Arc::new(
        ClientBuilder::new("http://stub.invalid")
            .with_custom_transport(Scripted {
                scripts,
                sends: Arc::clone(&sends),
            })
            .build()
            .unwrap(),
    );
    let message = Message::new(
        a2a_protocol_types::message::MessageId::new("first"),
        MessageRole::User,
        vec![Part::text("do it")],
    );
    let started = tokio::time::Instant::now();
    let mut params = MessageSendParams::new(message);
    params.tenant = Some("acme".to_owned());
    let child = Delegation::start_with(client, params, options)
        .await
        .unwrap();
    let outcome = child.wait(std::future::pending()).await.outcome;
    let sends = sends.lock().unwrap().clone();
    (outcome, sends, started.elapsed())
}

fn message_of(send: &serde_json::Value) -> &serde_json::Value {
    &send["message"]
}

/// A handed-back child is sent its message again, with a new message id, to
/// its task and context, after the first wait; it then completes.
#[tokio::test(start_paused = true)]
async fn a_handed_back_child_is_sent_again_and_completes() {
    let (outcome, sends, took) = Box::pin(run(
        vec![
            vec![
                status(TaskState::Working, false),
                status(TaskState::Working, true),
            ],
            vec![status(TaskState::Completed, false)],
        ],
        DelegationOptions::new(),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Completed(_)), "{outcome:?}");
    assert_eq!(sends.len(), 2);
    let (first, again) = (message_of(&sends[0]), message_of(&sends[1]));
    assert_eq!(again["taskId"], "t");
    assert_eq!(again["contextId"], "c");
    assert_eq!(
        (&sends[0]["tenant"], &sends[1]["tenant"]),
        (&serde_json::json!("acme"), &serde_json::json!("acme")),
        "the continuation goes to the child's tenant"
    );
    assert_ne!(again["messageId"], first["messageId"]);
    assert_eq!(again["parts"], first["parts"]);
    assert_eq!(took, Duration::from_secs(1), "the first wait");
}

/// The bound holds: two continuations, after 1 s and 2 s, and then the
/// child is lost (no resume here) and cancelled.
#[tokio::test(start_paused = true)]
async fn continuations_stop_at_the_bound() {
    let (outcome, sends, took) = Box::pin(run(
        vec![vec![status(TaskState::Working, true)]],
        DelegationOptions::new()
            .with_hand_off(HandOffPolicy::new(
                2,
                Duration::from_secs(1),
                Duration::from_secs(8),
            ))
            .with_resume(ResumePolicy::none()),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Lost(_)), "{outcome:?}");
    assert_eq!(sends.len(), 3, "the start and two continuations");
    assert_eq!(took, Duration::from_secs(3));
}

/// `HandOffPolicy::never` opts out: the mark is a lost stream.
#[tokio::test(start_paused = true)]
async fn never_does_not_continue() {
    let (outcome, sends, took) = Box::pin(run(
        vec![vec![status(TaskState::Working, true)]],
        DelegationOptions::new()
            .with_hand_off(HandOffPolicy::never())
            .with_resume(ResumePolicy::none()),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Lost(_)), "{outcome:?}");
    assert_eq!(sends.len(), 1);
    assert_eq!(took, Duration::ZERO);
}

/// A child that is working without the mark is not continued.
#[tokio::test(start_paused = true)]
async fn an_unmarked_end_is_resumed_not_continued() {
    let (outcome, sends, _) = Box::pin(run(
        vec![vec![status(TaskState::Working, false)]],
        DelegationOptions::new().with_resume(ResumePolicy::none()),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Lost(_)), "{outcome:?}");
    assert_eq!(sends.len(), 1);
}

#[test]
fn the_waits_double_up_to_the_longest() {
    let p = HandOffPolicy::default();
    assert_eq!(
        p,
        HandOffPolicy::new(3, Duration::from_secs(1), Duration::from_secs(8))
    );
    let waits: Vec<_> = (0..6).map(|n| p.wait(n).as_secs()).collect();
    assert_eq!(waits, [1, 2, 4, 8, 8, 8]);
}

#[test]
fn only_a_working_marked_status_is_handed_back() {
    let marked = |state, mark| match status(state, mark) {
        StreamResponse::StatusUpdate(u) => handed_back(&u.status),
        _ => unreachable!(),
    };
    assert!(marked(TaskState::Working, true));
    assert!(!marked(TaskState::Working, false));
    assert!(!marked(TaskState::Completed, true));
}
