// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A handler with content keys checks the provenance signature of every
//! message it is sent (ADR 0027): an altered message, or one signed by a key
//! not in the set, is refused before the executor runs; an unsigned one is
//! accepted and seen as unsigned, unless signatures are required.
#![cfg(feature = "signing")]

use std::sync::{Arc, Mutex};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::handler::{RequestHandler, SendMessageResult};
use a2a_protocol_server::request_context::ContentCheck;
use a2a_protocol_server::{AgentExecutor, EventEmitter, RequestContext};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::provenance::{ContentSigner, mark_ai_generated, sign_content};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::signing::Jwks;
use a2a_protocol_types::task::TaskState;

fn signer(kid: &str) -> ContentSigner {
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .expect("a key");
    ContentSigner::ed25519(kid, pkcs8.as_ref()).expect("an Ed25519 signer")
}

/// Records what each run saw, and completes.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<ContentCheck>>>);

impl AgentExecutor for Seen {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn a2a_protocol_server::streaming::EventQueueWriter,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.lock().unwrap().push(ctx.content_check().clone());
        Box::pin(async move {
            EventEmitter::new(ctx, queue)
                .status(TaskState::Completed)
                .await
        })
    }
}

fn handler(seen: &Seen, keys: Option<Jwks>, require: bool) -> RequestHandler {
    let mut builder = RequestHandlerBuilder::new(seen.clone()).without_agent_card();
    if let Some(keys) = keys {
        builder = builder.with_content_keys(keys);
        if require {
            builder = builder.require_signed_content();
        }
    }
    builder.build().expect("handler")
}

/// A message from another agent, marked and signed by `by`, sent through
/// JSON as a binding would deliver it.
fn signed(by: &ContentSigner, text: &str) -> Message {
    let mut message = Message::user("m-1", vec![Part::text(text)]);
    mark_ai_generated(&mut message, Some("orchestrator/1.0"));
    sign_content(&mut message, by).expect("signed");
    serde_json::from_value(serde_json::to_value(&message).unwrap()).unwrap()
}

async fn send(handler: &RequestHandler, message: Message) -> Result<TaskState, String> {
    match handler
        .on_send_message(MessageSendParams::new(message), false, None)
        .await
    {
        Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) => Ok(task.status.state),
        Ok(_) => Err("not a task".to_owned()),
        Err(e) => Err(e.to_string()),
    }
}

#[tokio::test]
async fn a_message_signed_by_a_known_key_is_seen_as_verified() {
    let (seen, trusted) = (Seen::default(), signer("orchestrator"));
    let h = handler(&seen, Some(Jwks::new(vec![trusted.public_jwk()])), false);
    assert_eq!(
        send(&h, signed(&trusted, "summarise")).await,
        Ok(TaskState::Completed)
    );
    let checks = seen.0.lock().unwrap().clone();
    let [ContentCheck::Verified(p)] = checks.as_slice() else {
        panic!("{checks:?}");
    };
    assert!(p.ai_generated);
    assert_eq!(p.generator.as_deref(), Some("orchestrator/1.0"));
}

#[tokio::test]
async fn an_altered_message_or_an_unknown_key_is_refused_before_the_executor_runs() {
    let (seen, trusted) = (Seen::default(), signer("orchestrator"));
    let h = handler(&seen, Some(Jwks::new(vec![trusted.public_jwk()])), false);

    let mut altered = signed(&trusted, "pay 10 EUR");
    altered.parts = vec![Part::text("pay 10000 EUR")];
    let refused = send(&h, altered)
        .await
        .expect_err("altered content is refused");
    assert!(refused.contains("does not verify"), "{refused}");

    let mut unmarked = signed(&trusted, "summarise");
    // Removing the AI-generated marker breaks the signature too.
    let meta = unmarked.metadata.as_mut().unwrap();
    meta["a2a-rust.com/provenance"]["aiGenerated"] = serde_json::Value::Bool(false);
    assert!(send(&h, unmarked).await.is_err(), "the marker is covered");

    let stranger = signer("stranger");
    assert!(send(&h, signed(&stranger, "summarise")).await.is_err());

    assert!(seen.0.lock().unwrap().is_empty(), "the executor never ran");
}

