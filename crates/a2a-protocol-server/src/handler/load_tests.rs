// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The load report's numbers and text, the hysteresis, and the handler's
//! load, readiness and header as sends run.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{MessageSendParams, SendMessageConfiguration};
use tokio::sync::Semaphore;

use super::*;
use crate::builder::RequestHandlerBuilder;
use crate::executor::AgentExecutor;
use crate::handler::{HaltScope, HandlerLimits};
use crate::request_context::RequestContext;
use crate::streaming::EventQueueWriter;

fn report(running: usize, queued: usize, capacity: Option<usize>) -> LoadReport {
    LoadReport {
        running,
        queued,
        capacity,
        draining: false,
        halted: false,
    }
}

#[test]
fn utilization_is_demand_over_capacity_in_thousandths() {
    assert_eq!(report(1, 0, Some(4)).utilization_permille(), Some(250));
    assert_eq!(report(1, 0, Some(3)).utilization_permille(), Some(333));
    assert_eq!(report(4, 2, Some(4)).utilization_permille(), Some(1500));
    assert_eq!(report(0, 0, Some(4)).utilization_permille(), Some(0));
    assert_eq!(report(3, 0, None).utilization_permille(), None);
    assert_eq!(report(3, 0, Some(0)).utilization_permille(), None);
    // Past what a u64 holds: saturated, not wrapped.
    assert_eq!(
        report(usize::MAX, usize::MAX, Some(1)).utilization_permille(),
        Some(u64::MAX)
    );
}

#[test]
fn the_header_and_the_body_say_the_same_numbers() {
    let load = LoadReport {
        draining: true,
        ..report(4, 2, Some(4))
    };
    assert_eq!(
        load.header_value(),
        "TEXT application_utilization=1.500, named_metrics.running=4, named_metrics.queued=2"
    );
    assert_eq!(
        load.to_json(),
        serde_json::json!({
            "running": 4, "queued": 2, "capacity": 4, "utilization": 1.5,
            "draining": true, "halted": false,
        })
    );
    let third = report(1, 0, Some(3));
    assert_eq!(
        third.header_value(),
        "TEXT application_utilization=0.333, named_metrics.running=1, named_metrics.queued=0"
    );
    assert_eq!(third.to_json()["utilization"], serde_json::json!(0.333));
    let unbounded = LoadReport {
        halted: true,
        ..report(2, 0, None)
    };
    assert_eq!(
        unbounded.header_value(),
        "TEXT named_metrics.running=2, named_metrics.queued=0"
    );
    assert_eq!(
        unbounded.to_json(),
        serde_json::json!({
            "running": 2, "queued": 0, "capacity": null, "utilization": null,
            "draining": false, "halted": true,
        })
    );
}

/// Unready at 80 %, ready again at 50 % or below, each edge exact.
#[test]
fn saturation_has_a_band_between_its_two_thresholds() {
    assert!(!next_saturated(false, 799, 80, 50));
    assert!(next_saturated(false, 800, 80, 50));
    assert!(next_saturated(true, 501, 80, 50));
    assert!(!next_saturated(true, 500, 80, 50));
    assert!(
        !next_saturated(false, 600, 80, 50),
        "in the band, stays ready"
    );
    assert!(
        next_saturated(true, 600, 80, 50),
        "in the band, stays saturated"
    );
}

/// Runs until the test hands it a permit, which it keeps.
struct Held(Arc<Semaphore>);

impl AgentExecutor for Held {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Spent, not returned: each permit the test adds ends one turn.
            if let Ok(permit) = self.0.acquire().await {
                permit.forget();
            }
            Ok(())
        })
    }
}

fn send(context: &str) -> MessageSendParams {
    let mut message = Message::new(
        MessageId::new(format!("m-{context}")),
        MessageRole::User,
        vec![Part::text("hello")],
    );
    message.context_id = Some(a2a_protocol_types::task::ContextId::new(context));
    MessageSendParams::new(message).with_configuration(
        SendMessageConfiguration::new()
            .with_accepted_output_modes(vec!["text/plain".into()])
            .with_return_immediately(true),
    )
}

/// Lets finished turns release their queues, on the paused clock.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(20)).await;
}

