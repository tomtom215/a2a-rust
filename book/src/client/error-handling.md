# Error Handling

a2a-rust uses a layered error model: protocol-level errors (`A2aError`), client errors (`ClientError`), and server errors (`ServerError`).

## A2aError (Protocol Level)

Protocol errors defined by the A2A spec:

```rust
use a2a_protocol_sdk::types::error::{A2aError, ErrorCode};

// Common error codes
ErrorCode::TaskNotFound         // Task doesn't exist
ErrorCode::TaskNotCancelable    // Agent doesn't support cancellation
ErrorCode::InvalidParams        // Bad request parameters
ErrorCode::MethodNotFound       // Unknown method
ErrorCode::InternalError        // Server-side failure
ErrorCode::UnsupportedOperation // Not implemented
```

### Handling Protocol Errors

```rust
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

```rust
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

```rust
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

### Connection Errors

```rust
// Connection timeout
let client = ClientBuilder::new(url)
    .with_connection_timeout(Duration::from_secs(2))
    .build()?;
```

### Automatic Retries

Use `RetryPolicy` to automatically retry transient errors:

```rust
use a2a_protocol_client::RetryPolicy;

let client = ClientBuilder::new(url)
    .with_retry_policy(RetryPolicy::default())
    .build()?;
```

You can check if an error is retryable programmatically:

```rust
match client.send_message(params).await {
    Err(e) if e.is_retryable() => println!("Transient error: {e}"),
    Err(e) => println!("Permanent error: {e}"),
    Ok(resp) => { /* ... */ }
}
```

Retryable errors include: `Http`, `HttpClient`, `Timeout`, and `UnexpectedStatus` with codes 429, 502, 503, or 504. gRPC `DeadlineExceeded` and `Cancelled` errors also map to `Timeout` (retryable), and `Unavailable` maps to `HttpClient` (retryable).

Retry backoff uses full jitter (0.5–1.0× randomization) to prevent thundering-herd storms when multiple clients experience the same failure simultaneously.

## Server Errors

When building an agent, the `ServerError` type covers handler-level failures:

```rust
use a2a_protocol_sdk::server::ServerError;

// Server errors are returned by RequestHandlerBuilder::build()
// and by store/executor operations
```

## Best Practices

### Don't Panic

a2a-rust never panics in library code. All fallible operations return `Result`. Follow the same pattern in your executors:

```rust
// Good: return an error
return Err(A2aError::internal("processing failed".into()));

// Bad: panic
panic!("processing failed");
```

### Executor Error Handling

In your `AgentExecutor`, catch errors and report them as status updates:

```rust
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

### Stream Error Recovery

For streaming, handle errors per-event:

```rust
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
