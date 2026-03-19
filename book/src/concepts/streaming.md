# Streaming with SSE

A2A uses **Server-Sent Events (SSE)** for real-time streaming. This enables agents to deliver progress updates, partial results, and artifacts as they're produced — instead of making the client wait for the complete response.

## How SSE Streaming Works

When a client calls `SendStreamingMessage`, the server holds the HTTP connection open and sends events as they occur:

```
HTTP/1.1 200 OK
Content-Type: text/event-stream

data: {"statusUpdate":{"taskId":"t-1","contextId":"ctx-1","status":{"state":"working"}}}

data: {"artifactUpdate":{"taskId":"t-1","contextId":"ctx-1","artifact":{"artifactId":"a-1","parts":[{"type":"text","text":"partial..."}]},"lastChunk":false}}

data: {"artifactUpdate":{"taskId":"t-1","contextId":"ctx-1","artifact":{"artifactId":"a-1","parts":[{"type":"text","text":"complete result"}]},"lastChunk":true}}

data: {"statusUpdate":{"taskId":"t-1","contextId":"ctx-1","status":{"state":"completed"}}}

```

Each `data:` line is a complete JSON object. Events are separated by blank lines.

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
      "state": "working",
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
      "parts": [{"type": "text", "text": "The answer is..."}]
    },
    "lastChunk": false,
    "append": false
  }
}
```

### Task

A complete task snapshot (usually the final event):

```json
{
  "task": {
    "id": "task-abc",
    "contextId": "ctx-123",
    "status": {"state": "completed"},
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
    "parts": [{"type": "text", "text": "Quick answer"}]
  }
}
```

## Server-Side: Writing Events

In your `AgentExecutor`, write events to the queue:

```rust
impl AgentExecutor for MyExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
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
| Queue capacity | 64 events | Broadcast channel ring buffer size |
| Max event size | 16 MiB | Rejects oversized events |

With broadcast channels, writes never block — if a reader is too slow, it receives a `Lagged` notification and skips missed events. The task store is the source of truth; SSE is best-effort notification.

Configure these via the builder:

```rust
RequestHandlerBuilder::new(executor)
    .with_event_queue_capacity(128)
    .with_max_event_size(8 * 1024 * 1024)  // 8 MiB
    .build()
    .unwrap()
```

## Client-Side: Consuming Streams

Use `stream_message` to receive events:

```rust
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
```

### Client Protections

The SSE parser includes safety limits:

- **16 MiB buffer cap** — Prevents OOM from malicious servers
- **30-second connect timeout** — Fails fast on unreachable servers
- **Partial line buffering** — Handles TCP frame boundaries correctly

## Re-subscribing

If a stream disconnects, re-subscribe to an existing task:

```rust
let mut stream = client
    .subscribe_to_task("task-abc")
    .await
    .expect("resubscribe");
```

The server creates a new broadcast subscriber for the task's event queue. Multiple SSE connections can be active simultaneously for the same task — each receives all events published after it subscribes. If a reader falls behind, it receives a `Lagged` notification and skips missed events rather than blocking other readers or the writer.

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
