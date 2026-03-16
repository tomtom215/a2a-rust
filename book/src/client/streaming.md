# Streaming Responses

For long-running tasks or when you want real-time progress, use `stream_message` to receive SSE events as the agent works.

## Basic Streaming

```rust
let mut stream = client
    .stream_message(params)
    .await
    .expect("connect to stream");

while let Some(event) = stream.next().await {
    match event {
        Ok(StreamResponse::StatusUpdate(ev)) => {
            println!("Status: {:?}", ev.status.state);
        }
        Ok(StreamResponse::ArtifactUpdate(ev)) => {
            for part in &ev.artifact.parts {
                if let a2a_protocol_types::message::PartContent::Text { text } = &part.content {
                    print!("{text}");
                }
            }
            if ev.last_chunk == Some(true) {
                println!(); // Newline after final chunk
            }
        }
        Ok(StreamResponse::Task(task)) => {
            println!("Final: {:?}", task.status.state);
        }
        Ok(StreamResponse::Message(msg)) => {
            println!("Message: {:?}", msg);
        }
        Ok(_) => {
            // Future event types — handle gracefully
        }
        Err(e) => {
            eprintln!("Error: {e}");
            break;
        }
    }
}
```

## Event Ordering

A typical stream delivers events in this order:

1. `StatusUpdate` → `Working`
2. `ArtifactUpdate` (one or more, potentially chunked)
3. `StatusUpdate` → `Completed` (or `Failed`)
4. Optionally, a final `Task` snapshot

## Chunked Artifacts

Artifacts can be delivered in multiple chunks:

```rust
Ok(StreamResponse::ArtifactUpdate(ev)) => {
    let is_append = ev.append.unwrap_or(false);
    let is_last = ev.last_chunk.unwrap_or(false);

    if is_append {
        // Append to existing artifact
        buffer.push_str(&extract_text(&ev.artifact));
    } else {
        // New artifact or first chunk
        buffer = extract_text(&ev.artifact);
    }

    if is_last {
        println!("Complete artifact: {buffer}");
    }
}
```

## Re-subscribing

If a stream disconnects, re-subscribe to get the latest state:

```rust
use a2a_protocol_sdk::types::params::TaskIdParams;

let mut stream = client
    .resubscribe(TaskIdParams {
        tenant: None,
        id: "task-abc".into(),
    })
    .await?;

// Continue processing events...
while let Some(event) = stream.next().await {
    // ...
}
```

## Stream Timeouts

The client has separate timeouts for stream connections:

```rust
use std::time::Duration;

let client = ClientBuilder::new(url)
    .with_stream_connect_timeout(Duration::from_secs(15))
    .build()?;
```

The connect timeout applies to establishing the SSE connection. Once connected, the stream stays open until the server closes it or an error occurs.

## Safety Limits

The SSE parser protects against resource exhaustion:

| Limit | Value | Purpose |
|-------|-------|---------|
| Buffer cap | 16 MiB | Prevents OOM from oversized events |
| Connect timeout | 30s (default) | Fails fast on unreachable servers |

## Next Steps

- **[Task Management](./task-management.md)** — Querying tasks after streaming
- **[Error Handling](./error-handling.md)** — Handling stream failures
