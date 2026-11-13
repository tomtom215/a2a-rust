// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The handler's final-snapshot hook reads the task only once the queue's
//! consumer has stored what the stream delivered (spec §11.7).
//!
//! The stream's last frame is built from the store, and the store trails the
//! stream while the consumer is storing an event. A hook that read at once
//! would find the task still `working`, send nothing, and the stream would end
//! without the `Task` the spec asks for. Time is paused.

use std::time::Duration;

use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};

use crate::agent_executor;
use crate::builder::RequestHandlerBuilder;
use crate::handler::HandlerLimits;

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

fn task(state: TaskState) -> Task {
    Task::new(
        TaskId::new("t"),
        ContextId::new("c"),
        TaskStatus::new(state),
    )
}

/// With a live queue, the hook waits for the consumer: here position 3 is being
/// stored when the stream asks, and the stored task is terminal a moment later.
#[tokio::test(start_paused = true)]
async fn the_final_snapshot_waits_for_the_consumer_of_a_live_queue() {
    let handler = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_handler_limits(
            HandlerLimits::default()
                .with_final_task_snapshot(true)
                .with_subscribe_replay_catchup(Duration::from_secs(5)),
        )
        .build()
        .unwrap();
    let id = TaskId::new("t");
    handler
        .task_store
        .save(&task(TaskState::Working))
        .await
        .unwrap();
    let (writer, _reader) = handler.event_queue_manager.get_or_create(&id).await;
    let mark = writer.applied_mark();
    let applying = mark.applying(3);

    let hook = handler
        .final_snapshot_hook(id, |_task| {})
        .expect("the hook is on");
    let consumer = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        handler
            .task_store
            .save(&task(TaskState::Completed))
            .await
            .unwrap();
        applying.done();
    };
    let (snapshot, ()) = tokio::join!(hook(3), consumer);

    assert!(
        matches!(&snapshot, Some(StreamResponse::Task(t)) if t.status.state == TaskState::Completed),
        "the snapshot read before the consumer had stored the ending: {snapshot:?}"
    );
}
