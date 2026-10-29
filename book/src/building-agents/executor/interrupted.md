<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Input, authorization and cancellation

A task does not have to run straight through. This page covers the three ways
it stops part-way: asking the client for more input (`INPUT_REQUIRED`),
asking for a credential (`AUTH_REQUIRED`), and being cancelled.

## Requesting more input

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

The client continues the task by sending another message with the same
`taskId` (and `contextId`). A message that names only the `contextId` starts a
new task in that context and leaves this one waiting for input.

## Asking for authorization in the task

When the agent needs a credential it does not have, spec §7.6.1 has it park
the task at `AUTH_REQUIRED` with a message saying what to authorize, and
take the credential out of band. `auth_required` does the first two. The
executor can then return, and continue on the client's next message, or
wait for the credential on the same run: the stream stays open across
`AUTH_REQUIRED` until the task ends, and after a blocking send, which answers
at `AUTH_REQUIRED`, what the executor emits next is still stored, so a client
polling `GetTask` sees the task finish. The two ways differ in what a
message to the task does while it waits: spec §7.6.1 asks an agent to keep
the stream open across `AUTH_REQUIRED` and also to accept messages to the
task meanwhile, and one run cannot do both. A message sent while the
executor is still running waits up to
`HandlerLimits::executor_drain_timeout` (5 s by default) for the run to
return, then is refused with `UnsupportedOperation` ("already being
processed"). An executor that should accept the client's next message,
the credential included, returns after `auth_required`. On the client side,
`A2aClient::follow_task(id, poll_every)` follows such a task, by subscription
when the agent can stream and by polling when it cannot (spec §7.6.2).

```rust
# use a2a_protocol_sdk::prelude::*;
use a2a_protocol_server::executor_helpers::EventEmitter;

struct Calendar;

agent_executor!(Calendar, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    if ctx.http_header("x-calendar-grant").is_none() {
        return emit
            .auth_required("Connect your calendar at https://cal.example/connect, then resend")
            .await;
    }
    emit.status(TaskState::Completed).await
});
```

## Supporting cancellation

Every executor supports cancellation without writing anything. By the time
`cancel` runs, the handler has cancelled the running task's
`cancellation_token` (the one in `execute`'s `ctx`), which a running `execute`
should observe (`EventEmitter::is_cancelled`), and the default `cancel` emits
the terminal `Canceled` status. The `ctx` passed to `cancel` is a separate
context for the cancel call; its token is not that one.

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

The last line is optional. After a hook that returns `Ok` without writing a
terminal state, the handler writes `Canceled` itself, so the task's open
streams end on it either way.
