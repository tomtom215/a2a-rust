# Streaming with SSE

A2A uses **Server-Sent Events (SSE)** for real-time streaming. This enables agents to deliver progress updates, partial results, and artifacts as they're produced — instead of making the client wait for the complete response.

## How SSE Streaming Works

When a client calls `SendStreamingMessage`, the server holds the HTTP connection open and sends events as they occur:

```text
HTTP/1.1 200 OK
Content-Type: text/event-stream

data: {"statusUpdate":{"taskId":"t-1","contextId":"ctx-1","status":{"state":"TASK_STATE_WORKING"}}}

data: {"artifactUpdate":{"taskId":"t-1","contextId":"ctx-1","artifact":{"artifactId":"a-1","parts":[{"text":"partial..."}]},"lastChunk":false}}

data: {"artifactUpdate":{"taskId":"t-1","contextId":"ctx-1","artifact":{"artifactId":"a-1","parts":[{"text":"complete result"}]},"lastChunk":true}}

data: {"statusUpdate":{"taskId":"t-1","contextId":"ctx-1","status":{"state":"TASK_STATE_COMPLETED"}}}

```

Each `data:` line is a complete JSON object. Events are separated by blank lines.

Shown in the HTTP+JSON form, trimmed to the update events; over JSON-RPC each
`data:` is a JSON-RPC success response whose `result` is this object. This
repository's server also sends the task's `Task` snapshot first, and precedes
each frame with `event: message` and, for agent-emitted events, an `id:` line
(see [Resuming from where you left off](#resuming-from-where-you-left-off)).

## Stream Event Types

Four types of events can appear in a stream:

### StatusUpdate

Reports a task state transition:

```json
{
  "statusUpdate": {
    "taskId": "task-abc",
    "contextId": "ctx-123",
    "status": {
      "state": "TASK_STATE_WORKING",
      "timestamp": "2026-03-15T10:30:00Z"
    }
  }
}
```

### ArtifactUpdate

Delivers artifact content (potentially in chunks):

```json
{
  "artifactUpdate": {
    "taskId": "task-abc",
    "contextId": "ctx-123",
    "artifact": {
      "artifactId": "result-1",
      "parts": [{"text": "The answer is..."}]
    },
    "lastChunk": false,
    "append": false
  }
}
```

#### Append Semantics

When `append: true`, the server merges the event into the existing artifact with
the same ID:

- **Parts** are appended to the existing artifact's parts list
- **Metadata** is merged key by key at the top level: new keys override existing keys
- If no artifact with the matching ID exists, a new artifact is created
- A chunk must carry at least one part, as every `Artifact` must; the queue
  refuses an empty one, so `lastChunk: true` goes on the chunk with the last part

### Task

A complete task snapshot (usually the first event on subscribe, or the final event):

```json
{
  "task": {
    "id": "task-abc",
    "contextId": "ctx-123",
    "status": {"state": "TASK_STATE_COMPLETED"},
    "artifacts": [...]
  }
}
```

### Message

A direct message response (for simple request/reply patterns):

```json
{
  "message": {
    "messageId": "msg-456",
    "role": "ROLE_AGENT",
    "parts": [{"text": "Quick answer"}]
  }
}
```

A `Message` is the whole stream only when it is the executor's first event
for a new task (spec §3.1.2: such a stream holds exactly one `Message`, with
"no task tracking"). A follow-up to an existing task has no direct reply:
every `Message` its executor writes, the first included, is sent as below,
so the follow-up's stream begins with the `Task` (§3.7: agents "attach
Messages to status update events"). If the executor answers a follow-up
with messages alone and sets no state, the task goes back to waiting where
it waited (`INPUT_REQUIRED` or `AUTH_REQUIRED`), with the last message as
its status message. A `Message` the
executor writes once the task stream has begun, after a status update, an
artifact or a task snapshot, is sent as a status update that keeps the
task's state and carries the message in `status.message`: a task stream
holds status and artifact updates only. The task store appends it to the
task's `history` as before and, while the task is not finished, holds it as
the status message, as a client applying the update does: written while the
task waits in `INPUT_REQUIRED` or `AUTH_REQUIRED`, it replaces the status
message that asked for the input. Before any status the state is the one
the turn began in, `SUBMITTED`. A `Message` written after the task's terminal
status is stored in `history` and reaches no stream, as every stream has
ended.

## Server-Side: Writing Events

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

### Queue Limits

The event queue uses `tokio::sync::broadcast` channels for fan-out to multiple subscribers:

| Limit | Default | Purpose |
|-------|---------|---------|
| Queue capacity | 256 events | Broadcast channel ring buffer size |
| Max event size | 16 MiB | Rejects oversized events |

With broadcast channels, writes never block on readers. A reader that falls more than the queue capacity behind catches up from the task's event log (spec §3.5.2): it reads the events it missed from the store, after the last position it delivered, and goes on, so a slow stream receives the same events in the same order as a fast one. Every built-in store keeps a log: the SQL stores keep a task's whole log, and the in-memory ones the last 512 events of a task by default (`TaskStoreConfig::max_events_per_task`). An attached stream holds the events it still owes, so the in-memory log keeps them past that bound, up to `TaskStoreConfig::max_held_events_factor` times it (8 × 512 = 4,096 events by default; since 2026-10-20). A replay longer than `HandlerLimits::subscribe_replay_limit` (1,000 by default) is read a page at a time. Only a complete replay is used. When the log cannot supply every missed event, because the store keeps none or the reader fell further behind than even a held log keeps, the stream is cut off instead: it ends with one `event: error` frame carrying the `streamLagged` signal (`ClientError::is_stream_lagged()` on the client). It gets a contiguous prefix and an announced end, never a silent gap, and resubscribing (with `Last-Event-ID` to replay) continues it. The task store is the source of truth; SSE is best-effort notification. The one write that waits is the terminal one, on the store rather than on readers (see [The terminal frame is the stored one](#the-terminal-frame-is-the-stored-one)).

> **High-volume streams:** For tasks producing >250 events, increase the queue
> capacity to match expected peak volume. The default capacity of 256 is sufficient
> for most use cases. A slower reader catches up from the event log, so what
> ends a stream with `streamLagged` is the log's retention and the replay
> limit, not the queue.

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

## Client-Side: Consuming Streams

Use `stream_message` to receive events:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn f(client: A2aClient, params: MessageSendParams) {
let mut stream = client
    .stream_message(params)
    .await
    .expect("connect");

while let Some(event) = stream.next().await {
    match event {
        Ok(StreamResponse::StatusUpdate(ev)) => {
            println!("State: {:?}", ev.status.state);
        }
        Ok(StreamResponse::ArtifactUpdate(ev)) => {
            println!("Artifact: {}", ev.artifact.id);
        }
        Ok(StreamResponse::Task(task)) => {
            println!("Final task: {:?}", task.status.state);
        }
        Ok(StreamResponse::Message(msg)) => {
            println!("Message: {:?}", msg);
        }
        Ok(_) => {
            // Future event types — handle gracefully
        }
        Err(e) => {
            eprintln!("Stream error: {e}");
            break;
        }
    }
}
# }
```

An error that answers `e.is_stream_lagged()` means this reader was cut off for
falling behind; the task is unaffected, so resubscribe rather than give up (see
[Streaming Responses](../client/streaming.md#how-a-stream-ends)).

### Client Protections

The SSE parser includes safety limits:

- **16 MiB event cap** — An oversized event, or a line that never ends, is refused with an error rather than buffered (`with_max_event_size`)
- **30-second stream connect timeout** — Bounds waiting for the stream's response headers (TCP connect itself is `with_connection_timeout`, 10 seconds)
- **First-event timeout** — A stream that is accepted but silent before its first data times out after 5 minutes by default (`with_stream_first_event_timeout`; separate from the 30-second connect timeout), on every transport
- **Idle timeout** — After the first frame, a stream that receives nothing at all (keep-alive comments count) for 5 minutes by default ends with `ClientError::Timeout`; resubscribe to continue
- **Partial line buffering** — Handles TCP frame boundaries correctly (CRLF, LF, and bare-CR line endings per the SSE spec)

### Errors on the wire

A streaming call can fail before its stream starts (unknown task, invalid
params, streaming not advertised) or partway through (an executor failure, the
`streamLagged` signal).

**Before the stream starts**, the error is not SSE:

- over JSON-RPC it is a plain `application/json` JSON-RPC error response,
  HTTP 200 (a refused credential answers HTTP 401/403 instead);
- over HTTP+JSON it is an HTTP error status with a `google.rpc.Status` body.

The official conformance kit (a2aproject/a2a-tck) requires the JSON-RPC shape,
because it reads any `text/event-stream` answer as a successful stream. One
peer loses it: a2a-go v2.5.0's client reads a streaming answer only as SSE, so
over JSON-RPC it sees an empty stream and no error. A Go client that needs
typed errors from a stream that fails to open should use HTTP+JSON or gRPC.

**Partway through**, the server writes the error inside the SSE body as one
`event: error` frame, then closes it:

| Binding | Error frame `data:` |
|---|---|
| JSON-RPC | the JSON-RPC error response, echoing the request id: `{"jsonrpc":"2.0","id":1,"error":{"code":-32001,…}}` |
| HTTP+JSON | a `google.rpc.Status` (§11.6): `{"error":{"code":404,"status":"NOT_FOUND","message":…,"details":[…]}}`; `A2aError::data` rides as a flattened `google.protobuf.Struct` detail |

This client accepts every shape a peer sends: a JSON-RPC refusal as SSE, as a
plain JSON body (this server, and the Python SDK's), or as an
HTTP status all fail the call itself with `ClientError::Protocol`. The one
exception is a peer that opens a live, chunked stream and then sends the error
as its first frame (a2a-go's server): that cannot be told from a stream that
has started, so the same `ClientError::Protocol` arrives as the first
`next()`. Handle errors in both places.

## Re-subscribing

If a stream disconnects, re-subscribe to an existing task:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn f(client: A2aClient) {
let mut stream = client
    .subscribe_to_task("task-abc")
    .await
    .expect("resubscribe");
# }
```

The server creates a new broadcast subscriber and immediately emits a `Task`
snapshot as the first event, allowing the client to recover the current state.
Multiple SSE connections can be active simultaneously for the same task — each
receives all events published after it subscribes. A reader that falls behind
by more than the queue capacity catches up from the task's event log; only when
the log cannot supply what it missed does its stream end with a `streamLagged`
error frame. Either way it never blocks other readers or the writer; after a
`streamLagged`, resubscribe with `subscribe_to_task_from` to continue.

> **Terminal tasks:** Subscribing to a task in a terminal state
> (`Completed`, `Failed`, `Canceled`, `Rejected`) returns an
> `UnsupportedOperation` error immediately. No stream is opened: on JSON-RPC
> the answer is a plain `application/json` error response (see
> [Errors on the wire](#errors-on-the-wire)).

### Resuming from where you left off

With `a2a-protocol-client`, a broken stream tells you: `next()` yields
`ClientError::IncompleteStream` when the body ends before the stream's final
event, carrying the last `id:` received (also available as
`EventStream::last_event_id()`). Pass it to
`client.subscribe_to_task_from(task_id, id)`, which sends it as
`Last-Event-ID` on JSON-RPC and HTTP+JSON. The wire contract underneath:

A snapshot tells you where the task *is*, not what happened while you were
disconnected. An agent that emitted three progress updates during the outage
folds them into one state, and a client polling or resubscribing sees one.

Every frame carrying an event the agent emitted therefore also carries an SSE
`id:`, which is that event's position in the task's event log:

```text
id: 7
event: message
data: {"statusUpdate":{"taskId":"task-abc", ...}}
```

Send the last one you saw back as `Last-Event-ID` on the resubscribe, and the
server replays the log from exactly there — after the snapshot, before the
live stream:

```text
POST /tasks/task-abc:subscribe
Last-Event-ID: 7
```

Details worth knowing:

- **The offset is exclusive.** `Last-Event-ID: 7` returns 8 onward. `0` asks
  for the whole history.
- **Frames without an `id:` are not in the log.** The `Task` snapshot and the
  terminal frame the server rebuilds from stored state are server-synthesized
  rather than agent-emitted, so they carry no position and do not move your
  offset.
- **The replay is bounded** by `HandlerLimits::subscribe_replay_limit`
  (default 1,000). Truncation is not loss: each replayed frame carries its own
  `id:`, so reconnect at the last one you received and continue.
- **It needs a store that keeps a log.** Every store this crate ships does
  (`TaskStore::supports_event_log`). A custom store that does not gets the
  snapshot and the live stream, which is the behaviour from before resumption
  existed — the header is ignored rather than refused.
- **A malformed `Last-Event-ID` is ignored**, not rejected, so echoing back an
  id from an unrelated stream costs you a replay, not the connection.

### The terminal frame is the stored one

Every frame but the last is broadcast as soon as the agent emits it, without
waiting for the store. The frame carrying a terminal state is held until the
server has persisted it (bounded by the queue's write timeout, 5 s by
default), and it goes out as the store ruled: the agent's own frame when it
persisted, or the state the store already holds when another writer finished
the task first — a `CancelTask` handled by another replica, typically. The
log records the same frame at that position, so a resumed stream ends the way
the live one did, and a `GetTask` made after reading the terminal frame
agrees with it. If the store has not answered within the timeout, the frame
goes out as the agent wrote it, which is how every frame behaved before.

The trade is latency on that one frame: it now waits for the store write, and
for the processing of any events queued ahead of it.

## Streaming vs Synchronous

| Aspect | SendMessage | SendStreamingMessage |
|--------|-------------|---------------------|
| Response | Complete task | SSE event stream |
| Progress | No intermediate updates | Real-time updates |
| Long tasks | Client waits | Client sees progress |
| Network | Single request/response | Held connection |
| Complexity | Simple | Requires event handling |

Use streaming when:
- Tasks take more than a few seconds
- You want to show progress to users
- You need incremental artifact delivery

Use synchronous when:
- Tasks complete quickly
- You don't need progress updates
- Simplicity is more important than responsiveness

## Next Steps

- **[Building a Client](../client/builder.md)** — Client configuration for streaming
- **[Streaming Responses](../client/streaming.md)** — Advanced client streaming patterns
- **[The AgentExecutor Trait](../building-agents/executor.md)** — Writing streaming executors
