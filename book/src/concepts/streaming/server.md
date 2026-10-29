<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Writing events and queue limits

This page covers the server side of a stream: how an executor writes events,
and how the event queue delivers them to every reader without letting a slow
one hold up the others.

## Writing events

In your `AgentExecutor`, write events to the queue:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# async fn produce(_: &Message) -> Vec<String> { vec![] }
impl AgentExecutor for MyExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let results = produce(&ctx.message).await;

            // Signal start
            queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                status: TaskStatus::new(TaskState::Working),
                metadata: None,
            })).await?;

            // Deliver results in chunks
            for (i, chunk) in results.iter().enumerate() {
                queue.write(StreamResponse::ArtifactUpdate(
                    TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact: Artifact::new("output", vec![Part::text(chunk)]),
                        append: Some(i > 0),
                        last_chunk: Some(i == results.len() - 1),
                        metadata: None,
                    }
                )).await?;
            }

            // Signal completion
            queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                status: TaskStatus::new(TaskState::Completed),
                metadata: None,
            })).await?;

            Ok(())
        })
    }
}
```

## Queue limits

The event queue uses `tokio::sync::broadcast` channels for fan-out to multiple subscribers:

| Limit | Default | Purpose |
|-------|---------|---------|
| Queue capacity | 256 events | Broadcast channel ring buffer size |
| Max event size | 16 MiB | Rejects oversized events |

With broadcast channels, writes never block on readers. A reader that falls more than the queue capacity behind catches up from the task's event log (spec §3.5.2): it reads the events it missed from the store, after the last position it delivered, and goes on, so a slow stream receives the same events in the same order as a fast one. Every built-in store keeps a log: the SQL stores keep a task's whole log, and the in-memory ones the last 512 events of a task by default (`TaskStoreConfig::max_events_per_task`). An attached stream holds the events it still owes, so the in-memory log keeps them past that bound, up to `TaskStoreConfig::max_held_events_factor` times it (8 × 512 = 4,096 events by default; since 2026-10-20). A replay longer than `HandlerLimits::subscribe_replay_limit` (1,000 by default) is read a page at a time. Only a complete replay is used. When the log cannot supply every missed event, because the store keeps none or the reader fell further behind than even a held log keeps, the stream is cut off instead: it ends with one `event: error` frame carrying the `streamLagged` signal (`ClientError::is_stream_lagged()` on the client). It gets a contiguous prefix and an announced end, never a silent gap, and resubscribing (with `Last-Event-ID` to replay) continues it. The task store is the source of truth; SSE is best-effort notification. The one write that waits is the terminal one, on the store rather than on readers (see [The terminal frame is the stored one](./resubscribe.md#the-terminal-frame-is-the-stored-one)).

> **High-volume streams:** A slower reader catches up from the task's event
> log, so the queue capacity does not decide whether it loses events. A stream
> ends with `streamLagged` only when the log cannot supply what it missed: the
> store keeps no log, the log no longer reaches back far enough
> (`max_events_per_task`, extended up to `max_held_events_factor` times while a
> stream owes events), or a page of the log could not be read. Raise the
> capacity only to reduce how often readers have to go to the log; the default
> of 256 is sufficient for most use cases.

Configure these via the builder:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
# let executor = MyAgent;
# let _handler =
RequestHandlerBuilder::new(executor)
    .with_agent_card(card())
    .with_event_queue_capacity(512)  // increase above 256 default for high-volume streams
    .with_max_event_size(8 * 1024 * 1024)  // 8 MiB
    .build()
    .unwrap()
# ;
# }
```
