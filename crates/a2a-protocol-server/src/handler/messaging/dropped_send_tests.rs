// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A send dropped at any await after its task is saved leaves nothing
//! non-terminal (found by the readiness-bar churn run, 2026-11-12).
//!
//! hyper drops a request's future when its client goes away. After the task row
//! is saved and the commit returns, the send read the queue map (an await that
//! yields when a queue is being made or destroyed) before it started the task
//! that stores the executor's events. A client that left at that await took the
//! events' channel with it while the executor, run by `DeferredExecutor`'s drop,
//! went on: its events had nowhere to be stored and the task stayed `Submitted`
//! for ever (1 in about 15,000 connections dropped early, loopback).
//!
//! The first test stops the send at that await (a gate in `applied_mark`,
//! compiled for tests only) and checks it is gone.
//!
//! The second is the one the 30,000-connection churn run found in the wild
//! (`tests/readiness_bar.rs`, 3 runs of 6 left one task `Submitted` for the whole
//! 120 s window): the row is written and the save has not yet returned when the
//! client leaves. The commit guard deletes a new task's row on a drop, but it
//! learned a row existed only when the save *returned*, so a drop while the
//! save was still finishing (a database's acknowledgement in flight, an
//! eviction sweep after the insert) left the row, with no executor, for ever.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use a2a_protocol_types::message::Message;
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams, SendMessageConfiguration};
use a2a_protocol_types::task::TaskState;

use super::*;
use crate::agent_executor;
use crate::builder::RequestHandlerBuilder;

struct Completes;
agent_executor!(Completes, |ctx, queue| async {
    crate::EventEmitter::new(ctx, queue)
        .status(TaskState::Completed)
        .await
});

#[derive(Clone, Copy, Debug)]
enum Shape {
    Blocking,
    Streaming,
    ReturnImmediately,
}

async fn non_terminal(handler: &RequestHandler) -> Vec<String> {
    let mut left = Vec::new();
    for state in [
        TaskState::Submitted,
        TaskState::Working,
        TaskState::InputRequired,
        TaskState::AuthRequired,
    ] {
        let mut params = ListTasksParams::default();
        params.status = Some(state);
        for t in handler.on_list_tasks(params, None).await.unwrap().tasks {
            left.push(format!("{} {:?}", t.id.0, t.status.state));
        }
    }
    left
}

fn params_for(shape: Shape) -> MessageSendParams {
    let mut params = MessageSendParams::new(Message::user_text("m-1", "go"));
    if matches!(shape, Shape::ReturnImmediately) {
        let mut configuration = SendMessageConfiguration::new();
        configuration.return_immediately = Some(true);
        params = params.with_configuration(configuration);
    }
    params
}

/// The send path reads nothing from the queue map between its commit and the
/// task that stores the executor's events, so there is no await there at which
/// a departing client could take the events' channel with it.
///
/// Before the fix the send awaited `applied_mark` there, and a send dropped at
/// that await (the gate holds it there) left its task `Submitted` for every
/// shape: `Blocking: left ["… Submitted"]`, `Streaming: …`, `ReturnImmediately: …`.
#[tokio::test(flavor = "current_thread")]
async fn a_committed_send_does_not_wait_on_the_queue_map_before_it_has_a_consumer() {
    for shape in [Shape::Blocking, Shape::Streaming, Shape::ReturnImmediately] {
        let handler = RequestHandlerBuilder::new(Completes)
            .without_agent_card()
            .build()
            .unwrap();
        let gate = handler.event_queue_manager.gate_applied_mark();
        let streaming = matches!(shape, Shape::Streaming);
        let sent = tokio::time::timeout(
            Duration::from_secs(5),
            handler.on_send_message(params_for(shape), streaming, None),
        )
        .await
        .unwrap_or_else(|_| panic!("{shape:?}: the send waited at the gate for ever"));
        drop(sent);
        let waited = tokio::time::timeout(Duration::ZERO, gate.reached.notified()).await;
        assert!(
            waited.is_err(),
            "{shape:?}: the send awaited `applied_mark`"
        );
    }
}

/// A blocking send dropped as soon as it has committed and handed its events to
/// the task that stores them: the task still ends (N27).
#[tokio::test(flavor = "current_thread")]
async fn a_blocking_send_dropped_after_its_commit_still_ends_its_task() {
    let handler = RequestHandlerBuilder::new(Completes)
        .without_agent_card()
        .build()
        .unwrap();
    {
        let mut send = Box::pin(handler.on_send_message(params_for(Shape::Blocking), false, None));
        let finished = std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::pin::Pin::new(&mut send).poll(cx).is_ready())
        })
        .await;
        // On this runtime the first poll commits and waits for the collector.
        assert!(!finished || non_terminal(&handler).await.is_empty());
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(non_terminal(&handler).await, Vec::<String>::new());
}

