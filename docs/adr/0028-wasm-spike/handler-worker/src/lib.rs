// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The real `RequestHandler` run inside workerd, behind a JSON-RPC front written for the
//! purpose: `JsonRpcDispatcher` takes a `hyper::Request<Incoming>`, which a Worker cannot
//! construct (ADR 0028). One deployed Worker exercises every path; the text of the message
//! picks the executor's behaviour (`slow`, `hang`, `until-cancelled`).
use std::sync::OnceLock;
use std::time::Duration;

use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_server::streaming::EventQueueReader;
use a2a_protocol_server::{EventEmitter, RequestHandler, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::jsonrpc::JsonRpcRequest;
use a2a_protocol_types::message::Part;
use a2a_protocol_types::params::{CancelTaskParams, MessageSendParams, TaskQueryParams};
use a2a_protocol_types::task::TaskState;
use worker::*;

struct Agent;

// The message's text picks the behaviour, so one deployed Worker exercises every path.
agent_executor!(Agent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    let who = ctx.message.text().unwrap_or("world");
    match who {
        // A timer inside the executor: setTimeout, via the crate's own timers.
        "slow" => a2a_protocol_server::rt::sleep(Duration::from_millis(80)).await,
        // Longer than the handler's executor timeout: the handler's own timer must fire.
        "hang" => a2a_protocol_server::rt::sleep(Duration::from_secs(30)).await,
        // Runs until CancelTask triggers the token.
        "until-cancelled" => ctx.cancellation_token.cancelled().await,
        _ => {}
    }
    emit.artifact(
        "greeting",
        vec![Part::text(format!("Hello, {who}!"))],
        None,
        Some(true),
    )
    .await?;
    emit.status(TaskState::Completed).await?;
    Ok(())
});

fn card(origin: &str) -> AgentCard {
    AgentCard::new("handler-worker", "0.0.0", AgentInterface::jsonrpc(origin))
        .with_description("The real RequestHandler, in workerd")
        .with_skill(AgentSkill::new("hello", "Hello", "Greets").with_tags(["greeting"]))
        .with_capabilities(
            a2a_protocol_types::agent_card::AgentCapabilities::none().with_streaming(true),
        )
}

// One handler per isolate: the in-memory stores live as long as the isolate does.
static HANDLER: OnceLock<RequestHandler> = OnceLock::new();
static KEYS: OnceLock<a2a_protocol_types::signing::Jwks> = OnceLock::new();

/// A fresh ES256 key from the Workers CSPRNG (`crypto.getRandomValues`, via ring's JS backend).
fn signer() -> a2a_protocol_types::provenance::ContentSigner {
    use ring::rand::SystemRandom;
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair};
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .expect("key generation");
    let signer = a2a_protocol_types::provenance::ContentSigner::es256("worker-1", pkcs8.as_ref())
        .expect("signer");
    let _ = KEYS.set(a2a_protocol_types::signing::Jwks::new(vec![
        signer.public_jwk(),
    ]));
    signer
}

fn handler(origin: &str) -> &'static RequestHandler {
    HANDLER.get_or_init(|| {
        RequestHandlerBuilder::new(Agent)
            .with_agent_card(card(origin))
            .with_executor_timeout(Duration::from_millis(400))
            .with_content_signer(signer())
            .build()
            .expect("handler builds")
    })
}

