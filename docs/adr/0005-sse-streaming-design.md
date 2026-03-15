# ADR 0005: SSE Streaming Design

**Date:** 2026-03-15
**Status:** Accepted
**Author:** Tom F.

---

## Context

The A2A `message/stream` and `tasks/resubscribe` methods return Server-Sent Events (SSE) streams. SSE is a plain-text HTTP/1.1 streaming protocol defined in the HTML Living Standard (WHATWG). The A2A spec wraps each event's `data:` field in a `JSONRPCResponse<StreamResponse>`.

Rust does not have a battle-tested, zero-dep SSE server or client library that is hyper 1.x native. Third-party crates (`eventsource-client`, `sse-codec`) either use older hyper versions, carry additional deps, or lack proper backpressure.

## Decision

### In-Tree SSE Implementation

Both client-side SSE parsing and server-side SSE emission are implemented in-tree with zero additional deps. The implementations are ~220 and ~200 lines respectively, within the 500-line limit.

### Client-Side: `sse_parser.rs`

A byte-stream state machine that reads from a `hyper::body::Incoming` byte stream:

```
State machine:
  IDLE → accumulating line bytes
  NEWLINE → process line (data/event/id/retry/comment)
  DOUBLE_NEWLINE → dispatch accumulated frame
```

Handles:
- `data: {...json...}` — accumulated across multiple `data:` lines (concatenated with `\n`).
- `:` lines — keep-alive comments, silently ignored.
- `event:` field — optional event type (A2A uses the default empty event type).
- `retry:` field — reconnection hint, stored but not automatically acted on.
- Fragmented TCP segments — the parser accumulates bytes without assuming line boundaries align with chunks.

Returns `SseFrame { data: String, event_type: Option<String>, id: Option<String>, retry: Option<u64> }`.

`EventStream` wraps the frame stream and deserializes each `data` field as `JsonRpcResponse<StreamResponse>`.

### Server-Side: `sse.rs`

An `SseResponseBuilder` constructs a `hyper::Response` with:
- Status: 200
- `Content-Type: text/event-stream`
- `Cache-Control: no-cache`
- `Connection: keep-alive`
- Chunked transfer encoding (via `http-body-util::StreamBody`)

Each `StreamResponse` event is encoded as:
```
data: {"jsonrpc":"2.0","id":"...","result":{...}}\n\n
```

Keep-alive frames are encoded as:
```
: keep-alive\n\n
```

The server SSE writer holds a `tokio::sync::mpsc::Receiver<SseFrame>` and polls both the receiver and a `tokio::time::interval` for keep-alive ticks. The `EventQueueManager` owns the corresponding `Sender<SseFrame>`.

### Backpressure

The `mpsc` channel between `AgentExecutor` and the SSE writer is bounded (default capacity: 64 events). If the channel is full, `EventQueueWriter::write` returns `A2aError::Internal` with message `"event queue full"`. The executor should handle this by slowing down or yielding. This prevents unbounded memory growth in slow-consumer scenarios.

### Stream Termination

The A2A spec uses `TaskStatusUpdateEvent.final: true` to signal end-of-stream. The server SSE writer inspects each event: when it sees `final: true`, it drains remaining buffered events, writes them, then closes the response body. The client `EventStream` treats the `final: true` event as the last item and returns `None` from `next()` after yielding it.

### Resubscription

`tasks/resubscribe` allows a client to reconnect to an in-progress SSE stream after a disconnect. The `EventQueueManager::get(task_id)` returns the existing queue if the task is still `working` or `input-required`. The new SSE response writer subscribes to the same queue. Events already consumed by the previous subscriber are not replayed (clients must handle potential gaps using `TaskVersion`).

### Error During Streaming

If `AgentExecutor::execute` returns an `A2aError`, the server:
1. Converts the error to a `TaskStatusUpdateEvent { state: Failed, ... }` with `final: true`.
2. Writes this terminal event to the queue.
3. Closes the queue.

The client receives the terminal event, marks the stream as ended, and returns the error from the next `EventStream::next()` call as `ClientResult::Err(ClientError::Protocol(A2aError { code: InternalError, ... }))`.

## Consequences

### Positive

- Zero additional deps for full SSE support.
- SSE parser handles all real-world edge cases (fragmented TCP, keep-alive, multi-line data).
- Backpressure prevents OOM in slow-consumer scenarios.
- Stream termination is deterministic and spec-compliant.

### Negative

- In-tree SSE code must be maintained when the SSE spec changes (rare; SSE is stable).
- Resubscription does not replay missed events; clients must tolerate gaps.
- Bounded queue (64 events) may need tuning for high-throughput streaming agents; `RequestHandlerBuilder::with_event_queue_capacity(n)` allows override.

## Alternatives Considered

### `eventsource-client` Crate

Rejected: uses hyper 0.14 (incompatible with hyper 1.x); maintenance status uncertain; would add a transitive dep on the old hyper version.

### `async-sse` Crate

Rejected: last released 2020; depends on `http-types` from the `tide` ecosystem; not hyper 1.x compatible.

### Unbounded Channel

Rejected: an agent that emits events faster than the client can consume them would exhaust server memory. Bounded channel with explicit error on overflow is the correct production behavior.
