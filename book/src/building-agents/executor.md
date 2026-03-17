# The AgentExecutor Trait

The `AgentExecutor` trait is the heart of every A2A agent. It defines what happens when a message arrives. Everything else — HTTP handling, task management, streaming — is handled by the framework.

## The Trait

```rust
pub trait AgentExecutor: Send + Sync + 'static {
    /// Called when a message arrives (SendMessage or SendStreamingMessage).
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Called when a client requests task cancellation.
    /// Default: returns "task not cancelable" error.
    fn cancel<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Called during graceful server shutdown.
    /// Default: no-op.
    fn on_shutdown<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}
```

### Why `Pin<Box<dyn Future>>`?

This signature ensures **object safety** — the trait can be stored as `Arc<dyn AgentExecutor>` and shared across threads. Standard `async fn` in traits would prevent this. The `Box::pin(async move { ... })` wrapper is the idiomatic pattern:

```rust
impl AgentExecutor for MyAgent {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Your async logic here
            Ok(())
        })
    }
}
```

## Ergonomic Helpers

The `executor_helpers` module provides shortcuts to reduce boilerplate.

### `boxed_future` helper

Wraps an async block into the required `Pin<Box<dyn Future>>`:

```rust
use a2a_protocol_server::executor_helpers::boxed_future;

fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter)
    -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>
{
    boxed_future(async move {
        // Your logic here — no Box::pin wrapper needed!
        Ok(())
    })
}
```

### `agent_executor!` macro

Generates the full `AgentExecutor` impl from a closure-like syntax:

```rust
use a2a_protocol_server::agent_executor;

struct EchoAgent;

// Simple form (execute only)
agent_executor!(EchoAgent, |ctx, queue| async {
    Ok(())
});

// With cancel handler
agent_executor!(CancelableAgent,
    execute: |ctx, queue| async { Ok(()) },
    cancel: |ctx, queue| async { Ok(()) }
);
```

### `EventEmitter` helper

Eliminates the repetitive `task_id.clone()` / `context_id.clone()` in every event:

```rust
use a2a_protocol_server::executor_helpers::EventEmitter;

agent_executor!(MyAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);

    emit.status(TaskState::Working).await?;
    emit.artifact("result", vec![Part::text("done")], None, Some(true)).await?;

    if emit.is_cancelled() {
        emit.status(TaskState::Canceled).await?;
        return Ok(());
    }

    emit.status(TaskState::Completed).await?;
    Ok(())
});
```

| Method | Description |
|--------|-------------|
| `status(TaskState)` | Emit a status update event |
| `artifact(id, parts, append, last_chunk)` | Emit an artifact update event |
| `is_cancelled()` | Check if the task was cancelled |

## RequestContext

The `RequestContext` provides information about the incoming request:

| Field | Type | Description |
|-------|------|-------------|
| `task_id` | `TaskId` | Server-assigned task ID |
| `context_id` | `String` | Conversation context ID |
| `message` | `Message` | The incoming message with parts |
| `stored_task` | `Option<Task>` | Previously stored task snapshot (for continuations) |
| `metadata` | `Option<Value>` | Arbitrary metadata from the request |
| `cancellation_token` | `CancellationToken` | Token for cooperative cancellation |

## EventQueueWriter

The queue is your channel for sending events back to the client:

```rust
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
```

For **synchronous** clients (`SendMessage`), the handler collects all events and assembles the final `Task` response. For **streaming** clients (`SendStreamingMessage`), events are delivered as SSE in real time. Your executor doesn't need to know which mode the client used — just write events to the queue.

## Common Patterns

### The Standard Three-Event Pattern

Most executors follow this structure:

```rust
Box::pin(async move {
    // 1. Working
    queue.write(StreamResponse::StatusUpdate(/* Working */)).await?;

    // 2. Produce results
    let result = do_work(&ctx.message).await?;
    queue.write(StreamResponse::ArtifactUpdate(/* result */)).await?;

    // 3. Completed
    queue.write(StreamResponse::StatusUpdate(/* Completed */)).await?;

    Ok(())
})
```

### Error Handling

If your executor encounters an error, transition to `Failed` with a descriptive message:

```rust
Box::pin(async move {
    queue.write(/* Working */).await?;

    match do_work(&ctx.message).await {
        Ok(result) => {
            queue.write(/* ArtifactUpdate with result */).await?;
            queue.write(/* Completed */).await?;
        }
        Err(e) => {
            queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                status: TaskStatus {
                    state: TaskState::Failed,
                    message: Some(Message {
                        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
                        role: MessageRole::Agent,
                        parts: vec![Part::text(&format!("Error: {e}"))],
                        task_id: None,
                        context_id: None,
                        reference_task_ids: None,
                        extensions: None,
                        metadata: None,
                    }),
                    timestamp: None,
                },
                metadata: None,
            })).await?;
        }
    }

    Ok(())
})
```

### Requesting More Input

When the agent needs clarification:

```rust
queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
    task_id: ctx.task_id.clone(),
    context_id: ContextId::new(ctx.context_id.clone()),
    status: TaskStatus {
        state: TaskState::InputRequired,
        message: Some(Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::Agent,
            parts: vec![Part::text("Which format would you like: PDF or HTML?")],
            // ...remaining fields
            task_id: None, context_id: None, reference_task_ids: None,
            extensions: None, metadata: None,
        }),
        timestamp: None,
    },
    metadata: None,
})).await?;
```

The client can then send another message with the same `context_id` to continue the conversation.

### Supporting Cancellation

Override the `cancel` method:

```rust
fn cancel<'a>(
    &'a self,
    ctx: &'a RequestContext,
    queue: &'a dyn EventQueueWriter,
) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
    Box::pin(async move {
        // Clean up any in-progress work
        self.cancel_token.cancel();

        queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            status: TaskStatus::new(TaskState::Canceled),
            metadata: None,
        })).await?;

        Ok(())
    })
}
```

### Executor with State

Executors can hold state — database connections, model handles, configuration:

```rust
struct LlmExecutor {
    model: Arc<Model>,
    db: Arc<DatabasePool>,
    max_tokens: usize,
}

impl AgentExecutor for LlmExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Access self.model, self.db, self.max_tokens
            let response = self.model.generate(&input, self.max_tokens).await?;
            // ...
            Ok(())
        })
    }
}
```

Because the trait requires `Send + Sync + 'static`, the executor must be safe to share across threads. Use `Arc` for shared state.

## Executor Timeout

The builder can set a timeout that kills hung executors:

```rust
use std::time::Duration;

RequestHandlerBuilder::new(my_executor)
    .with_executor_timeout(Duration::from_secs(300))  // 5 minutes
    .build()
```

If the executor doesn't complete within the timeout, the task transitions to `Failed` automatically.

## Next Steps

- **[Request Handler & Builder](./handler.md)** — Configuring the handler around your executor
- **[Push Notifications](./push-notifications.md)** — Delivering results asynchronously