/// Four streams, unready at 80 % and ready at 50 %: three running is ready,
/// four is saturated, three again stays saturated, one is ready. The load
/// and the header follow the turns.
#[tokio::test(start_paused = true)]
async fn readiness_follows_saturation_with_hysteresis() {
    let go = Arc::new(Semaphore::new(0));
    let handler = RequestHandlerBuilder::new(Held(Arc::clone(&go)))
        .without_agent_card()
        .with_max_concurrent_streams(4)
        .with_handler_limits(HandlerLimits::default().with_saturation_readiness(80, 50))
        .with_load_reporting()
        .build()
        .unwrap();
    assert!(handler.reports_load());
    for c in ["c1", "c2", "c3"] {
        handler.on_send_message(send(c), false, None).await.unwrap();
    }
    assert_eq!(handler.load(), report(3, 0, Some(4)));
    assert_eq!(handler.readiness().await, Ok(()));
    handler
        .on_send_message(send("c4"), false, None)
        .await
        .unwrap();
    assert_eq!(handler.readiness().await, Err("saturated"));
    let mut headers = hyper::HeaderMap::new();
    handler.stamp_load(&mut headers);
    assert_eq!(
        headers[LOAD_HEADER],
        "TEXT application_utilization=1.000, named_metrics.running=4, named_metrics.queued=0"
    );

    go.add_permits(1);
    settle().await;
    assert_eq!(handler.load().running, 3);
    assert_eq!(
        handler.readiness().await,
        Err("saturated"),
        "75 % is in the band"
    );
    go.add_permits(2);
    settle().await;
    assert_eq!(handler.load().running, 1);
    assert_eq!(
        handler.readiness().await,
        Ok(()),
        "25 % is at or below 50 %"
    );
    go.add_permits(8);
}

/// Without load reporting there is no header, and without saturation
/// readiness a full replica is still ready.
#[tokio::test(start_paused = true)]
async fn by_default_load_changes_neither_readiness_nor_responses() {
    let go = Arc::new(Semaphore::new(0));
    let handler = RequestHandlerBuilder::new(Held(Arc::clone(&go)))
        .without_agent_card()
        .with_max_concurrent_streams(1)
        .build()
        .unwrap();
    assert!(!handler.reports_load());
    handler
        .on_send_message(send("c1"), false, None)
        .await
        .unwrap();
    assert_eq!(handler.load(), report(1, 0, Some(1)));
    assert_eq!(handler.readiness().await, Ok(()));
    let mut headers = hyper::HeaderMap::new();
    handler.stamp_load(&mut headers);
    assert!(headers.is_empty());
    go.add_permits(8);
}

/// A halt of everything shows in the load; a tenant's halt does not; and a
/// draining replica says so.
#[tokio::test]
async fn the_load_says_halted_and_draining() {
    let handler = RequestHandlerBuilder::new(Held(Arc::new(Semaphore::new(8))))
        .without_agent_card()
        .build()
        .unwrap();
    assert_eq!(
        handler.load().capacity,
        Some(crate::builder::DEFAULT_MAX_CONCURRENT_STREAMS)
    );
    handler
        .halt(HaltScope::Tenant("acme".into()), "op", "one tenant")
        .await;
    assert!(!handler.load().halted);
    handler.halt(HaltScope::All, "op", "everything").await;
    assert!(handler.load().halted);
    assert!(!handler.load().draining);
    handler.begin_draining();
    assert!(handler.load().draining);
}

/// With an admission gate, the load counts its waiting sends and measures
/// against its capacity, not the stream cap.
#[tokio::test(start_paused = true)]
async fn with_a_gate_the_load_counts_its_queue_against_its_capacity() {
    let go = Arc::new(Semaphore::new(0));
    let handler = Arc::new(
        RequestHandlerBuilder::new(Held(Arc::clone(&go)))
            .without_agent_card()
            .with_max_concurrent_streams(8)
            .with_admission(
                crate::admission::Admission::new(1)
                    .with_max_queued(4)
                    .with_max_wait(Duration::from_secs(30)),
            )
            .build()
            .unwrap(),
    );
    handler
        .on_send_message(send("c1"), false, None)
        .await
        .unwrap();
    let waiting = {
        let handler = Arc::clone(&handler);
        tokio::spawn(async move {
            handler
                .on_send_message(send("c2"), false, None)
                .await
                .map(drop)
        })
    };
    settle().await;
    let load = handler.load();
    assert_eq!(load, report(1, 1, Some(1)));
    assert_eq!(load.utilization_permille(), Some(2000));
    go.add_permits(1);
    waiting.await.unwrap().unwrap();
    go.add_permits(8);
}

/// The build refuses a band that is not one.
#[test]
fn the_build_refuses_saturation_thresholds_that_are_not_a_band() {
    let build = |unready: u8, ready: u8| {
        RequestHandlerBuilder::new(Held(Arc::new(Semaphore::new(0))))
            .without_agent_card()
            .with_handler_limits(HandlerLimits::default().with_saturation_readiness(unready, ready))
            .build()
    };
    assert!(build(80, 79).is_ok());
    assert!(build(1, 0).is_ok());
    for (unready, ready) in [(80, 80), (50, 80), (0, 0)] {
        let Err(crate::error::ServerError::Config(why)) = build(unready, ready) else {
            panic!("({unready}, {ready}) must be refused");
        };
        assert!(why.contains("saturation readiness"), "{why}");
    }
}
