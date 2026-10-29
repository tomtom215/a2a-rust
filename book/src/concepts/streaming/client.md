<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Consuming a stream

This page covers what a client sees: the events it reads, the limits its SSE
parser applies, and how an error reaches it before and during a stream. The
client API in full (event ordering, chunked artifacts, timeouts) is in
[Streaming Responses](../../client/streaming.md).

## Reading a stream

Use `stream_message` to receive events:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# async fn f(client: A2aClient, params: MessageSendParams) {
let mut stream = client
    .stream_message(params)
    .await
    .expect("connect");

while let Some(event) = stream.next().await {
    match event {
        Ok(StreamResponse::StatusUpdate(ev)) => {
            println!("State: {:?}", ev.status.state);
        }
        Ok(StreamResponse::ArtifactUpdate(ev)) => {
            println!("Artifact: {}", ev.artifact.id);
        }
        Ok(StreamResponse::Task(task)) => {
            println!("Task snapshot: {:?}", task.status.state);
        }
        Ok(StreamResponse::Message(msg)) => {
            println!("Message: {:?}", msg);
        }
        Ok(_) => {
            // Future event types — handle gracefully
        }
        Err(e) => {
            eprintln!("Stream error: {e}");
            break;
        }
    }
}
# }
```

An error that answers `e.is_stream_lagged()` means this reader was cut off for
falling behind; the task is unaffected, so resubscribe rather than give up (see
[Streaming Responses](../../client/streaming.md#how-a-stream-ends)).

## Client protections

The SSE parser includes safety limits:

- **16 MiB event cap** — An oversized event, or a line that never ends, is refused with an error rather than buffered (`with_max_event_size`)
- **30-second stream connect timeout** — Bounds waiting for the stream's response headers (TCP connect itself is `with_connection_timeout`, 10 seconds)
- **First-event timeout** — A stream that is accepted but silent before its first data times out after 5 minutes by default (`with_stream_first_event_timeout`; separate from the 30-second connect timeout), on every transport
- **Idle timeout** — After the first frame, a stream that receives nothing at all (keep-alive comments count) for 5 minutes by default ends with `ClientError::Timeout`; resubscribe to continue
- **Partial line buffering** — Handles TCP frame boundaries correctly (CRLF, LF, and bare-CR line endings per the SSE spec)

## Errors on the wire

A streaming call can fail before its stream starts (unknown task, invalid
params, streaming not advertised) or partway through (an executor failure, the
`streamLagged` signal).

**Before the stream starts**, the error is not SSE:

- over JSON-RPC it is a plain `application/json` JSON-RPC error response,
  HTTP 200 (a refused credential answers HTTP 401/403 instead);
- over HTTP+JSON it is an HTTP error status with a `google.rpc.Status` body.

The official conformance kit (a2aproject/a2a-tck) requires the JSON-RPC shape,
because it reads any `text/event-stream` answer as a successful stream. One
peer loses it: a2a-go v2.5.0's client reads a streaming answer only as SSE, so
over JSON-RPC it sees an empty stream and no error. A Go client that needs
typed errors from a stream that fails to open should use HTTP+JSON or gRPC.

**Partway through**, the server writes the error inside the SSE body as one
`event: error` frame, then closes it:

| Binding | Error frame `data:` |
|---|---|
| JSON-RPC | the JSON-RPC error response, echoing the request id: `{"jsonrpc":"2.0","id":1,"error":{"code":-32001,…}}` |
| HTTP+JSON | a `google.rpc.Status` (§11.6): `{"error":{"code":404,"status":"NOT_FOUND","message":…,"details":[…]}}`; `A2aError::data` rides as a `google.protobuf.Struct` detail, `{"@type":"type.googleapis.com/google.protobuf.Struct","value":{…}}` (data that names its own `@type` goes as it is) |

This client accepts every shape a peer sends: a JSON-RPC refusal as SSE, as a
plain JSON body (this server, and the Python SDK's), or as an
HTTP status whose body is a `google.rpc.Status` (HTTP+JSON) all fail the call
itself with `ClientError::Protocol`; any other non-success status, such as a
`401` or `403`, is `ClientError::UnexpectedStatus`. The one
exception is a peer that opens a live, chunked stream and then sends the error
as its first frame (a2a-go's server): that cannot be told from a stream that
has started, so the same `ClientError::Protocol` arrives as the first
`next()`. Handle errors in both places.
