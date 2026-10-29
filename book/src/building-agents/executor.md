# The AgentExecutor Trait

The `AgentExecutor` trait is the heart of every A2A agent. It defines what happens when a message arrives. Everything else — HTTP handling, task management, streaming — is handled by the framework.

## Which page do I need

| You want to… | Page |
|---|---|
| Skip the `Box::pin` boilerplate, or emit events without cloning ids | [Executor helpers](./executor/helpers.md) |
| Read the message, the task, the caller's identity or tenant | [RequestContext](./executor/request-context.md) |
| Write status and artifact events, and know what the queue refuses | [Writing events](./executor/event-queue.md) |
| Copy a working shape: the three-event run, failures, state, a timeout | [Common patterns](./executor/patterns.md) |
| Ask the client for input or a credential, or release resources on cancel | [Input, authorization and cancellation](./executor/interrupted.md) |
| Answer with a model that calls tools, without writing an executor | [The Tool Loop](./tool-loop.md) |

What happens around the executor — the handler that spawns it, the store that
keeps its events, the stream that carries them — is on
[Request Handler & Builder](./handler.md).

## The trait

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::{A2aResult, EventQueueWriter, RequestContext};
pub trait AgentExecutor: Send + Sync + 'static {
    /// Called when a message arrives (SendMessage or SendStreamingMessage).
    /// Runs in a spawned task: write the task's status and artifacts to
    /// `queue`, end with a terminal status (or `INPUT_REQUIRED` /
    /// `AUTH_REQUIRED`) and return `Ok(())`. `Err` fails the task. Returning
    /// `Ok(())` without a terminal or interrupted state invents none: the
    /// task stays in the last state written.
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

    /// Called by `RequestHandler::shutdown()` (bounded to 10 s) or
    /// `shutdown_with_timeout()`, which your shutdown code calls after the
    /// server has drained. `serve`, `serve_with_addr` and
    /// `Server::serve_with_shutdown` do not call it.
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

## Next steps

- **[Request Handler & Builder](./handler.md)** — Configuring the handler around your executor
- **[The Tool Loop](./tool-loop.md)** — An executor that runs a model and its tools for you
- **[Push Notifications](./push-notifications.md)** — Delivering results asynchronously
