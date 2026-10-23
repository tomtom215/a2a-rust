// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A handler with a content signer signs what it serves (ADR 0027): every
//! stream frame, every push, and what `SendMessage`, `GetTask`,
//! `ListTasks`, `CancelTask` and `SubscribeToTask` return. Each item is
//! sent through JSON first, as a binding would, and verified against the
//! signer's public key.
#![cfg(feature = "signing")]

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::handler::{RequestHandler, SendMessageResult};
use a2a_protocol_server::push::PushSender;
use a2a_protocol_server::streaming::{EventQueueReader, EventQueueWriter};
use a2a_protocol_server::{AgentExecutor, EventEmitter, RequestContext};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageRole, Part};
use a2a_protocol_types::params::{
    CancelTaskParams, ListTasksParams, MessageSendParams, SendMessageConfiguration, TaskIdParams,
    TaskQueryParams,
};
use a2a_protocol_types::provenance::{ContentSigner, provenance_of, sign_content, verify_content};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::signing::Jwks;
use a2a_protocol_types::task::{ContextId, Task, TaskState, TaskStatus};
use serde::Serialize;
use serde::de::DeserializeOwned;

fn signer(kid: &str) -> ContentSigner {
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .expect("a key");
    ContentSigner::ed25519(kid, pkcs8.as_ref()).expect("an Ed25519 signer")
}

fn keys(signer: &ContentSigner) -> Jwks {
    Jwks::new(vec![signer.public_jwk()])
}

/// `value` as a binding carries it: to JSON and back.
fn wire<T: Serialize + DeserializeOwned>(value: &T) -> T {
    serde_json::from_str(&serde_json::to_string(value).expect("serialize")).expect("deserialize")
}

fn agent_note(id: &str, text: &str) -> Message {
    Message::new(id, MessageRole::Agent, vec![Part::text(text)])
}

/// "work": a status with a note, an artifact in two chunks, another agent's
/// signed artifact forwarded, and `completed` with a note. "park": an
/// artifact, then `input-required` with a note.
struct Agent {
    forwarded: Artifact,
}

impl AgentExecutor for Agent {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let status = |state, note: Option<Message>| {
                let mut status = TaskStatus::new(state);
                status.message = note;
                StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status,
                    metadata: None,
                })
            };
            if ctx.message.text() == Some("park") {
                EventEmitter::new(ctx, queue)
                    .artifact("draft", vec![Part::text("so far")], None, Some(true))
                    .await?;
                return queue
                    .write(status(
                        TaskState::InputRequired,
                        Some(agent_note("n-3", "which one?")),
                    ))
                    .await;
            }
            queue
                .write(status(
                    TaskState::Working,
                    Some(agent_note("n-1", "starting")),
                ))
                .await?;
            let emit = EventEmitter::new(ctx, queue);
            emit.artifact("report", vec![Part::text("part one")], None, Some(false))
                .await?;
            emit.artifact(
                "report",
                vec![Part::text("part two")],
                Some(true),
                Some(true),
            )
            .await?;
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: self.forwarded.clone(),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;
            queue
                .write(status(
                    TaskState::Completed,
                    Some(agent_note("n-2", "done")),
                ))
                .await
        })
    }
}

/// Records every event it is asked to push.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<StreamResponse>>>);

impl PushSender for Recorder {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.lock().unwrap().push(event.clone());
        Box::pin(async { Ok(()) })
    }
    fn allows_private_urls(&self) -> bool {
        true
    }
}

struct Served {
    handler: RequestHandler,
    ours: Jwks,
    theirs: Jwks,
    pushed: Recorder,
}

