<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Writing events

The `EventQueueWriter` is the executor's only output. Everything the client
sees — status changes, artifacts, the final task — is an event written here.
This page shows how to write one directly and what the queue checks before
it accepts it.

## Writing to the queue

The queue is your channel for sending events back to the client:

```rust
# use a2a_protocol_sdk::prelude::*;
# async fn f(ctx: &RequestContext, queue: &dyn EventQueueWriter) -> A2aResult<()> {
// Write a status update
queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
    task_id: ctx.task_id.clone(),
    context_id: ContextId::new(ctx.context_id.clone()),
    status: TaskStatus::new(TaskState::Working),
    metadata: None,
})).await?;

// Write an artifact
queue.write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
    task_id: ctx.task_id.clone(),
    context_id: ContextId::new(ctx.context_id.clone()),
    artifact: Artifact::new("result", vec![Part::text("output")]),
    append: None,
    last_chunk: Some(true),
    metadata: None,
})).await?;
# Ok(())
# }
```
For **synchronous** clients (`SendMessage`), the handler collects all events and assembles the final `Task` response. For **streaming** clients (`SendStreamingMessage`), events are delivered as SSE in real time. Your executor doesn't need to know which mode the client used — just write events to the queue.

[`EventEmitter`](./helpers.md#eventemitter-helper) writes the same events with
less code.

## What the queue refuses

The queue checks what an executor writes before anything stores or sends
it, and refuses an event that leaves a field a2a.proto marks REQUIRED unset:
a message with no `messageId`, no role or no parts; an artifact with no
`artifactId` or no parts (an appended chunk included: end a streamed
artifact with `lastChunk: true` on the chunk that carries its last part, not
with an empty chunk); a status in `TASK_STATE_UNSPECIFIED`. That holds for a
message, a status update and its message, an artifact update, and a task
snapshot's status, artifacts and history. It refuses, too, what a client's
message is refused for because the gRPC binding cannot carry it: a `raw`
part that is not base64, an integer beyond ±2^53 in metadata or a `data`
part, a `metadata` that is not a JSON object. The write returns
`InvalidParams` naming the field, and `?` returns it from `execute`, which
fails the task.

A status, artifact or task event that leaves its `taskId` or `contextId`
empty is given the running task's; one that names another task or context is
refused with `InvalidParams`.

An agent message the executor puts on a status, or in a snapshot's history,
without a `contextId` or `taskId` is given the task's (a2a.proto: a server
message's `context_id` "must be provided"); ids it set are kept.
