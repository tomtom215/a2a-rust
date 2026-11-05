// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The handler bounds its own webhook deliveries (audit N-SEC-6, SEC-14).
//!
//! `PushGate` is tested alone in `src/push/gate/tests.rs`. This is the test
//! that the handler a builder returns has one in front of the sender it was
//! given: without it, every unit test of the gate passes while a deployment
//! delivers as many requests at once as it has configs.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_server::{HandlerLimits, PushSender, RequestHandlerBuilder};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use serde_json::json;

use crate::DEADLINE;
use crate::s3_agent::{Scripted, Seen, immediately, send};

/// Counts the sends running at once; each takes a moment, so they overlap.
#[derive(Default)]
struct Overlap {
    now: AtomicUsize,
    max: AtomicUsize,
    done: AtomicUsize,
}

/// The sender the handler is given; the counters stay shared with the test.
struct Counting(Arc<Overlap>);

impl PushSender for Counting {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let o = &self.0;
            let n = o.now.fetch_add(1, Ordering::SeqCst) + 1;
            o.max.fetch_max(n, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(40)).await;
            o.now.fetch_sub(1, Ordering::SeqCst);
            o.done.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn allows_private_urls(&self) -> bool {
        true
    }
}

async fn six_webhooks_on_one_task(limits: HandlerLimits) -> Arc<Overlap> {
    let overlap = Arc::new(Overlap::default());
    let seen = Arc::new(Seen::default());
    let handler = RequestHandlerBuilder::new(Scripted(Arc::clone(&seen)))
        .with_agent_card(crate::card(&[]))
        .with_push_sender(Counting(Arc::clone(&overlap)))
        .with_handler_limits(limits)
        .build()
        .expect("handler");
    let task = match handler
        .on_send_message(immediately(send("m-1", "wait", None, None)), false, None)
        .await
        .expect("send")
    {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    };
    for i in 0..6 {
        let config: TaskPushNotificationConfig = serde_json::from_value(json!({
            "taskId": task, "url": format!("https://h{i}.example.com/hook"),
        }))
        .expect("config");
        handler
            .on_create_push_config(config, None)
            .await
            .expect("create");
    }
    seen.release.notify_one();
    // The terminal update goes to every config: at least six deliveries.
    tokio::time::timeout(DEADLINE, async {
        while overlap.done.load(Ordering::SeqCst) < 6 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("every webhook was delivered to");
    overlap
}

#[tokio::test]
async fn a_handler_delivers_to_every_webhook_but_never_beyond_its_bound() {
    let overlap =
        six_webhooks_on_one_task(HandlerLimits::default().with_push_max_in_flight(2)).await;
    assert_eq!(
        overlap.max.load(Ordering::SeqCst),
        2,
        "six configs, a bound of two: saturated, never beyond"
    );
}

#[tokio::test]
async fn without_the_bound_the_same_six_run_together() {
    // The control: the same scenario with the bound out of the way, so the
    // test above cannot pass merely because deliveries happen to be serial.
    let overlap =
        six_webhooks_on_one_task(HandlerLimits::default().with_push_max_in_flight(1000)).await;
    assert_eq!(overlap.max.load(Ordering::SeqCst), 6);
}
