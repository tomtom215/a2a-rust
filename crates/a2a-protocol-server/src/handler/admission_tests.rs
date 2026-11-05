// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The admission gate on the send path (ADR 0045): a send waits for a slot
//! and runs, a full queue refuses, a budget bounds the wait, a reservation
//! holds across tenants, and the build refuses a gate that cannot work.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::budget::Budget;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{MessageSendParams, SendMessageConfiguration};
use tokio::sync::Semaphore;

use crate::admission::{Admission, AdmissionLoad, TenantShare};
use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::executor::AgentExecutor;
use crate::handler::RequestHandler;
use crate::request_context::RequestContext;
use crate::streaming::EventQueueWriter;

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

fn send(tenant: &str, context: &str) -> MessageSendParams {
    let mut message = Message::new(
        MessageId::new(format!("m-{context}")),
        MessageRole::User,
        vec![Part::text("hello")],
    );
    message.context_id = Some(a2a_protocol_types::task::ContextId::new(context));
    MessageSendParams::new(message)
        .with_tenant(tenant.to_owned())
        .with_configuration(
            SendMessageConfiguration::new()
                .with_accepted_output_modes(vec!["text/plain".into()])
                .with_return_immediately(true),
        )
}

fn handler(admission: Admission) -> (Arc<RequestHandler>, Arc<Semaphore>) {
    let go = Arc::new(Semaphore::new(0));
    let handler = RequestHandlerBuilder::new(Held(Arc::clone(&go)))
        .without_agent_card()
        .with_task_store(crate::store::TenantAwareInMemoryTaskStore::new())
        .with_admission(admission)
        .build()
        .expect("build");
    (Arc::new(handler), go)
}

fn load(running: usize, queued: usize, capacity: usize) -> AdmissionLoad {
    AdmissionLoad {
        running,
        queued,
        capacity,
    }
}

/// Lets spawned sends run as far as they can, on the paused clock.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(20)).await;
}

/// One slot: the first send runs, the second waits, the third finds the
/// queue full and is refused `Overloaded`; when the first ends the second
/// runs.
#[tokio::test(start_paused = true)]
async fn a_send_waits_for_a_slot_and_runs_when_one_frees() {
    let (h, go) = handler(
        Admission::new(1)
            .with_max_queued(1)
            .with_max_wait(Duration::from_secs(30)),
    );
    h.on_send_message(send("", "c1"), false, None)
        .await
        .unwrap();
    assert_eq!(h.admission_load(), Some(load(1, 0, 1)));
    let second = {
        let h = Arc::clone(&h);
        tokio::spawn(async move {
            h.on_send_message(send("", "c2"), false, None)
                .await
                .map(drop)
        })
    };
    settle().await;
    assert_eq!(h.admission_load(), Some(load(1, 1, 1)));
    let third = h.on_send_message(send("", "c3"), false, None).await;
    let Err(ServerError::Overloaded(why)) = third else {
        panic!("a full queue refuses Overloaded: {third:?}");
    };
    assert!(why.contains("no room to wait"), "{why}");
    go.add_permits(1);
    second.await.unwrap().unwrap();
    assert_eq!(
        h.admission_load(),
        Some(load(1, 0, 1)),
        "the second send runs"
    );
    go.add_permits(1);
    settle().await;
    assert_eq!(h.admission_load(), Some(load(0, 0, 1)));
}

/// A wait that reaches `max_wait` is `Overloaded`, retryable; one cut short
/// by the send's budget is refused as a spent budget is.
#[tokio::test(start_paused = true)]
async fn the_wait_ends_at_max_wait_or_at_the_budget() {
    let (h, go) = handler(
        Admission::new(1)
            .with_max_queued(4)
            .with_max_wait(Duration::from_millis(100)),
    );
    h.on_send_message(send("", "c1"), false, None)
        .await
        .unwrap();
    let waited = h.on_send_message(send("", "c2"), false, None).await;
    let Err(ServerError::Overloaded(why)) = waited else {
        panic!("{waited:?}");
    };
    assert!(why.contains("within 100ms"), "{why}");

    let (h, _go2) = handler(
        Admission::new(1)
            .with_max_queued(4)
            .with_max_wait(Duration::from_secs(30)),
    );
    h.on_send_message(send("", "c1"), false, None)
        .await
        .unwrap();
    let mut budgeted = send("", "c2");
    Budget::new()
        .with_time(Duration::from_millis(50))
        .attach(&mut budgeted.message);
    let started = tokio::time::Instant::now();
    let refused = h.on_send_message(budgeted, false, None).await;
    let Err(ServerError::InvalidParams(why)) = refused else {
        panic!("{refused:?}");
    };
    assert!(why.contains("waited for admission"), "{why}");
    // The budget is timed on the system clock from the request's arrival,
    // so what is left of it is at most 50 ms; the paused clock shows the wait
    // ended within it and not at the 30 s `max_wait`.
    let waited = started.elapsed();
    assert!(
        !waited.is_zero() && waited <= Duration::from_millis(50),
        "bounded by the budget, not the 30 s wait: {waited:?}"
    );
    assert_eq!(h.admission_load(), Some(load(1, 0, 1)));
    go.add_permits(8);
}

