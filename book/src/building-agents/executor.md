# The AgentExecutor Trait

The `AgentExecutor` trait is the heart of every A2A agent. It defines what happens when a message arrives. Everything else — HTTP handling, task management, streaming — is handled by the framework.

## The Trait

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::{A2aResult, EventQueueWriter, RequestContext};
pub trait AgentExecutor: Send + Sync + 'static {
    /// Called when a message arrives (SendMessage or SendStreamingMessage).
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Called when a client requests task cancellation.
    /// Default: emits the terminal `Canceled` status and returns `Ok(())`.
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
# // The signatures above are the real trait's: this impl of it uses them.
# struct Probe;
# impl a2a_protocol_sdk::server::AgentExecutor for Probe {
#     fn execute<'a>(&'a self, _: &'a RequestContext, _: &'a dyn EventQueueWriter)
#         -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
#     fn cancel<'a>(&'a self, _: &'a RequestContext, _: &'a dyn EventQueueWriter)
#         -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
#     fn on_shutdown<'a>(&'a self) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
#         Box::pin(async {})
#     }
# }
```
### Why `Pin<Box<dyn Future>>`?

This signature ensures **object safety** — the trait can be stored as `Arc<dyn AgentExecutor>` and shared across threads. Standard `async fn` in traits would prevent this. The `Box::pin(async move { ... })` wrapper is the idiomatic pattern:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
struct MyAgent;

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
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
use a2a_protocol_server::executor_helpers::boxed_future;
# struct MyAgent;
# impl AgentExecutor for MyAgent {

fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter)
    -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>
{
    boxed_future(async move {
        // Your logic here — no Box::pin wrapper needed!
        Ok(())
    })
}
# }
```
### `agent_executor!` macro

Generates the full `AgentExecutor` impl from a closure-like syntax:

```rust
use a2a_protocol_server::agent_executor;

struct EchoAgent;
struct CancelableAgent;

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
# use a2a_protocol_sdk::prelude::*;
use a2a_protocol_server::executor_helpers::EventEmitter;

struct MyAgent;

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
| `fail(FailureClass, reason)` | Emit the terminal `Failed` status, with the reason and a failure class a caller can branch on |
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
| `call_context` | `Option<CallContext>` | The call this execution belongs to (see below) |

`RequestContext` is `#[non_exhaustive]` as of 0.13 — build one with
`RequestContext::new` and the `with_*` methods, not a struct literal.

### What the caller sent

Before 0.13 an executor could see nothing about the caller. The handler built
a `CallContext` with the caller's identity, the resolved tenant, the HTTP
headers and the activated extensions, handed it to the interceptor chain, and
then dropped it. That ruled out a whole class of deployment — an executor
could not enforce "only this tenant may invoke this skill" — and left
`Message.metadata` as the only channel for anything caller-specific, which
the *caller* writes and so is not a fact about the caller at all.

Five accessors read it, each returning `None` rather than a default when
nobody said:

| Accessor | Returns |
|----------|---------|
| `ctx.caller_identity()` | Who the caller is, once an authenticating interceptor established it |
| `ctx.tenant()` | The tenant this call resolved to, after any `TenantResolver` |
| `ctx.http_header(name)` | One inbound header, matched case-insensitively |
| `ctx.activated_extensions()` | The URIs from the `A2A-Extensions` header (spec §14.2.2) |
| `ctx.request_id()` | The caller's `X-Request-ID`, if they sent one |

```rust
# use a2a_protocol_sdk::prelude::*;
# fn entitled(_tenant: &str, _skill: &str) -> bool { true }
/// Refuse work the caller's tenant is not entitled to.
fn check_entitlement(ctx: &RequestContext) -> A2aResult<()> {
    let Some(tenant) = ctx.tenant() else {
        return Err(A2aError::invalid_params("this skill requires a tenant"));
    };
    if !entitled(tenant, "premium-analysis") {
        return Err(A2aError::invalid_params("skill not enabled for this tenant"));
    }
    Ok(())
}
# fn main() {
#     let ctx = RequestContext::new(
#         Message::user_text("m1", "hi"), TaskId::new("t1"), "c1".to_owned());
#     assert!(check_entitlement(&ctx).is_err(), "no tenant means refusal");
# }
```

Use `ctx.tenant()` rather than `TenantContext::current()` inside an executor.
The latter is a `tokio::task_local` and the executor runs in a spawned task;
`ctx.tenant()` is an owned copy taken before the spawn. (The spawn now
re-enters the tenant scope as well, so the task-local is correct too — but
the field is the one that cannot silently become empty.)

