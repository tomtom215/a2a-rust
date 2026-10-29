# Error Handling

a2a-rust uses a layered error model: protocol-level errors (`A2aError`), client errors (`ClientError`), and server errors (`ServerError`).

## A2aError (Protocol Level)

Protocol errors defined by the A2A spec:

```rust
use a2a_protocol_sdk::types::error::{A2aError, ErrorCode};

// Common error codes
let _common = [
    ErrorCode::TaskNotFound,         // Task doesn't exist
    ErrorCode::TaskNotCancelable,    // Task already in a terminal state
    ErrorCode::InvalidParams,        // Bad request parameters
    ErrorCode::MethodNotFound,       // Unknown method
    ErrorCode::InternalError,        // Server-side failure
    ErrorCode::UnsupportedOperation, // Operation invalid for current state
                                     // (e.g. SendMessage to terminal task,
                                     //  SubscribeToTask on completed task)
];
let e = A2aError::task_not_found("task-abc");
assert_eq!(e.code, ErrorCode::TaskNotFound);
```

### Handling protocol errors

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::types::error::ErrorCode;
# async fn f(client: A2aClient) {
# let params = TaskQueryParams { tenant: None, id: "task-abc".into(), history_length: None };
match client.get_task(params).await {
    Ok(task) => println!("Got task: {}", task.id),
    Err(ClientError::Protocol(e)) if e.code == ErrorCode::TaskNotFound => {
        eprintln!("no such task: {}", e.message);
    }
    Err(e) => eprintln!("Error: {e}"),
}
# }
```

## Client errors

The client wraps transport and protocol errors:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn f(client: A2aClient, params: MessageSendParams) -> Result<(), ClientError> {
match client.send_message(params).await {
    Ok(response) => { /* handle response */ }
    Err(e) => {
        // Which variant it is says what went wrong: a transport failure,
        // an A2A error from the agent (`Protocol`), or a response that
        // would not read (`Serialization`, `InvalidAgentResponse`).
        eprintln!("Client error: {e}");
    }
}
# Ok(())
# }
```

`ClientError` is `#[non_exhaustive]`: a `match` on it needs a wildcard arm.

