// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A client with content keys checks the provenance signature of what the
//! agent sends (ADR 0027), against a stub that serves a signed artifact as
//! signed, altered after signing, or unsigned: on a stream and from
//! `GetTask`.
#![cfg(feature = "signing")]

mod common;

use a2a_protocol_client::{ClientBuilder, ClientError};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::message::Part;
use a2a_protocol_types::params::TaskQueryParams;
use a2a_protocol_types::provenance::{ContentSigner, sign_content};
use a2a_protocol_types::signing::Jwks;
use common::{LAST_CHUNK, SSE_HEAD, chunk, jsonrpc_status_frame, params, serve, write};

fn signer() -> ContentSigner {
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .expect("a key");
    ContentSigner::ed25519("agent", pkcs8.as_ref()).expect("an Ed25519 signer")
}

#[derive(Clone, Copy)]
enum Served {
    Signed,
    Altered,
    Unsigned,
}

/// The report artifact as the stub serves it.
fn artifact(by: &ContentSigner, how: Served) -> serde_json::Value {
    let mut a = Artifact::new("report", vec![Part::text("revenue rose 4%")]);
    if !matches!(how, Served::Unsigned) {
        sign_content(&mut a, by).expect("signed");
    }
    if matches!(how, Served::Altered) {
        a.parts = vec![Part::text("revenue rose 40%")];
    }
    serde_json::to_value(&a).unwrap()
}

/// A stub agent: a stream of the artifact then `completed`, or a task
/// holding it, for whichever method is called.
async fn agent(by: &ContentSigner, how: Served) -> String {
    let artifact = artifact(by, how);
    serve(move |mut stream, request| {
        let artifact = artifact.clone();
        async move {
            let body = &request[request.find("\r\n\r\n").map_or(0, |i| i + 4)..];
            let call: serde_json::Value = serde_json::from_str(body).expect("a JSON-RPC call");
            let id = call["id"].clone();
            if call["method"] == "SendStreamingMessage" {
                let update = serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                    "artifactUpdate": {"taskId": "t", "contextId": "c", "artifact": artifact}
                }});
                let frames = format!(
                    "{SSE_HEAD}{}{}{LAST_CHUNK}",
                    chunk(&format!("data: {update}\n\n")),
                    chunk(&jsonrpc_status_frame("TASK_STATE_COMPLETED"))
                );
                write(&mut stream, &frames).await;
            } else {
                let task = serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                    "id": "t", "contextId": "c",
                    "status": {"state": "TASK_STATE_COMPLETED"},
                    "artifacts": [artifact]
                }});
                let body = task.to_string();
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
                    body.len()
                );
                write(&mut stream, &format!("{head}{body}")).await;
            }
        }
    })
    .await
}

fn client(url: &str, keys: Option<Jwks>, require: bool) -> a2a_protocol_client::A2aClient {
    let mut builder = ClientBuilder::new(url).without_capability_checks();
    if let Some(keys) = keys {
        builder = builder.with_content_keys(keys);
        if require {
            builder = builder.require_signed_content();
        }
    }
    builder.build().expect("client")
}

fn query() -> TaskQueryParams {
    TaskQueryParams {
        tenant: None,
        id: "t".to_owned(),
        history_length: None,
    }
}

/// Every event of a stream, or the first error.
async fn streamed(client: &a2a_protocol_client::A2aClient) -> Result<usize, ClientError> {
    let mut stream = client.stream_message(params()).await?;
    let mut events = 0;
    while let Some(event) = stream.next().await {
        event?;
        events += 1;
    }
    Ok(events)
}

#[tokio::test]
async fn signed_content_verifies_and_altered_content_is_refused() {
    let by = signer();
    let keys = Jwks::new(vec![by.public_jwk()]);

    let good = agent(&by, Served::Signed).await;
    let c = client(&good, Some(keys.clone()), false);
    assert_eq!(streamed(&c).await.expect("verifies"), 2);
    let task = c.get_task(query()).await.expect("verifies");
    assert_eq!(task.artifacts.map(|a| a.len()), Some(1));

    let bad = agent(&by, Served::Altered).await;
    let c = client(&bad, Some(keys), false);
    let streamed_err = streamed(&c)
        .await
        .expect_err("the altered artifact is refused");
    assert!(
        matches!(streamed_err, ClientError::ContentNotVerified(_)),
        "{streamed_err}"
    );
    let got_err = c.get_task(query()).await.expect_err("and from GetTask");
    assert!(
        matches!(got_err, ClientError::ContentNotVerified(_)),
        "{got_err}"
    );
}

