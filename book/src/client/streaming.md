# Streaming Responses

For long-running tasks or when you want real-time progress, use `stream_message` to receive SSE events as the agent works.

## Basic Streaming

```rust,ignore
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
                if let a2a_protocol_types::message::PartContent::Text(text) = &part.content {
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

1. `Task` snapshot (always first — per spec, both `SendStreamingMessage` and `SubscribeToTask` emit this)
2. `StatusUpdate` → `Working`
3. `ArtifactUpdate` (one or more, potentially chunked)
4. `StatusUpdate` → `Completed` (or `Failed`)
5. Optionally, a final `Task` snapshot with accumulated artifacts

> **Note:** The server always emits a `Task` snapshot as the **first event** in
> any streaming response. For `subscribe_to_task()`, this allows reconnecting
> clients to recover the current state. For `send_streaming_message()`, it
> provides the initial task state before execution events begin.

## Chunked Artifacts

Artifacts can be delivered in multiple chunks:

```rust,ignore
Ok(StreamResponse::ArtifactUpdate(ev)) => {
    let is_append = ev.append.unwrap_or(false);
    let is_last = ev.last_chunk.unwrap_or(false);

    if is_append {
        // Append parts to existing artifact. The server also
        // deep-merges metadata from the new event into the existing
        // artifact's metadata (new keys override existing).
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

```rust,ignore
let mut stream = client
    .subscribe_to_task("task-abc")
    .await?;

// Continue processing events...
while let Some(event) = stream.next().await {
    // ...
}
```

## Stream Timeouts

A stream has three bounds, one per phase:

```rust,ignore
use std::time::Duration;

let client = ClientBuilder::new(url)
    .with_stream_connect_timeout(Duration::from_secs(15))        // headers
    .with_stream_first_event_timeout(Duration::from_secs(120))   // first data
    .with_stream_idle_timeout(Some(Duration::from_secs(300)))    // between data
    .build()?;
```

The **connect timeout** (default 30 seconds) bounds establishing the stream: until the response headers arrive (for gRPC, until the call is accepted), and reading the error body when the answer is not a stream.

The **first-event timeout** (default 5 minutes) bounds the wait for the stream's first data once it is established; a keep-alive comment counts. The specification asks a server to open with its `Task` or `Message` at once, and this repository's server does, but a2a-go writes nothing until its agent emits an event, so an agent that makes a slow model call first is silent until the call returns.

> **Migrating from 0.13 or earlier:** the connect timeout used to bound the first event too, so an agent that flushed its headers and thought for longer than 30 seconds was cut off. If you shortened `with_stream_connect_timeout` in order to fail fast on a silent agent, set `with_stream_first_event_timeout` to the same value to keep that behaviour. `GrpcTransport::with_stream_connect_timeout` likewise now bounds opening the call rather than the first event.

After the first frame, the **idle timeout** (`with_stream_idle_timeout`, default 5 minutes) bounds the silence between chunks. Any bytes reset it, including the `: keep-alive` comments this repository's server writes every 30 seconds, so a healthy stream from it runs for as long as the task does. A stream that receives nothing for the whole bound ends with `ClientError::Timeout` ("stream idle timeout: …"); the task on the server is not cancelled, so resubscribe with `subscribe_to_task` to continue. Set `None` to disable it.

The default is five minutes because a healthy peer is never that quiet on the wire: this repository's server heartbeats every 30 seconds; a2a-go v2.5.0 sends no keep-alives unless the server opts in, but its own client gives a whole request 3 minutes; and common proxies close a connection that is silent for about 60 seconds. gRPC and WebSocket streams carry no heartbeat the stream can see, so on those bindings the bound is on the gap between *events* — raise it for agents that think silently for longer.

## Safety Limits

The SSE parser protects against resource exhaustion:

| Limit | Value | Purpose |
|-------|-------|---------|
| Buffer cap | 16 MiB | Prevents OOM from oversized events |
| Connect timeout | 30s (default) | Fails fast on unreachable servers |
| Idle timeout | 5 min (default) | Ends a stream whose server stopped sending, keep-alives included |

## Next Steps

- **[Task Management](./task-management.md)** — Querying tasks after streaming
- **[Error Handling](./error-handling.md)** — Handling stream failures