| Variant | Meaning | `is_retryable()` |
|---|---|---|
| `Http`, `HttpClient` | A transport failure from hyper or its client (connection, redirect) | yes |
| `Timeout` | A request, or a stream's connect, first-event or idle bound, expired | yes |
| `TooManyPendingRequests { limit }` | The connection's cap on requests awaiting a response (WebSocket) refused this one up front | yes |
| `IncompleteStream { last_event_id, detail }` | A stream ended before its final event; resume with `subscribe_to_task_from` ([below](#a-stream-that-ends-early)) | yes |
| `UnexpectedStatus { status, body, retry_after }` | An HTTP status the binding did not map to an A2A error (gRPC `Unauthenticated`, `PermissionDenied` and `ResourceExhausted` arrive as 401, 403 and 429) | 429, 502, 503, 504 |
| `Protocol(A2aError)` | An A2A error from the agent. Also what `build()` returns for `ExtensionSupportRequired`, and `InvalidParams` for a message the client refuses before sending | only when it names a retry delay |
| `Serialization` | A request could not be encoded, or the agent's JSON does not parse | no |
| `InvalidAgentResponse(String)` | The agent sent an object `a2a.proto` does not allow: a REQUIRED field missing or empty (spec §5.7), or a card that fails `AgentCard::validate`. The message names the call and the field | no |
| `Transport`, `InvalidEndpoint`, `ProtocolBindingMismatch` | A transport configuration or connection error, an unusable URL (or a card with no interface this client speaks), or a client and agent on different bindings | no |
| `CapabilityNotDeclared { method, capability }` | Not sent: the card the client was built from does not declare the capability the call needs (spec §3.3.4) | no |
| `InvalidAuthScheme`, `InsecureTransport`, `InsecureWebhook` | Not sent: an API key the client cannot place (a name that is not an HTTP token, or a query-string key over gRPC or WebSocket) or credentials that fail `CredentialPreflight::Refuse`; a credential bound for a plain `http://` or `ws://` host (`require_https_credentials`); a plain `http://` webhook URL (`require_https_webhooks`) | no |
| `ContentNotVerified` | Received content failed its provenance check (feature `signing`) | no |
| `AuthRequired { task_id }` | The agent requires authentication for a task. No bundled transport returns it: a 401 or 403 arrives as `UnexpectedStatus` | no |

### Timeouts

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn f(url: &str, params: MessageSendParams) -> Result<(), ClientError> {
# use std::time::Duration;
let client = ClientBuilder::new(url)
    .with_timeout(Duration::from_secs(5))            // the whole request
    .with_connection_timeout(Duration::from_secs(2)) // the TCP connect
    .build()?;

match client.send_message(params).await {
    Ok(_response) => { /* success */ }
    Err(ClientError::Timeout(detail)) => eprintln!("timed out: {detail}"),
    Err(e) => eprintln!("failed: {e}"),
}
# Ok(())
# }
```

A request that outlasts `with_timeout` fails with `ClientError::Timeout`, as do
a stream's connect, first-event and idle bounds and a gRPC `DeadlineExceeded`.
A connect that outlasts `with_connection_timeout` fails as a connection error
(`HttpClient`). Both are retryable.

### A stream that ends early

A stream finishes cleanly on a `Message`, or on a task or status update in a
terminal (`completed`, `failed`, `canceled`, `rejected`) or interrupted
(`input-required`, `auth-required`) state. If the body ends — or the
connection closes — anywhere else, `next()` yields
`ClientError::IncompleteStream` instead of `None`, and a frame the server did
not finish writing is named in its message rather than dropped silently. The
task is very likely still running; the error carries the last SSE `id:` to
resume from:

```rust,no_run
# use a2a_protocol_client::{A2aClient, ClientError, EventStream};
# async fn step(client: &A2aClient, task_id: &str, mut stream: EventStream)
#     -> Result<EventStream, ClientError> {
match stream.next().await {
    Some(Err(ClientError::IncompleteStream { last_event_id: Some(id), .. })) => {
        stream = client.subscribe_to_task_from(task_id, id).await?;
    }
    Some(Err(ClientError::IncompleteStream { last_event_id: None, .. })) => {
        stream = client.subscribe_to_task(task_id).await?; // snapshot + live
    }
    _other => { /* ... */ }
}
# Ok(stream)
# }
```

### Errors on an HTTP+JSON stream

A streaming request over HTTP+JSON reports errors as `ClientError::Protocol`
with the exact A2A code, as unary calls do: an AIP-193 error body on a non-2xx
answer (§11.6) decodes to, for example, `TaskNotFound` for `subscribe_to_task`
on a missing task. So does an error the server sends *inside* an open stream,
in the shapes seen in practice: a2a-go's AIP-193 object as a data frame
(`{"error":{"code":404,"status":"NOT_FOUND",...}}`), this repository's
`event: error` frame carrying the same object with the error's `data` as a
`google.protobuf.Struct` detail — decoded back into `data`, so
`is_stream_lagged()` still works — and, from releases up to 0.13.0, an
`event: error` frame carrying a bare `A2aError`. The stream ends after it.

### Automatic retries

Use `RetryPolicy` to automatically retry transient errors:

```rust
# use a2a_protocol_sdk::client::{ClientBuilder, ClientError};
# fn main() -> Result<(), ClientError> {
# let url = "http://agent.example.com";
use a2a_protocol_client::RetryPolicy;

let client = ClientBuilder::new(url)
    .with_retry_policy(RetryPolicy::default())
    .build()?;
# Ok(())
# }
```

You can check if an error is retryable programmatically:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn f(client: A2aClient, params: MessageSendParams) -> Result<(), ClientError> {
match client.send_message(params).await {
    Err(e) if e.is_retryable() => println!("Transient error: {e}"),
    Err(e) => println!("Permanent error: {e}"),
    Ok(resp) => { /* ... */ }
}
# Ok(())
# }
```

Retryable errors include: `Http`, `HttpClient`, `Timeout`, `IncompleteStream`, `TooManyPendingRequests`, `UnexpectedStatus` with codes 429, 502, 503, or 504, and a `Protocol` error whose details carry a `google.rpc.RetryInfo` delay, which is how a rate limit is refused over JSON-RPC and WebSocket. `retry_after()` returns that delay, or the delta-seconds `Retry-After` of an HTTP error response such as a 429 or 503 (an HTTP-date is ignored; more than an hour is read as an hour). The retry policy waits that long instead of its own backoff, but never longer than its `max_backoff` (30 seconds by default). A 429, a 503 and a `RetryInfo` delay are retried even for `SendMessage`, because the request was refused before it was processed. Over gRPC, `DeadlineExceeded` maps to `Timeout`, and `Unavailable` or a response truncated before its `grpc-status` maps to `HttpClient` (all retryable). `ResourceExhausted` maps to `UnexpectedStatus { status: 429 }` (retryable), and `Unauthenticated`/`PermissionDenied` map to `UnexpectedStatus` 401/403 (not retryable). A peer's `Cancelled` is a non-retryable `Protocol` error.

Each computed backoff is randomized to between 0.5x and 1.0x of its value, to prevent thundering-herd storms when multiple clients experience the same failure simultaneously. A server-directed delay is used as given (capped at `max_backoff`).

## Server errors

When you build an agent, `ServerError` is what `RequestHandlerBuilder::build()`
and the handler's and stores' operations return (`ServerResult<T>` is
`Result<T, ServerError>`). The handler turns it into the A2A error the caller
receives with `ServerError::to_a2a_error`: `TaskNotFound` on the server arrives
as `ClientError::Protocol` with `ErrorCode::TaskNotFound` on the client, and a
variant with no A2A counterpart arrives as `InternalError`. An executor returns
`A2aError` (`A2aResult`) instead, as below.

## Best practices

### Don't panic

a2a-rust reports failures as `Result` rather than panicking; [the introduction](../introduction.md#why-rust) says how far a CI gate holds it to that. Follow the same pattern in your executors:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# fn good() -> A2aResult<()> {
// Good: return an error
return Err(A2aError::internal("processing failed"));
# }
# fn bad() {

// Bad: panic
panic!("processing failed");
# }
```

### Executor error handling

In your `AgentExecutor`, catch errors and report them as status updates:

```rust
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::types::failure::FailureClass;

struct Summarizer;
# async fn risky_operation(_: &Message) -> Result<String, std::io::Error> { Ok("ok".into()) }

agent_executor!(Summarizer, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;

    match risky_operation(&ctx.message).await {
        Ok(result) => {
            emit.artifact("summary", vec![Part::text(result)], None, Some(true)).await?;
            emit.status(TaskState::Completed).await?;
        }
        Err(e) => {
            // Report failure through the protocol: a `Failed` status whose
            // message says why, and a class a caller can branch on.
            emit.fail(FailureClass::Internal, e.to_string()).await?;
        }
    }

    Ok(())
});
```

### Delegating to another agent with `?`

An executor that calls another agent can use `?` on client calls:
`ClientError` converts into `A2aError`, and the server turns the returned
error into a `Failed` task whose failure class says whether a retry is worth
it.

```rust,no_run
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::Message;
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::responses::SendMessageResponse;

// The body of an `AgentExecutor::execute` that delegates.
async fn delegate(downstream_url: &str) -> A2aResult<Option<String>> {
    let client = ClientBuilder::new(downstream_url).build()?;
    let params = MessageSendParams::new(Message::user_text("m1", "summarise this"));
    let reply = client.send_message(params).await?; // ClientError -> A2aError
    Ok(match reply {
        SendMessageResponse::Task(task) => task.text().map(str::to_owned),
        _ => None,
    })
}
```

| Client error | Task's failure class |
|---|---|
| `Protocol(e)` from the downstream agent | passed through unchanged (code, message, data), classified by its code |
| `Protocol(e)` naming a retry delay (a downstream rate limit) | passed through with its delay, `Transient` |
| timeouts, connection failures, a stream cut off before its final event (`IncompleteStream`), HTTP `429`/`502`/`503`/`504`, `TooManyPendingRequests` | `Transient` (retry with backoff) |
| anything else | `Internal` |

`Transient` is given exactly when `ClientError::is_retryable()` is true, so the
client's retry policy and the caller's agree. For every client error other than
`Protocol`, the failed task's status text is `downstream A2A call failed: `
followed by the client error and its causes; a `Protocol` error keeps the
downstream agent's own message.

### Stream error recovery

For streaming, handle errors per-event:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn f(mut stream: EventStream) {
while let Some(event) = stream.next().await {
    match event {
        Ok(ev) => { /* process event */ }
        Err(e) if e.is_stream_lagged() => {
            // This reader fell behind and was cut off; the task is unaffected.
            eprintln!("dropped {:?} events; resubscribe to continue", e.dropped_event_count());
            break;
        }
        Err(e) => {
            eprintln!("Stream error: {e}");
            // Decide: retry via resubscribe, or give up
            break;
        }
    }
}
# }
```

An error that answers `is_stream_lagged()` means this reader fell behind and
was cut off; resubscribe to continue, as
[Streaming Responses](./streaming.md#how-a-stream-ends) shows.

## Next Steps

- **[Testing Your Agent](../deployment/testing.md)** — Testing error scenarios
- **[Pitfalls & Lessons Learned](../reference/pitfalls.md)** — Common mistakes
