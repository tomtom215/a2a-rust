// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! COR-06: a start always keys its message, and takes a key it added off
//! again only on the typed refusal of a server that cannot honour keys.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::idempotency::{self, keys_unsupported};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};
use a2a_protocol_types::{A2aError, ErrorCode, MessageSendParams};

use crate::ClientBuilder;
use crate::delegation::Delegation;
use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

/// Answers the n-th streaming send with the n-th answer (the last once they
/// run out) and records each send's message.
struct Answers {
    answers: Vec<Result<(), A2aError>>,
    sent: Arc<Mutex<Vec<Message>>>,
}

impl Transport for Answers {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        Box::pin(async { Err(ClientError::Transport("not scripted".into())) })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        params: serde_json::Value,
        _headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(async move {
            let p: MessageSendParams = serde_json::from_value(params).unwrap();
            let n = {
                let mut sent = self.sent.lock().unwrap();
                sent.push(p.message);
                (sent.len() - 1).min(self.answers.len() - 1)
            };
            self.answers[n].clone().map_err(ClientError::Protocol)?;
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            let working = TaskStatusUpdateEvent::new(
                TaskId::new("t"),
                ContextId::new("c"),
                TaskStatus::new(TaskState::Working),
            );
            tx.try_send(Ok(StreamResponse::StatusUpdate(working)))
                .unwrap();
            Ok(EventStream::from_event_channel(rx))
        })
    }
}

async fn start(
    answers: Vec<Result<(), A2aError>>,
    key: Option<&str>,
) -> (ClientResult<Delegation>, Vec<Message>) {
    let sent = Arc::new(Mutex::new(Vec::new()));
    // A client that knows nothing of its agent: no card, no assertion.
    let client = Arc::new(
        ClientBuilder::new("http://stub.invalid")
            .with_custom_transport(Answers {
                answers,
                sent: Arc::clone(&sent),
            })
            .build()
            .unwrap(),
    );
    assert!(!client.honours_idempotency());
    let mut message = Message::new(
        MessageId::new("m"),
        MessageRole::User,
        vec![Part::text("go")],
    );
    if let Some(key) = key {
        idempotency::set_key(&mut message, key).unwrap();
    }
    let started = Delegation::start(client, MessageSendParams::new(message)).await;
    let sent = sent.lock().unwrap().clone();
    (started, sent)
}

fn key_of(m: &Message) -> Option<String> {
    idempotency::key_of(m).unwrap().map(str::to_owned)
}

/// Before DEC-08's change, a client not known to honour keys sent none.
#[tokio::test]
async fn a_start_keys_its_message_whatever_it_knows_of_the_agent() {
    let (started, sent) = start(vec![Ok(())], None).await;
    let child = started.expect("started");
    assert_eq!(sent.len(), 1);
    let key = key_of(&sent[0]).expect("the message is keyed");
    assert_eq!(child.idempotency_key(), Some(key.as_str()));
}

#[tokio::test]
async fn a_key_it_added_is_taken_off_once_on_the_typed_refusal() {
    let (started, sent) = start(vec![Err(keys_unsupported("no keys")), Ok(())], None).await;
    let child = started.expect("started without the key");
    assert_eq!(sent.len(), 2, "one resend");
    assert!(key_of(&sent[0]).is_some());
    assert_eq!(key_of(&sent[1]), None);
    assert!(!idempotency::declares_extension(&sent[1]));
    assert_eq!(child.idempotency_key(), None);
}

#[tokio::test]
async fn the_callers_key_is_never_taken_off_and_any_other_refusal_is_final() {
    let own = "c".repeat(64);
    let (started, sent) = start(vec![Err(keys_unsupported("no keys")), Ok(())], Some(&own)).await;
    assert!(matches!(started, Err(ClientError::Protocol(ref e)) if idempotency::refuses_keys(e)));
    assert_eq!(sent.len(), 1);
    let untyped = A2aError::new(ErrorCode::UnsupportedOperation, "cannot honour keys");
    let (started, sent) = start(vec![Err(untyped), Ok(())], None).await;
    assert!(started.is_err(), "only the typed refusal is resent");
    assert_eq!(sent.len(), 1);
}
