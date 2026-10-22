// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the queue writer does so the content a signing server serves can
//! always be signed and verified (ADR 0027): it refuses malformed
//! provenance, and unsets the empty strings and lists gRPC cannot carry.
#![cfg(feature = "signing")]

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_server::{AgentExecutor, EventEmitter, RequestContext};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent};
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::provenance::ContentSigner;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskState};

fn signer(kid: &str) -> ContentSigner {
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .expect("a key");
    ContentSigner::ed25519(kid, pkcs8.as_ref()).expect("an Ed25519 signer")
}

#[tokio::test]
async fn malformed_provenance_is_refused_when_written() {
    struct Bad;
    impl AgentExecutor for Bad {
        fn execute<'a>(
            &'a self,
            ctx: &'a RequestContext,
            queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async move {
                let mut artifact = Artifact::new("a", vec![Part::text("x")]);
                artifact.metadata = Some(serde_json::json!({"a2a-rust.com/provenance": 7}));
                let refused = queue
                    .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact,
                        append: None,
                        last_chunk: None,
                        metadata: None,
                    }))
                    .await;
                assert!(refused.is_err(), "the writer refuses it");
                EventEmitter::new(ctx, queue)
                    .status(TaskState::Completed)
                    .await
            })
        }
    }
    let handler = RequestHandlerBuilder::new(Bad)
        .without_agent_card()
        .with_content_signer(signer("ours"))
        .build()
        .expect("handler");
    let Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) = handler
        .on_send_message(
            MessageSendParams::new(Message::user_text("u", "go")),
            false,
            None,
        )
        .await
    else {
        panic!("a task");
    };
    assert_eq!(task.status.state, TaskState::Completed);
    assert!(task.artifacts.is_none_or(|a| a.is_empty()));
}

/// gRPC reads an empty string or list as absent. The writer unsets them, so
/// what is signed is what every binding carries, and a signature verifies
/// after a protobuf round trip.
#[cfg(feature = "grpc")]
#[tokio::test]
async fn a_signature_survives_the_grpc_conversion() {
    struct Blank;
    impl AgentExecutor for Blank {
        fn execute<'a>(
            &'a self,
            ctx: &'a RequestContext,
            queue: &'a dyn EventQueueWriter,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async move {
                let mut part = Part::text("x");
                part.media_type = Some(String::new());
                part.filename = Some(String::new());
                let mut artifact = Artifact::new("a", vec![part]);
                artifact.name = Some(String::new());
                artifact.description = Some(String::new());
                artifact.extensions = Some(Vec::new());
                queue
                    .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact,
                        append: None,
                        last_chunk: Some(true),
                        metadata: None,
                    }))
                    .await?;
                EventEmitter::new(ctx, queue)
                    .status(TaskState::Completed)
                    .await
            })
        }
    }
    let ours = signer("ours");
    let keys = a2a_protocol_types::signing::Jwks::new(vec![ours.public_jwk()]);
    let handler = RequestHandlerBuilder::new(Blank)
        .without_agent_card()
        .with_content_signer(ours)
        .build()
        .expect("handler");
    let Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) = handler
        .on_send_message(
            MessageSendParams::new(Message::user_text("u", "go")),
            false,
            None,
        )
        .await
    else {
        panic!("a task");
    };
    let served = task.artifacts.expect("artifacts").remove(0);
    let proto = a2a_protocol_types::proto::Artifact::try_from(served.clone()).expect("to proto");
    let back = Artifact::try_from(proto).expect("from proto");
    assert_eq!(back, served, "the gRPC round trip changes nothing");
    a2a_protocol_types::provenance::verify_content(&back, &keys)
        .expect("and the signature verifies after it");
}