fn served(sign: bool) -> Served {
    let ours = signer("ours");
    let other = signer("theirs");
    let mut forwarded = Artifact::new("forwarded", vec![Part::text("from another agent")]);
    sign_content(&mut forwarded, &other).expect("signed by the other agent");
    let pushed = Recorder::default();
    let (our_keys, their_keys) = (keys(&ours), keys(&other));
    let mut builder = RequestHandlerBuilder::new(Agent { forwarded })
        .without_agent_card()
        .with_push_sender(pushed.clone());
    if sign {
        builder = builder.with_content_signer(ours);
    }
    Served {
        handler: builder.build().expect("handler"),
        ours: our_keys,
        theirs: their_keys,
        pushed,
    }
}

fn send(text: &str) -> MessageSendParams {
    let mut params = MessageSendParams::new(Message::user_text("u-1", text));
    params.configuration = Some(SendMessageConfiguration {
        accepted_output_modes: Vec::new(),
        task_push_notification_config: Some(TaskPushNotificationConfig::new(
            "",
            "https://hooks.example/push",
        )),
        history_length: None,
        return_immediately: None,
    });
    params
}

/// Checks the status note and every artifact in `task`: ours verify with
/// our key, the forwarded artifact still with the other agent's, and history
/// is left unsigned. Returns how many items were checked.
fn check_task(s: &Served, task: &Task) -> usize {
    let mut checked = 0;
    if let Some(note) = &task.status.message {
        verify_content(note, &s.ours).expect("the status note verifies");
        checked += 1;
    }
    for artifact in task.artifacts.iter().flatten() {
        check_artifact(s, artifact);
        checked += 1;
    }
    // History holds the caller's messages as they arrived, so none of it
    // is signed: signing it would sign the caller's words as the agent's.
    for message in task.history.iter().flatten() {
        assert_eq!(
            provenance_of(message).unwrap(),
            None,
            "history is not signed"
        );
    }
    checked
}

fn check_artifact(s: &Served, artifact: &Artifact) {
    if artifact.id.0 == "forwarded" {
        verify_content(artifact, &s.theirs).expect("the forwarded signature survives");
        assert!(
            verify_content(artifact, &s.ours).is_err(),
            "and was not replaced"
        );
    } else {
        verify_content(artifact, &s.ours).expect("our artifact verifies");
    }
}

fn check_event(s: &Served, event: &StreamResponse) -> usize {
    match event {
        StreamResponse::Task(task) => check_task(s, task),
        StreamResponse::StatusUpdate(update) => match &update.status.message {
            Some(note) => {
                verify_content(note, &s.ours).expect("a status note verifies");
                1
            }
            None => 0,
        },
        StreamResponse::ArtifactUpdate(update) => {
            check_artifact(s, &update.artifact);
            1
        }
        StreamResponse::Message(message) => {
            verify_content(message, &s.ours).expect("a reply verifies");
            1
        }
        _ => 0,
    }
}

async fn drain(reader: &mut impl EventQueueReader) -> Vec<StreamResponse> {
    let mut events = Vec::new();
    while let Some(frame) = reader.read().await {
        events.push(wire(&frame.expect("a frame").event));
    }
    events
}

#[tokio::test]
async fn every_stream_frame_and_push_verifies() {
    let s = served(true);
    let Ok(SendMessageResult::Stream(mut reader)) =
        s.handler.on_send_message(send("work"), true, None).await
    else {
        panic!("a stream");
    };
    let events = drain(&mut reader).await;
    let checked: usize = events.iter().map(|e| check_event(&s, e)).sum();
    // Two notes, two chunks, the forwarded artifact, at least.
    assert!(checked >= 5, "checked {checked} of {events:?}");

    // Each chunk verifies as the chunk it is.
    let chunks: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            StreamResponse::ArtifactUpdate(u) if u.artifact.id.0 == "report" => Some(&u.artifact),
            _ => None,
        })
        .collect();
    assert_eq!(chunks.len(), 2);

    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while s.pushed.0.lock().unwrap().len() < 5 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the pushes arrive");
    let pushed = s.pushed.0.lock().unwrap().clone();
    let checked: usize = pushed.iter().map(|e| check_event(&s, &wire(e))).sum();
    assert!(checked >= 5, "checked {checked} of {pushed:?}");
}