/// Writes the row, then never answers: the worst case of a save that is
/// still finishing when the client leaves.
struct WritesThenStalls {
    inner: crate::store::InMemoryTaskStore,
    written: std::sync::Arc<tokio::sync::Notify>,
    /// Stalls only while set, so a test can let its first send through.
    armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl WritesThenStalls {
    async fn finish(&self) {
        if self.armed.load(std::sync::atomic::Ordering::SeqCst) {
            self.written.notify_one();
            std::future::pending::<()>().await;
        }
    }
}

type Fut<'a, T> =
    Pin<Box<dyn Future<Output = a2a_protocol_types::error::A2aResult<T>> + Send + 'a>>;

impl crate::store::TaskStore for WritesThenStalls {
    fn save<'a>(&'a self, task: &'a a2a_protocol_types::task::Task) -> Fut<'a, ()> {
        Box::pin(async move {
            self.inner.save(task).await?;
            self.finish().await;
            Ok(())
        })
    }
    fn save_appending_history<'a>(
        &'a self,
        task: &'a a2a_protocol_types::task::Task,
        appended: &'a [Message],
        limit: crate::store::HistoryLimit,
    ) -> Fut<'a, ()> {
        Box::pin(async move {
            self.inner
                .save_appending_history(task, appended, limit)
                .await?;
            self.finish().await;
            Ok(())
        })
    }
    fn get<'a>(
        &'a self,
        id: &'a a2a_protocol_types::task::TaskId,
    ) -> Fut<'a, Option<a2a_protocol_types::task::Task>> {
        self.inner.get(id)
    }
    fn list<'a>(
        &'a self,
        params: &'a ListTasksParams,
    ) -> Fut<'a, a2a_protocol_types::responses::TaskListResponse> {
        self.inner.list(params)
    }
    fn insert_if_absent<'a>(&'a self, task: &'a a2a_protocol_types::task::Task) -> Fut<'a, bool> {
        self.inner.insert_if_absent(task)
    }
    fn delete<'a>(&'a self, id: &'a a2a_protocol_types::task::TaskId) -> Fut<'a, ()> {
        self.inner.delete(id)
    }
}

/// A send dropped after its new task's row is written and before the save
/// returns leaves no row.
#[tokio::test(flavor = "current_thread")]
async fn a_send_dropped_while_its_save_is_finishing_leaves_no_row() {
    for shape in [Shape::Blocking, Shape::Streaming, Shape::ReturnImmediately] {
        let written = std::sync::Arc::new(tokio::sync::Notify::new());
        let handler = RequestHandlerBuilder::new(Completes)
            .without_agent_card()
            .with_task_store(WritesThenStalls {
                inner: crate::store::InMemoryTaskStore::new(),
                written: std::sync::Arc::clone(&written),
                armed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            })
            .build()
            .unwrap();
        let streaming = matches!(shape, Shape::Streaming);
        {
            let mut send = Box::pin(handler.on_send_message(params_for(shape), streaming, None));
            tokio::select! {
                outcome = &mut send => panic!("the send finished: {:?}", outcome.map(|_| ())),
                () = written.notified() => {}
            }
            // The client has gone: the future is dropped with the row written.
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let rows = handler
            .on_list_tasks(ListTasksParams::default(), None)
            .await
            .unwrap()
            .tasks;
        assert_eq!(rows.len(), 0, "{shape:?}: left {rows:?}");
    }
}

/// Parks the task for input, so a second message continues it.
struct Parks;
agent_executor!(Parks, |ctx, queue| async {
    crate::EventEmitter::new(ctx, queue)
        .status(TaskState::InputRequired)
        .await
});

/// The same drop during a *continuation's* save must not remove the task it
/// continues: the guard rolls back a new row only.
#[tokio::test(flavor = "current_thread")]
async fn a_continuation_dropped_while_its_save_is_finishing_keeps_its_task() {
    let written = std::sync::Arc::new(tokio::sync::Notify::new());
    let armed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handler = RequestHandlerBuilder::new(Parks)
        .without_agent_card()
        .with_task_store(WritesThenStalls {
            inner: crate::store::InMemoryTaskStore::new(),
            written: std::sync::Arc::clone(&written),
            armed: std::sync::Arc::clone(&armed),
        })
        .build()
        .unwrap();
    let first = handler
        .on_send_message(params_for(Shape::Blocking), false, None)
        .await
        .unwrap();
    let crate::handler::SendMessageResult::Response(
        a2a_protocol_types::responses::SendMessageResponse::Task(parked),
    ) = first
    else {
        panic!("a task");
    };
    assert_eq!(parked.status.state, TaskState::InputRequired);

    armed.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut next = Message::user_text("m-2", "more");
    next.task_id = Some(parked.id.clone());
    next.context_id = Some(parked.context_id.clone());
    {
        let mut send = Box::pin(handler.on_send_message(MessageSendParams::new(next), false, None));
        tokio::select! {
            outcome = &mut send => panic!("the send finished: {:?}", outcome.map(|_| ())),
            () = written.notified() => {}
        }
    }
    armed.store(false, std::sync::atomic::Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let rows = handler
        .on_list_tasks(ListTasksParams::default(), None)
        .await
        .unwrap()
        .tasks;
    assert_eq!(rows.len(), 1, "the continued task was removed: {rows:?}");
}
