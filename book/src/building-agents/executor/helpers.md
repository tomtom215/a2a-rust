<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Executor helpers

Writing `AgentExecutor` by hand means a `Box::pin` around every body and a
`task_id.clone()` / `context_id.clone()` in every event. This page covers the
three shortcuts the server crate ships for that: `boxed_future`, the
`agent_executor!` macro, and `EventEmitter`.

## `boxed_future` helper

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
## `agent_executor!` macro

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

## `EventEmitter` helper

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
| `auth_required(explanation)` | Park the task at `AUTH_REQUIRED` with the explanation as its status message (spec §7.6.1) |
| `relay_auth_required(&status)` | Pass a downstream agent's `AUTH_REQUIRED` on as this task's own, with its explanation (§7.6.2) |
| `request_approval(request, question)` | Park the task at `INPUT_REQUIRED` asking a person to approve an action |
| `artifact(id, parts, append, last_chunk)` | Emit an artifact update event |
| `is_cancelled()` | Check if the task was cancelled |

What the queue does with each event the emitter writes, and what it refuses,
is on [Writing events](./event-queue.md).
