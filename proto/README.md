<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Protocol Buffer Definitions

gRPC service definitions for the A2A protocol v1.0.

## Canonical binding — `a2a_v1/a2a.proto`

`a2a_v1/a2a.proto` is the **canonical A2A v1.0 protobuf schema**
(`package lf.a2a.v1`, `service A2AService`): fully-typed messages for all
11 A2A methods, exactly as published by the specification. This is the
binding the client speaks and the server serves — it is wire-compatible
with the official A2A SDKs (Python, JS, Go, Java).

Rules for this file:

- It is **never edited** for codegen convenience. It is a byte-for-byte copy
  of `specification/a2a.proto` from the A2A release named in
  [`UPSTREAM`](UPSTREAM) (tag, commit and SHA-256; currently **v1.0.1**).
  The `proto_schema_sync` tests in `a2a-protocol-types` and
  `scripts/check_proto_copies.sh` (CI, offline) keep this file, the per-crate
  copies under `crates/*/proto/a2a_v1/`, `tck/proto/` and
  `docs/implementation/a2a.proto` identical, the script also holds them to
  the pinned hash, and
  `scripts/check_a2a_proto_upstream.sh` (`official-tck.yml`, online) fails
  when a newer A2A release changes the file.
- Until 2026-10-10 every copy here was A2A **v1.0.0**'s text. v1.0.1
  (2026-05-28) changed only comments, but they are the normative `tenant`
  rules, and nothing compared the copies with upstream, so the drift went
  unnoticed for five months. That is why `UPSTREAM` and the upstream check
  exist.
- Wire compatibility is proven against the official A2A Python SDK
  (`a2a-sdk`): golden binary fixtures under `tck/fixtures/grpc/` are
  validated in both directions (prost decodes the official SDK's bytes;
  the official SDK parses prost-encoded bytes) by the `grpc-wire-compat`
  CI job.
- `a2a_v1/google/api/` holds minimal vendored stubs of the googleapis
  annotation protos the schema imports (Apache-2.0, from
  github.com/googleapis/googleapis).

See ADR 0009 (`docs/adr/0009-protobuf-native-grpc.md`) for the design
record.

## Legacy tunnel — `a2a.proto` (deprecated)

`a2a.proto` (`package a2a.v1`, `service A2aService`) is the pre-0.7
JSON-in-`bytes` tunnel: every RPC takes and returns a `JsonPayload`
wrapper around UTF-8 JSON. It is **not** the A2A gRPC binding and cannot
interoperate with the official SDKs.

It was retained only so 0.6 clients could be rolling-upgraded, served
*alongside* the canonical service via the off-by-default `grpc-legacy-json`
feature. The client support was removed in 0.7 and the tunnel service, its
feature and its `a2a.proto` schema were removed in 0.8. Nothing in this
workspace compiles or serves it any more.

## Usage

These protos are compiled by the `grpc`-feature build scripts of
`a2a-protocol-types` (messages), `a2a-protocol-server` (server stubs), and
`a2a-protocol-client` (client stubs) using a vendored `protoc` — no manual
compilation or system protobuf install is needed.

## License

Apache-2.0
