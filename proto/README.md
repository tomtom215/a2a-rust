<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Protocol Buffer Definitions

gRPC service definitions for the A2A protocol v1.0.

## Canonical binding — `a2a_v1/a2a.proto`

`a2a_v1/a2a.proto` is the **canonical A2A v1.0 protobuf schema**
(`package lf.a2a.v1`, `service A2AService`): fully-typed messages for all
11 A2A methods, exactly as published by the specification. This is the
binding the client speaks and the server serves. Its wire compatibility
with the official SDKs is checked on every pull request against Python
(the golden fixtures below) and Go (the `go-sdk-interop` job in `ci.yml`
runs the a2a-go client against this server, gRPC included, and this
client against an a2a-go server). The upstream ITK nightly
(`itk-nightly.yml`, not a merge gate) also pairs this SDK with the JS and
Java SDKs over gRPC.

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

## Legacy JSON tunnel (removed in 0.8)

The legacy `a2a.proto` (`package a2a.v1`, `service A2aService`), no longer
in the repository, was the pre-0.7 JSON-in-`bytes` tunnel: every RPC took and
returned a `JsonPayload` wrapper around UTF-8 JSON. It was **not** the A2A
gRPC binding and could not interoperate with the official SDKs.

It was retained only so 0.6 clients could be rolling-upgraded, served
*alongside* the canonical service via the off-by-default `grpc-legacy-json`
feature. The client support was removed in 0.7 and the tunnel service, its
feature and its `a2a.proto` schema were removed in 0.8. Nothing in this
workspace compiles or serves it any more.

## Usage

Nothing compiles this directory: it is the repository-level reference
copy. A published crate can only ship files inside its own directory, so
each build script compiles its own copy (`<crate>/proto/a2a_v1/a2a.proto`,
resolved from `CARGO_MANIFEST_DIR`), and `scripts/check_proto_copies.sh`
holds every copy, this one included, byte-identical:

| Build script | Feature | Generates |
|---|---|---|
| `crates/a2a-protocol-types/build.rs` | `proto` | messages |
| `crates/a2a-protocol-server/build.rs` | `grpc` (enables `a2a-protocol-types/proto`) | server stubs |
| `crates/a2a-protocol-client/build.rs` | `grpc` (enables `a2a-protocol-types/proto`) | client stubs |
| `tck/build.rs` | always | the TCK's own messages and client stubs |

Each uses a vendored `protoc` (`protoc-bin-vendored`) unless `PROTOC` is
already set — no manual compilation or system protobuf install is
needed.

## License

Apache-2.0