#[tokio::test]
async fn a_key_not_in_the_set_is_refused() {
    let (by, stranger) = (signer(), signer());
    let url = agent(&by, Served::Signed).await;
    let c = client(&url, Some(Jwks::new(vec![stranger.public_jwk()])), false);
    assert!(matches!(
        c.get_task(query()).await,
        Err(ClientError::ContentNotVerified(_))
    ));
}

#[tokio::test]
async fn unsigned_content_passes_unless_signatures_are_required() {
    let by = signer();
    let keys = Jwks::new(vec![by.public_jwk()]);
    let url = agent(&by, Served::Unsigned).await;
    assert!(
        client(&url, Some(keys.clone()), false)
            .get_task(query())
            .await
            .is_ok()
    );
    let strict = client(&url, Some(keys), true);
    assert!(matches!(
        strict.get_task(query()).await,
        Err(ClientError::ContentNotVerified(_))
    ));
    assert!(matches!(
        streamed(&strict).await,
        Err(ClientError::ContentNotVerified(_))
    ));
}

#[tokio::test]
async fn without_keys_nothing_is_checked() {
    let url = agent(&signer(), Served::Altered).await;
    let c = client(&url, None, false);
    assert!(c.get_task(query()).await.is_ok());
    assert_eq!(streamed(&c).await.expect("no check"), 2);
}

/// A stub whose `GetTask` answers with a status note the agent signed and
/// an intermediary then relabelled `ROLE_USER`, and whose stream carries two
/// altered artifacts in one body.
async fn relabelling_agent(by: &ContentSigner) -> String {
    let mut note = a2a_protocol_types::message::Message::agent_text("n-1", "approved");
    sign_content(&mut note, by).expect("signed");
    note.role = a2a_protocol_types::message::MessageRole::User;
    let note = serde_json::to_value(&note).unwrap();
    let altered = artifact(by, Served::Altered);
    serve(move |mut stream, request| {
        let (note, altered) = (note.clone(), altered.clone());
        async move {
            let body = &request[request.find("\r\n\r\n").map_or(0, |i| i + 4)..];
            let call: serde_json::Value = serde_json::from_str(body).expect("a JSON-RPC call");
            let id = call["id"].clone();
            if call["method"] == "SendStreamingMessage" {
                let frame = |n: u32| {
                    let update = serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                        "artifactUpdate": {"taskId": "t", "contextId": "c", "artifact": altered}
                    }});
                    format!("id: {n}\ndata: {update}\n\n")
                };
                // Both frames in one chunk, so the second is buffered behind
                // the first when it is refused.
                let frames = format!("{SSE_HEAD}{}{LAST_CHUNK}", chunk(&(frame(1) + &frame(2))));
                write(&mut stream, &frames).await;
            } else {
                let task = serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                    "id": "t", "contextId": "c",
                    "status": {"state": "TASK_STATE_COMPLETED", "message": note}
                }});
                let body = task.to_string();
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
                    body.len()
                );
                write(&mut stream, &format!("{head}{body}")).await;
            }
        }
    })
    .await
}

/// A signed status note relabelled to another role is still checked, and
/// fails (found by review, 2026-10-22: it was skipped as not the agent's).
#[tokio::test]
async fn a_signed_note_relabelled_to_another_role_is_refused() {
    let by = signer();
    let url = relabelling_agent(&by).await;
    let c = client(&url, Some(Jwks::new(vec![by.public_jwk()])), false);
    assert!(matches!(
        c.get_task(query()).await,
        Err(ClientError::ContentNotVerified(_))
    ));
}

/// After a refusal the stream ends: the frame buffered behind the refused
/// one is not delivered, and the refused one does not count as delivered.
#[tokio::test]
async fn nothing_follows_a_refusal_on_a_stream() {
    let by = signer();
    let url = relabelling_agent(&by).await;
    let c = client(&url, Some(Jwks::new(vec![by.public_jwk()])), false);
    let mut stream = c.stream_message(params()).await.expect("stream");
    assert!(matches!(
        stream.next().await,
        Some(Err(ClientError::ContentNotVerified(_)))
    ));
    assert!(stream.next().await.is_none(), "nothing after the refusal");
    assert_eq!(
        stream.last_event_id(),
        None,
        "the refused frame is not delivered"
    );
}
