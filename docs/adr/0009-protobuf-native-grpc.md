# ADR 0009: Protobuf-Native gRPC Binding

**Date:** 2026-07-22
**Status:** Accepted
**Author:** Tom F.

---

## Context

Through 0.6, the gRPC transport was a JSON tunnel: a homegrown service
(`a2a.v1.A2aService`) whose eleven methods all carried a single
`JsonPayload { bytes data }` message containing the canonical A2A JSON.
This reused the serde types across all transports with zero duplication and
gave gRPC framing (HTTP/2 multiplexing, native server streaming), but it
was **not the A2A gRPC binding**. The canonical binding is the
`lf.a2a.v1.A2AService` service defined by the A2A specification's protobuf
schema — fully-typed messages, wire-compatible across the official Go,
Python, and Java SDKs.

The consequence was binary: an official-SDK client calling our gRPC
endpoint failed at the first call with `UNIMPLEMENTED` (different
fully-qualified service name, method signatures, and encoding). Our
"gRPC support" interoperated only with itself. The JSON layer never had
this problem — A2A v1.0 defines the JSON-RPC/REST wire format as the
ProtoJSON rendering of the same protobuf schema, which the serde types
already produce (verified by the cross-language TCK).

## Decision

### Canonical schema, byte-identical

`proto/a2a_v1/a2a.proto` is the canonical A2A v1.0 protobuf file, kept
**byte-identical** to the specification copy in
`docs/implementation/a2a.proto`. It is never edited for codegen
convenience. The `google/api/*.proto` files it imports are vendored as
*minimal stubs* (extension declarations only) — custom options carry no
wire-format significance; they exist solely so `protoc` can resolve the
imports. A unit test asserts the copies in each crate stay identical.

### Message types generated once, services per crate

- `a2a-protocol-types` (feature `proto`) runs `prost-build` over the
  canonical schema and exposes the generated **message types** as
  `a2a_protocol_types::proto`, plus a hand-written conversion layer
  (`proto::convert`) mapping them to the serde domain types.
- The server and client crates generate only tonic **service glue**, with
  `extern_path` pointing message references back at
  `a2a_protocol_types::proto`. One set of message types, one conversion
  layer, no drift between client and server.

### Conversion layer contract

`TryFrom` in both directions (both can reject values), following ProtoJSON
semantics so protobuf and JSON round-trips agree:

- proto3 implicit presence: empty string / empty repeated / `false` bool ⇄
  absent domain `Option`s. Message-typed fields keep explicit presence
  (`Some({})` ≠ `None` for metadata).
- `google.protobuf.Struct` ⇄ JSON objects; integral `f64`s inside the
  exact range map back to JSON integers; integers beyond 2^53 are rejected
  rather than silently rounded.
- `google.protobuf.Timestamp` ⇄ RFC 3339 (UTC-normalized on output).
- `bytes` ⇄ base64 (standard-with-padding encode; standard/URL-safe,
  padded or not, on decode — matching ProtoJSON parsers).
- Coverage: unit round-trips per message plus proptest properties that
  push arbitrary domain values through **encoded protobuf bytes** and back.

### Transports

- The server dispatcher serves the canonical `lf.a2a.v1.A2AService`,
  converting typed requests to the same domain params the JSON-RPC and
  REST bindings feed into `RequestHandler`.
- The client `GrpcTransport` speaks the canonical service, converting the
  client core's JSON params to typed messages per method. Streaming events
  arrive as typed `StreamResponse` messages and are re-encoded into the
  existing SSE envelope for the shared `EventStream` parser.
- Because the two service names differ, the deprecated tunnel is served
  *alongside* the canonical service behind the server-only
  `grpc-legacy-json` feature (off by default) so 0.6 gRPC clients survive
  rolling upgrades. The tunnel **client** was removed. The tunnel service
  will be removed in 0.8.

## Consequences

- gRPC interop with the official Go/Python/Java SDKs becomes possible at
  the wire level; the e2e suite proves the binding against this
  workspace's own client and a hand-rolled 0.6-style tunnel client
  sharing one listener.
- Wire compatibility is claimed against the canonical schema file, and the
  schema fidelity is what the byte-identical rule protects. Cross-SDK
  binary fixtures (bytes produced by the official Python/Go SDKs) are the
  remaining strengthening step, tracked for the TCK.
- The types crate gains optional deps (`prost`, `prost-types`, `time`,
  `base64`) behind the `proto` feature; it remains pure data — no I/O.
- Conversion adds one allocation-bearing hop per request compared to the
  tunnel's direct serde path. gRPC payloads are small; the cost is noise
  next to the interop gain (and the tunnel's double JSON parse is gone).
- `history_length`/page sizes are `u32` domain-side but `int32` on the
  wire; negatives are rejected at conversion, values above `i32::MAX`
  cannot be sent.