/// Capacity 2, tenant `a` reserving 1: tenant `b` takes the shared slot and
/// is refused another, while `a` is still admitted.
#[tokio::test(start_paused = true)]
async fn a_reservation_holds_across_tenants() {
    let (h, go) = handler(Admission::new(2).with_tenant("a", TenantShare::new().with_reserved(1)));
    h.on_send_message(send("b", "c1"), false, None)
        .await
        .unwrap();
    let refused = h.on_send_message(send("b", "c2"), false, None).await;
    assert!(
        matches!(refused, Err(ServerError::Overloaded(_))),
        "{refused:?}"
    );
    h.on_send_message(send("a", "c3"), false, None)
        .await
        .unwrap();
    assert_eq!(h.admission_load(), Some(load(2, 0, 2)));
    go.add_permits(8);
}

/// Without a gate nothing changes, and there is no load to report.
#[tokio::test(start_paused = true)]
async fn without_a_gate_there_is_no_admission_load() {
    let h = RequestHandlerBuilder::new(Held(Arc::new(Semaphore::new(8))))
        .without_agent_card()
        .build()
        .unwrap();
    assert_eq!(h.admission_load(), None);
    h.on_send_message(send("", "c1"), false, None)
        .await
        .unwrap();
}

/// The build refuses a gate above the concurrent-stream cap, and accepts one
/// at it.
#[test]
fn the_build_refuses_a_gate_above_the_stream_cap() {
    let build = |gate: usize| {
        RequestHandlerBuilder::new(Held(Arc::new(Semaphore::new(0))))
            .without_agent_card()
            .with_max_concurrent_streams(4)
            .with_admission(Admission::new(gate))
            .build()
    };
    assert!(build(4).is_ok());
    let Err(ServerError::Config(why)) = build(5) else {
        panic!("a gate above the cap must be refused");
    };
    assert!(why.contains("above the concurrent-stream cap (4)"), "{why}");
    let default_cap = RequestHandlerBuilder::new(Held(Arc::new(Semaphore::new(0))))
        .without_agent_card()
        .with_admission(Admission::new(
            crate::builder::DEFAULT_MAX_CONCURRENT_STREAMS + 1,
        ))
        .build();
    assert!(matches!(default_cap, Err(ServerError::Config(_))));
}

/// Writes one status update, then runs until the test hands it a permit.
struct Speaks(Arc<Semaphore>);

impl AgentExecutor for Speaks {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(a2a_protocol_types::events::StreamResponse::StatusUpdate(
                    a2a_protocol_types::events::TaskStatusUpdateEvent::new(
                        ctx.task_id.clone(),
                        ctx.context_id.clone(),
                        a2a_protocol_types::task::TaskStatus::new(
                            a2a_protocol_types::task::TaskState::Working,
                        ),
                    ),
                ))
                .await?;
            if let Ok(permit) = self.0.acquire().await {
                permit.forget();
            }
            Ok(())
        })
    }
}

/// With a byte budget for buffered events (ADR 0047), a stream whose reader
/// does not read pins the event it was sent; while that leaves less than a
/// tenth of the budget free, a new send is refused `Overloaded`.
#[tokio::test(start_paused = true)]
async fn a_spent_byte_budget_refuses_a_new_turn() {
    let go = Arc::new(Semaphore::new(0));
    let h = RequestHandlerBuilder::new(Speaks(Arc::clone(&go)))
        .without_agent_card()
        .with_handler_limits(
            crate::handler::HandlerLimits::default().with_max_buffered_event_bytes(64),
        )
        .build()
        .unwrap();
    let mut first = send("", "c1");
    first.configuration = None;
    let crate::handler::SendMessageResult::Stream(reader) =
        h.on_send_message(first, true, None).await.unwrap()
    else {
        panic!("a streaming send answers with a stream");
    };
    settle().await;
    assert_eq!(
        h.event_queue_manager.buffered_bytes(),
        Some((64, 64)),
        "the unread event holds the whole budget"
    );
    let mut second = send("", "c2");
    second.configuration = None;
    let refused = h.on_send_message(second, true, None).await;
    let Err(ServerError::Overloaded(why)) = refused else {
        panic!("{refused:?}");
    };
    assert!(why.contains("byte budget"), "{why}");
    drop(reader);
    go.add_permits(8);
}