`call_context` is `None` when the executor is driven directly, as a unit test
or a conformance harness does. That is the honest answer there rather than a
synthetic context claiming a call that never happened.

## EventQueueWriter

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

## Common Patterns

### The Standard Three-Event Pattern

Most executors follow this structure:

```rust
# use a2a_protocol_sdk::prelude::*;
# async fn do_work(_: &Message) -> A2aResult<String> { Ok(String::new()) }
struct MyAgent;

agent_executor!(MyAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);

    // 1. Working
    emit.status(TaskState::Working).await?;

    // 2. Produce results
    let result = do_work(&ctx.message).await?;
    emit.artifact("result", vec![Part::text(result)], None, Some(true)).await?;

    // 3. Completed
    emit.status(TaskState::Completed).await?;

    Ok(())
});
```
### Error Handling

If your executor encounters an error, transition to `Failed` with a descriptive message. `EventEmitter::fail` writes that status, and records a failure class a caller can branch on — `Transient` for "retry me", `Internal` for a fault here:

```rust
# use a2a_protocol_sdk::prelude::*;
# async fn do_work(_: &Message) -> Result<String, std::io::Error> { Ok(String::new()) }
use a2a_protocol_sdk::types::failure::FailureClass;

struct MyAgent;

agent_executor!(MyAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;

    match do_work(&ctx.message).await {
        Ok(result) => {
            emit.artifact("result", vec![Part::text(result)], None, Some(true)).await?;
            emit.status(TaskState::Completed).await?;
        }
        Err(e) => {
            emit.fail(FailureClass::Internal, format!("Error: {e}")).await?;
        }
    }

    Ok(())
});
```

Returning `Err` also ends the task `Failed`, with the error's text; the class
is then inferred from its code, which can only say `InvalidRequest` or
`Internal`.
### Requesting More Input

When the agent needs clarification:

```rust
# use a2a_protocol_sdk::prelude::*;
# async fn f(ctx: &RequestContext, queue: &dyn EventQueueWriter) -> A2aResult<()> {
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
# Ok(())
# }
```

The client can then send another message with the same `context_id` to continue the conversation.

### Supporting Cancellation

Every executor supports cancellation without writing anything. By the time
`cancel` runs, the handler has triggered `ctx.cancellation_token`, which a
running `execute` should observe (`EventEmitter::is_cancelled`), and the
default `cancel` emits the terminal `Canceled` status.

Override `cancel` when the task holds something that must be released:

```rust
# use std::future::Future;
# use std::pin::Pin;
use std::collections::HashSet;
use std::sync::Mutex;
# use a2a_protocol_sdk::prelude::*;

struct GpuAgent {
    // Tasks holding a GPU slot.
    reserved: Mutex<HashSet<TaskId>>,
}

impl AgentExecutor for GpuAgent {
#     fn execute<'a>(&'a self, _: &'a RequestContext, _: &'a dyn EventQueueWriter)
#         -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
    fn cancel<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Release what the task holds...
            if let Ok(mut reserved) = self.reserved.lock() {
                reserved.remove(&ctx.task_id);
            }
            // ...then do what the default does: tell subscribers.
            EventEmitter::new(ctx, queue).status(TaskState::Canceled).await
        })
    }
}
```

### Executor with State

Executors can hold state — database connections, model handles, configuration:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use std::sync::Arc;
# use a2a_protocol_sdk::prelude::*;
# struct Model;
# impl Model { async fn generate(&self, _: &str, _: usize) -> A2aResult<String> { Ok(String::new()) } }
# struct DatabasePool;
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
            let input = ctx.message.text().unwrap_or_default();
            let response = self.model.generate(input, self.max_tokens).await?;
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
# use a2a_protocol_sdk::prelude::*;
# fn f(my_executor: impl AgentExecutor) -> ServerResult<RequestHandler> {
use std::time::Duration;

RequestHandlerBuilder::new(my_executor)
    .with_executor_timeout(Duration::from_secs(300))  // 5 minutes
    .build()
# }
```
The default is one hour. If the executor doesn't complete within the timeout, the task transitions to `Failed` automatically, with the failure class `BudgetExhausted`: the same call would hit the same bound.

## Next Steps

- **[Request Handler & Builder](./handler.md)** — Configuring the handler around your executor
- **[Push Notifications](./push-notifications.md)** — Delivering results asynchronously