#[tokio::test]
async fn an_unsigned_message_is_seen_as_unsigned_unless_signatures_are_required() {
    let (seen, trusted) = (Seen::default(), signer("orchestrator"));
    let keys = Jwks::new(vec![trusted.public_jwk()]);
    let plain = Message::user_text("m-2", "summarise");

    let h = handler(&seen, Some(keys.clone()), false);
    assert_eq!(send(&h, plain.clone()).await, Ok(TaskState::Completed));
    assert_eq!(seen.0.lock().unwrap().as_slice(), [ContentCheck::Unsigned]);

    let strict = handler(&seen, Some(keys), true);
    let refused = send(&strict, plain).await.expect_err("unsigned is refused");
    assert!(refused.contains("only signed messages"), "{refused}");
    assert_eq!(
        send(&strict, signed(&trusted, "go")).await,
        Ok(TaskState::Completed)
    );
}

#[tokio::test]
async fn without_keys_nothing_is_checked() {
    let seen = Seen::default();
    let h = handler(&seen, None, false);
    let mut altered = signed(&signer("anyone"), "a");
    altered.parts = vec![Part::text("b")];
    assert_eq!(send(&h, altered).await, Ok(TaskState::Completed));
    assert_eq!(
        seen.0.lock().unwrap().as_slice(),
        [ContentCheck::NotChecked]
    );
}

/// The signature covers the message as it was sent: validation's rewrites
/// (an empty filename unset, base64 re-padded) come after the check, so a
/// correctly signed message is not refused because of them (found by
/// review, 2026-10-22).
#[tokio::test]
async fn a_message_is_checked_as_it_was_sent_before_validation_rewrites_it() {
    let (seen, trusted) = (Seen::default(), signer("orchestrator"));
    let h = handler(&seen, Some(Jwks::new(vec![trusted.public_jwk()])), false);
    let mut part = Part::text("summarise");
    part.filename = Some(String::new());
    let mut message = Message::user("m-3", vec![part]);
    sign_content(&mut message, &trusted).expect("signed");
    assert_eq!(send(&h, message).await, Ok(TaskState::Completed));
    assert!(matches!(
        seen.0.lock().unwrap().as_slice(),
        [ContentCheck::Verified(_)]
    ));
}

/// A resend under a send key that holds a task is answered from that task,
/// but only once its signature verifies: an altered resend is refused, not
/// answered (found by review, 2026-10-22).
#[tokio::test]
async fn an_altered_resend_under_a_held_key_is_refused_not_replayed() {
    let (seen, trusted) = (Seen::default(), signer("orchestrator"));
    let h = handler(&seen, Some(Jwks::new(vec![trusted.public_jwk()])), false);
    let mut message = Message::user("m-4", vec![Part::text("pay 10 EUR")]);
    a2a_protocol_types::idempotency::set_key(&mut message, "0123456789abcdef0123").expect("key");
    sign_content(&mut message, &trusted).expect("signed");
    assert_eq!(send(&h, message.clone()).await, Ok(TaskState::Completed));
    assert_eq!(
        send(&h, message.clone()).await,
        Ok(TaskState::Completed),
        "a replay"
    );
    let mut altered = message;
    altered.parts = vec![Part::text("pay 10000 EUR")];
    assert!(send(&h, altered).await.is_err());
    assert_eq!(seen.0.lock().unwrap().len(), 1, "the executor ran once");
}

#[tokio::test]
async fn a_streaming_send_is_checked_too() {
    let (seen, trusted) = (Seen::default(), signer("orchestrator"));
    let h = handler(&seen, Some(Jwks::new(vec![trusted.public_jwk()])), false);
    let mut altered = signed(&trusted, "a");
    altered.parts = vec![Part::text("b")];
    let result = h
        .on_send_message(MessageSendParams::new(altered), true, None)
        .await;
    assert!(result.is_err(), "refused before a stream opens");
    assert!(seen.0.lock().unwrap().is_empty());
}
