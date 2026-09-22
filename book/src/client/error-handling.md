# Error Handling

a2a-rust uses a layered error model: protocol-level errors (`A2aError`), client errors (`ClientError`), and server errors (`ServerError`).

## A2aError (Protocol Level)

Protocol errors defined by the A2A spec:

```rust,ignore
use a2a_protocol_sdk::types::error::{A2aError, ErrorCode};

// Common error codes
ErrorCode::TaskNotFound         // Task doesn't exist
ErrorCode::TaskNotCancelable    // Agent doesn't support cancellation
ErrorCode::InvalidParams        // Bad request parameters
ErrorCode::MethodNotFound       // Unknown method
ErrorCode::InternalError        // Server-side failure
ErrorCode::UnsupportedOperation // Operation invalid for current state
                                // (e.g. SendMessage to terminal task,
                                //  SubscribeToTask on completed task)
```

### Handling Protocol Errors

```rust,ignore
match client.get_task(params).await {
    Ok(task) => println!("Got task: {}", task.id),
    Err(e) => {
        // Check the error type
        eprintln!("Error: {e}");
    }
}
```

## Client Errors

The client wraps transport and protocol errors:

```rust,ignore
match client.send_message(params).await {
    Ok(response) => { /* handle response */ }
    Err(e) => {
        // Transport errors (network, timeout, etc.)
        // Protocol errors (task not found, invalid params, etc.)
        // Parse errors (malformed response)
        eprintln!("Client error: {e}");
    }
}
```

### Timeout Errors

```rust,ignore
// Per-request timeout
let client = ClientBuilder::new(url)
    .with_timeout(Duration::from_secs(5))
    .build()?;

// This will error if the agent takes longer than 5 seconds
match client.send_message(params).await {
    Ok(response) => { /* success */ }
    Err(e) => {
        // Could be a timeout error
        eprintln!("Failed (possibly timeout): {e}");
    }
}
```

### A Stream That Ends Early

A stream finishes cleanly on a `Message`, or on a task or status update in a
terminal (`completed`, `failed`, `canceled`, `rejected`) or interrupted
(`input-required`, `auth-required`) state. If the body ends — or the
connection closes — anywhere else, `next()` yields
`ClientError::IncompleteStream` instead of `None`, and a frame the server did
not finish writing is named in its message rather than dropped silently. The
task is very likely still running; the error carries the last SSE `id:` to
resume from:

```rust,ignore
match stream.next().await {
    Some(Err(ClientError::IncompleteStream { last_event_id: Some(id), .. })) => {
        stream = client.subscribe_to_task_from(task_id, id).await?;
    }
    Some(Err(ClientError::IncompleteStream { last_event_id: None, .. })) => {
        stream = client.subscribe_to_task(task_id).await?; // snapshot + live
    }
    other => { /* ... */ }
}
```

### Errors on an HTTP+JSON Stream

A streaming request over HTTP+JSON reports errors as `ClientError::Protocol`
with the exact A2A code, as unary calls do: an AIP-193 error body on a non-2xx
answer (§11.6) decodes to, for example, `TaskNotFound` for `subscribe_to_task`
on a missing task. So does an error the server sends *inside* an open stream,
in either shape seen in practice — a2a-go's AIP-193 object as a data frame
(`{"error":{"code":404,"status":"NOT_FOUND",...}}`), or this repository's
`event: error` frame carrying an `A2aError`. The stream ends after it.

### Connection Errors

```rust,ignore
// Connection timeout
let client = ClientBuilder::new(url)
    .with_connection_timeout(Duration::from_secs(2))
    .build()?;
```

### Automatic Retries

Use `RetryPolicy` to automatically retry transient errors:

```rust,ignore
use a2a_protocol_client::RetryPolicy;

let client = ClientBuilder::new(url)
    .with_retry_policy(RetryPolicy::default())
    .build()?;
```

You can check if an error is retryable programmatically:

```rust,ignore
match client.send_message(params).await {
    Err(e) if e.is_retryable() => println!("Transient error: {e}"),
    Err(e) => println!("Permanent error: {e}"),
    Ok(resp) => { /* ... */ }
}
```

Retryable errors include: `Http`, `HttpClient`, `Timeout`, `IncompleteStream`, and `UnexpectedStatus` with codes 429, 502, 503, or 504. gRPC `DeadlineExceeded` and `Cancelled` errors also map to `Timeout` (retryable), and `Unavailable` maps to `HttpClient` (retryable).

Retry backoff uses full jitter (0.5–1.0× randomization) to prevent thundering-herd storms when multiple clients experience the same failure simultaneously.

## Server Errors

When building an agent, the `ServerError` type covers handler-level failures:

```rust,no_run
use a2a_protocol_sdk::server::ServerError;

// Server errors are returned by RequestHandlerBuilder::build()
// and by store/executor operations
```

## Best Practices

### Don't Panic

a2a-rust never panics on caller input or I/O failure — every fallible operation returns `Result`. (The only `expect` calls in the libraries assert internal invariants, such as propagating lock poisoning, that callers cannot trigger.) Follow the same pattern in your executors:

```rust,ignore
// Good: return an error
return Err(A2aError::internal("processing failed"));

// Bad: panic
panic!("processing failed");
```

### Executor Error Handling

In your `AgentExecutor`, catch errors and report them as status updates:

```rust,ignore
Box::pin(async move {
    queue.write(/* Working */).await?;

    match risky_operation().await {
        Ok(result) => {
            queue.write(/* ArtifactUpdate */).await?;
            queue.write(/* Completed */).await?;
        }
        Err(e) => {
            // Report failure through the protocol
            queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                status: TaskStatus {
                    state: TaskState::Failed,
                    message: Some(Message {
                        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
                        role: MessageRole::Agent,
                        parts: vec![Part::text(&e.to_string())],
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

### Delegating to Another Agent with `?`

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
| timeouts, connection failures, HTTP `429`/`502`/`503`/`504`, `TooManyPendingRequests` | `Transient` (retry with backoff) |
| anything else | `Internal` |

`Transient` is given exactly when `ClientError::is_retryable()` is true, so the
client's retry policy and the caller's agree. The failed task's status text is
`downstream A2A call failed: ` followed by the client error and its causes.

### Stream Error Recovery

For streaming, handle errors per-event:

```rust,ignore
while let Some(event) = stream.next().await {
    match event {
        Ok(ev) => { /* process event */ }
        Err(e) => {
            eprintln!("Stream error: {e}");
            // Decide: retry via resubscribe, or give up
            break;
        }
    }
}
```

## Next Steps

- **[Testing Your Agent](../deployment/testing.md)** — Testing error scenarios
- **[Pitfalls & Lessons Learned](../reference/pitfalls.md)** — Common mistakes
