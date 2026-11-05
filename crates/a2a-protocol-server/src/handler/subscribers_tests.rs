// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The subscriber bounds (`handler::subscribers`, audit H5): per task and on
//! the whole handler, refused retryably, and given back when a stream ends.

use a2a_protocol_types::params::TaskIdParams;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::handler::HandlerLimits;
use crate::{RequestHandler, agent_executor};

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

async fn handler(per_task: usize, total: usize, tasks: &[&str]) -> RequestHandler {
    let h = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_handler_limits(
            HandlerLimits::default()
                .with_max_subscribers_per_task(per_task)
                .with_max_subscribers(total),
        )
        .build()
        .unwrap();
    for id in tasks {
        let task = Task::new(
            TaskId::new(*id),
            ContextId::new("ctx"),
            TaskStatus::new(TaskState::Working),
        );
        h.task_store.save(&task).await.unwrap();
    }
    h
}

async fn subscribe(
    h: &RequestHandler,
    id: &str,
) -> Result<crate::streaming::InMemoryQueueReader, ServerError> {
    h.on_resubscribe(TaskIdParams::new(id.to_owned()), None)
        .await
}

fn overloaded(r: Result<crate::streaming::InMemoryQueueReader, ServerError>, what: &str) {
    match r {
        Err(ServerError::Overloaded(msg)) => assert!(msg.contains(what), "{msg}"),
        other => panic!("expected Overloaded naming {what}, got {other:?}"),
    }
}

/// Two streams on a task are allowed and a third is refused, retryably;
/// ending one lets the next in; another task is counted on its own.
#[tokio::test]
async fn a_task_has_at_most_its_bound_of_subscribers() {
    let h = handler(2, 100, &["a", "b"]).await;
    let first = subscribe(&h, "a").await.unwrap();
    let _second = subscribe(&h, "a").await.unwrap();
    overloaded(subscribe(&h, "a").await, "max_subscribers_per_task");
    assert_eq!(h.subscribers.open("", &TaskId::new("a")), (2, 2));
    let _other = subscribe(&h, "b").await.unwrap();
    drop(first);
    assert_eq!(h.subscribers.open("", &TaskId::new("a")), (2, 1));
    let _third = subscribe(&h, "a").await.unwrap();
}

/// The handler-wide bound counts streams on every task.
#[tokio::test]
async fn the_handler_has_at_most_its_bound_of_subscribers() {
    let h = handler(10, 2, &["a", "b", "c"]).await;
    let _a = subscribe(&h, "a").await.unwrap();
    let b = subscribe(&h, "b").await.unwrap();
    overloaded(subscribe(&h, "c").await, "max_subscribers");
    drop(b);
    let _c = subscribe(&h, "c").await.unwrap();
    assert_eq!(h.subscribers.open("", &TaskId::new("c")), (2, 1));
}

/// A refused subscription, and one to a task that does not exist, leave
/// nothing counted.
#[tokio::test]
async fn refusals_count_nothing() {
    let h = handler(1, 100, &["a"]).await;
    let held = subscribe(&h, "a").await.unwrap();
    overloaded(subscribe(&h, "a").await, "max_subscribers_per_task");
    assert!(matches!(
        subscribe(&h, "missing").await,
        Err(ServerError::TaskNotFound(_))
    ));
    assert_eq!(h.subscribers.open("", &TaskId::new("a")), (1, 1));
    drop(held);
    assert_eq!(h.subscribers.open("", &TaskId::new("a")), (0, 0));
}

/// The defaults are generous: 256 a task, 10,000 in all.
#[test]
fn the_defaults_are_generous() {
    let limits = HandlerLimits::default();
    assert_eq!(
        (limits.max_subscribers_per_task, limits.max_subscribers),
        (256, 10_000)
    );
}