#[tokio::test]
async fn what_unary_calls_return_verifies_and_the_stored_artifact_verifies_whole() {
    let s = served(true);
    let Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) =
        s.handler.on_send_message(send("work"), false, None).await
    else {
        panic!("a task");
    };
    let task = wire(&task);
    assert!(check_task(&s, &task) >= 3);

    let got = s
        .handler
        .on_get_task(
            TaskQueryParams {
                tenant: None,
                id: task.id.0.clone(),
                history_length: None,
            },
            None,
        )
        .await
        .expect("get");
    let got = wire(&got);
    check_task(&s, &got);
    let report = got
        .artifacts
        .iter()
        .flatten()
        .find(|a| a.id.0 == "report")
        .expect("the merged report");
    assert_eq!(report.parts.len(), 2, "both chunks, merged");
    verify_content(report, &s.ours).expect("the merged artifact verifies whole");

    let page = s
        .handler
        .on_list_tasks(
            ListTasksParams {
                include_artifacts: Some(true),
                ..ListTasksParams::default()
            },
            None,
        )
        .await
        .expect("list");
    let page = wire(&page);
    assert_eq!(page.tasks.len(), 1);
    assert!(check_task(&s, &page.tasks[0]) >= 3);
}

#[tokio::test]
async fn a_cancel_and_a_subscription_verify() {
    let s = served(true);
    let Ok(SendMessageResult::Response(SendMessageResponse::Task(parked))) =
        s.handler.on_send_message(send("park"), false, None).await
    else {
        panic!("a task");
    };
    assert_eq!(parked.status.state, TaskState::InputRequired);
    assert!(check_task(&s, &wire(&parked)) >= 2);

    let mut reader = s
        .handler
        .on_resubscribe(
            TaskIdParams {
                tenant: None,
                id: parked.id.0.clone(),
            },
            None,
        )
        .await
        .expect("subscribe");
    // The snapshot is the first frame; the task waits, so read just that.
    let first = wire(&reader.read().await.expect("a frame").expect("ok").event);
    let StreamResponse::Task(snapshot) = &first else {
        panic!("a snapshot first: {first:?}");
    };
    assert!(check_task(&s, snapshot) >= 2);
    drop(reader);

    let canceled = s
        .handler
        .on_cancel_task(
            CancelTaskParams {
                tenant: None,
                id: parked.id.0.clone(),
                metadata: None,
            },
            None,
        )
        .await
        .expect("cancel");
    assert_eq!(canceled.status.state, TaskState::Canceled);
    assert!(check_task(&s, &wire(&canceled)) >= 1, "the draft at least");
}

#[tokio::test]
async fn without_a_signer_nothing_is_signed() {
    let s = served(false);
    let Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) =
        s.handler.on_send_message(send("work"), false, None).await
    else {
        panic!("a task");
    };
    for artifact in task.artifacts.iter().flatten() {
        let signed = provenance_of(artifact)
            .unwrap()
            .is_some_and(|p| p.signature.is_some());
        assert_eq!(signed, artifact.id.0 == "forwarded", "{}", artifact.id.0);
    }
    let note = task.status.message.as_ref().expect("a note");
    assert_eq!(provenance_of(note).unwrap(), None);
}

/// A caller may send a message with role `agent`; it is stored in history as
/// it arrived, and must never come back signed with the server's key (found
/// by review: it did, 2026-10-22).
#[tokio::test]
async fn a_callers_agent_role_message_is_never_signed_as_the_agents() {
    let s = served(true);
    let mut params = send("park");
    params.message.role = MessageRole::Agent;
    params.configuration.as_mut().unwrap().history_length = Some(10);
    let Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) =
        s.handler.on_send_message(params, false, None).await
    else {
        panic!("a task");
    };
    let task = wire(&task);
    let history = task.history.as_ref().expect("history");
    assert!(
        history.iter().any(|m| m.text() == Some("park")),
        "{history:?}"
    );
    check_task(&s, &task);
}
