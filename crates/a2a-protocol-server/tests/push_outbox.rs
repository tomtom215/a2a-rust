// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The probe the audit wrote for H14 (ADR 0037), in one process: a task's
//! terminal event is sent to a webhook that is down; the handler stops; the
//! webhook comes up; a new handler on the same SQLite file delivers the event,
//! with the delivery id of the attempt that failed. Without the outbox
//! nothing is delivered after the stop.
//!
//! Real sockets and a real database file, so the waits are bounded polls on
//! what the receiver saw, with a generous deadline, never a sleep for time.

#![cfg(feature = "sqlite")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use a2a_protocol_types::TaskState;
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{MessageSendParams, SendMessageConfiguration};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::ContextId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::push::{
    HttpPushSender, PushConfigStore, PushOutbox, PushRetryPolicy, SqlitePushConfigStore,
    SqlitePushOutbox,
};
use a2a_protocol_server::{EventEmitter, RequestHandler, agent_executor};

struct Completes;
agent_executor!(Completes, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Completed)
        .await
});

fn card() -> AgentCard {
    AgentCard::new(
        "Outbox Agent",
        "1.0.0",
        a2a_protocol_types::agent_card::AgentInterface::new(
            "https://agent.example.com/rpc",
            "JSONRPC",
        )
        .with_protocol_version("1.0.0"),
    )
    .with_description("Delivers through an outbox")
    .with_input_modes(vec!["text/plain"])
    .with_output_modes(vec!["text/plain"])
    .with_skill(a2a_protocol_types::AgentSkill::new("t", "T", "t").with_tags(["t"]))
    .with_capabilities(AgentCapabilities::none().with_push_notifications(true))
}

/// One request the receiver answered: its delivery id, whether it carried
/// the terminal status, and whether the receiver was up.
#[derive(Debug, Clone)]
struct Answered {
    delivery_id: String,
    terminal: bool,
    up: bool,
}

/// A webhook on loopback that answers `503` until `up`, then `200`.
async fn receiver(up: Arc<AtomicBool>) -> (String, mpsc::UnboundedReceiver<Answered>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let (tx, up) = (tx.clone(), Arc::clone(&up));
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                // Head and body: read until the body's length is in.
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
                let text = String::from_utf8_lossy(&buf).into_owned();
                let delivery_id = text
                    .lines()
                    .find_map(|l| {
                        let (name, value) = l.split_once(':')?;
                        name.eq_ignore_ascii_case("a2a-delivery-id")
                            .then(|| value.trim().to_owned())
                    })
                    .unwrap_or_default();
                let is_up = up.load(Ordering::SeqCst);
                let _ = tx.send(Answered {
                    delivery_id,
                    terminal: text.contains("TASK_STATE_COMPLETED"),
                    up: is_up,
                });
                let status = if is_up {
                    "200 OK"
                } else {
                    "503 Service Unavailable"
                };
                let _ = stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    (url, rx)
}

async fn handler(url: &str) -> RequestHandler {
    let configs = SqlitePushConfigStore::new(url).await.unwrap();
    let outbox = SqlitePushOutbox::new(url).await.unwrap();
    RequestHandlerBuilder::new(Completes)
        .with_agent_card(card())
        .with_push_sender(
            HttpPushSender::new()
                .allow_private_urls()
                .with_retry_policy(PushRetryPolicy::default().with_max_attempts(1)),
        )
        .with_push_config_store_arc(Arc::new(configs) as Arc<dyn PushConfigStore>)
        .with_push_outbox(Arc::new(outbox) as Arc<dyn PushOutbox>)
        .build()
        .expect("handler")
}

/// The next terminal delivery the receiver answered, within 60 s.
async fn next_terminal(rx: &mut mpsc::UnboundedReceiver<Answered>) -> Answered {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let answered = rx.recv().await.expect("receiver");
            if answered.terminal {
                return answered;
            }
        }
    })
    .await
    .expect("a terminal delivery within 60 s")
}

#[tokio::test]
async fn a_terminal_event_left_by_a_stopped_handler_is_delivered_by_the_next() {
    let path = std::env::temp_dir().join(format!(
        "a2a-push-outbox-probe-{}-{}.db",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let db = format!("sqlite://{}", path.display());
    let up = Arc::new(AtomicBool::new(false));
    let (hook, mut answered) = receiver(Arc::clone(&up)).await;

    let first = handler(&db).await;
    let mut configuration = SendMessageConfiguration::new();
    configuration.task_push_notification_config =
        Some(TaskPushNotificationConfig::for_url(hook.clone()));
    first
        .on_send_message(
            MessageSendParams::new(
                Message::new(
                    MessageId::new("m1"),
                    MessageRole::User,
                    vec![Part::text("go")],
                )
                .with_context_id(ContextId::new("ctx-outbox")),
            )
            .with_configuration(configuration),
            false,
            None,
        )
        .await
        .expect("send");

    let refused = next_terminal(&mut answered).await;
    assert!(!refused.up, "the webhook was down");
    assert!(!refused.delivery_id.is_empty());

    // The process stops; the webhook comes back; another starts.
    let _report = first.shutdown().await;
    drop(first);
    up.store(true, Ordering::SeqCst);
    let _second = handler(&db).await;

    let delivered = loop {
        let next = next_terminal(&mut answered).await;
        if next.up {
            break next;
        }
    };
    assert_eq!(
        delivered.delivery_id, refused.delivery_id,
        "the same delivery, from the next process"
    );

    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}