fn rpc_ok(id: &serde_json::Value, result: impl serde::Serialize) -> Result<Response> {
    Response::from_json(&serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn rpc_err(id: &serde_json::Value, e: impl std::fmt::Display) -> Result<Response> {
    Response::from_json(&serde_json::json!({
        "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": e.to_string() } }))
}

fn from_params<T: serde::de::DeserializeOwned>(rpc: &JsonRpcRequest) -> Result<T> {
    serde_json::from_value(rpc.params.clone().unwrap_or_default())
        .map_err(|e| Error::RustError(format!("invalid params: {e}")))
}

#[event(fetch)]
async fn fetch(mut req: Request, _env: Env, ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();
    let url = req.url()?;
    let origin = format!(
        "{}://{}:{}",
        url.scheme(),
        url.host_str().unwrap_or("localhost"),
        url.port().unwrap_or(80)
    );
    let h = handler(&origin);
    match (req.method(), url.path()) {
        (Method::Get, "/.well-known/agent-card.json") => Response::from_json(&card(&origin)),
        // Sends through the handler, then verifies the artifact it returned
        // against the signer's own public key; a tampered copy must fail.
        (Method::Get, "/signed-check") => {
            use a2a_protocol_types::provenance::verify_content;
            let params: MessageSendParams = serde_json::from_value(serde_json::json!({
                "message": { "messageId": "sc-1", "role": "ROLE_USER", "parts": [{ "text": "signed" }] }
            })).map_err(|e| Error::RustError(e.to_string()))?;
            let Ok(SendMessageResult::Response(
                a2a_protocol_types::responses::SendMessageResponse::Task(task),
            )) = h.on_send_message(params, false, None).await
            else {
                return Response::error("no task", 500);
            };
            let artifact = task
                .artifacts
                .as_ref()
                .and_then(|a| a.first())
                .cloned()
                .ok_or(Error::RustError("no artifact".into()))?;
            let keys = KEYS.get().ok_or(Error::RustError("no keys".into()))?;
            let genuine = verify_content(&artifact, keys).is_ok();
            let mut tampered = artifact.clone();
            tampered.parts = vec![Part::text("Hello, someone else!")];
            let forged_rejected = verify_content(&tampered, keys).is_err();
            Response::ok(format!(
                "genuine_verifies={genuine} tampered_rejected={forged_rejected} sig_in_metadata={}",
                serde_json::to_string(&artifact.metadata)
                    .map(|m| m.contains("signature"))
                    .unwrap_or(false)
            ))
        }
        // Whether the handler has nothing in flight: `wait_idle` resolves within 100 ms.
        (Method::Get, "/idle") => {
            use futures::future::{Either, select};
            let idle = std::pin::pin!(h.wait_idle());
            let limit = std::pin::pin!(a2a_protocol_server::rt::sleep(Duration::from_millis(100)));
            Response::ok(matches!(select(idle, limit).await, Either::Left(_)).to_string())
        }
        (Method::Post, "/") => {
            let body = req.text().await?;
            let rpc: JsonRpcRequest =
                serde_json::from_str(&body).map_err(|e| Error::RustError(e.to_string()))?;
            let id = serde_json::to_value(&rpc.id).unwrap_or_default();
            match rpc.method.as_str() {
                "SendMessage" => match h
                    .on_send_message(from_params::<MessageSendParams>(&rpc)?, false, None)
                    .await
                {
                    Ok(SendMessageResult::Response(r)) => {
                        // `?hold=1`: keep this request's context, and the timers armed in it,
                        // alive past the response until the handler has no executor running.
                        // Registered here, not at the top of the request: before the send
                        // there is nothing in flight yet, and `wait_idle` returns at once.
                        if url.query_pairs().any(|(k, v)| k == "hold" && v == "1") {
                            ctx.wait_until(async move { h.wait_idle().await });
                        }
                        rpc_ok(&id, r)
                    }
                    Ok(_) => rpc_err(&id, "unexpected stream"),
                    Err(e) => rpc_err(&id, e),
                },
                "SendStreamingMessage" => match h
                    .on_send_message(from_params::<MessageSendParams>(&rpc)?, true, None)
                    .await
                {
                    Ok(SendMessageResult::Stream(reader)) => {
                        let frames = futures::stream::unfold(
                            (reader, id),
                            |(mut reader, id)| async move {
                                let next = reader.read().await?;
                                let frame = match next {
                                    Ok(ev) => {
                                        serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": ev.event })
                                    }
                                    Err(e) => {
                                        serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": e.to_string() } })
                                    }
                                };
                                let chunk: std::result::Result<Vec<u8>, std::convert::Infallible> =
                                    Ok(format!("data: {frame}\n\n").into_bytes());
                                Some((chunk, (reader, id)))
                            },
                        );
                        let mut resp = Response::from_stream(frames)?;
                        resp.headers_mut()
                            .set("content-type", "text/event-stream")?;
                        Ok(resp)
                    }
                    Ok(_) => rpc_err(&id, "expected a stream"),
                    Err(e) => rpc_err(&id, e),
                },
                "GetTask" => match h
                    .on_get_task(from_params::<TaskQueryParams>(&rpc)?, None)
                    .await
                {
                    Ok(t) => rpc_ok(&id, t),
                    Err(e) => rpc_err(&id, e),
                },
                "CancelTask" => match h
                    .on_cancel_task(from_params::<CancelTaskParams>(&rpc)?, None)
                    .await
                {
                    Ok(t) => rpc_ok(&id, t),
                    Err(e) => rpc_err(&id, e),
                },
                other => rpc_err(&id, format!("method not found: {other}")),
            }
        }
        _ => Response::error("not found", 404),
    }
}
