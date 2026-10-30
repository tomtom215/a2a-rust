<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Common patterns

Most executors are variations on a handful of shapes. This page covers the
standard working-artifact-completed sequence, reporting failure, holding
state, and bounding how long a run may take. Pausing for input or
authorization, and cancellation, are on
[Input, authorization and cancellation](./interrupted.md).

## The standard three-event pattern

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
## Error handling

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

Returning `Err` also ends the task `Failed`; the class is then inferred from
its code, which can only say `InvalidRequest` or `Internal`, unless the error
carries one recorded with `a2a_protocol_types::failure::set_error_class`.

What the caller reads depends on the code. An error of any code but
`InternalError` is the executor's message to its caller, and its text is
the task's status message. An `A2aError::internal`'s text is withheld: the
caller reads `internal error (reference 3f9c0a1b2d4e)`, and the text is
logged under that reference with the task id (feature `tracing`). That text
is whatever the failure wrote, often a URL with its API key, a database's
table names or a downstream agent's address, and every later `GetTask`
returns it (audit SEC-07; before 2026-10-30 it was sent as it was). What
`emit.fail` writes is sent as written: it is what you chose to tell the
caller. `RequestHandlerBuilder::expose_internal_errors()` sends internal
errors' text too, for development or an agent whose callers are its
operators. The same rule holds for an internal error a store, an
interceptor or a database returns to any call; the SDK's own messages (an
executor's deadline, a refused configuration) are sent as written.

## Executor with state

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

## Executor timeout

The builder can set a timeout that kills hung executors:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# fn f(my_executor: impl AgentExecutor) -> ServerResult<RequestHandler> {
use std::time::Duration;

RequestHandlerBuilder::new(my_executor)
    .with_agent_card(card())
    .with_executor_timeout(Duration::from_secs(300))  // 5 minutes
    .build()
# }
```
The default is one hour. If the executor doesn't complete within the timeout, the task transitions to `Failed` automatically, with the failure class `BudgetExhausted`: the same call would hit the same bound.
