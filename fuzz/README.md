<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Fuzzing

Fuzzing harnesses for the a2a-rust protocol types, powered by `cargo-fuzz` and `libFuzzer`.

## Overview

Fuzz testing validates that the A2A type system handles arbitrary and malformed input gracefully — no panics, no undefined behavior, no memory safety issues.

## Targets

| Target | What it fuzzes |
|--------|---------------|
| `json_deser` | JSON deserialization of all A2A types (`AgentCard`, `Task`, `Message`, `StreamResponse`, etc.) |
| `jsonrpc_envelope` | The JSON-RPC request envelope and every method's params/response types |
| `sse_parser` | The client SSE parser (`SseParser`), including arbitrary chunk-boundary splits and the bounded-queue OOM guard |
| `proto_convert` | Differential round-trip of the protobuf <-> serde conversion layer (decode, convert, convert back, re-encode) |
| `iso8601` | The ISO-8601 timestamp parser used on stored task timestamps and the `statusTimestampAfter` filter |
| `jwks_parse` | `Jwks::from_json`, parsing a key set fetched from a remote OIDC/JWKS endpoint |
| `trace_context` | The W3C Trace Context parser (`TraceContext::parse` and `with_tracestate`) on a peer-supplied `traceparent`/`tracestate` pair |
| `jwt_token` | Bearer-token validation (`JwtValidator`), HS256 and ES256 keys fixed; asserts nothing unsigned is accepted |
| `rest_route` | The REST request line: tenant prefix (`/{tenant}/`, `/tenants/{tenant}/`) and the `ListTasks` query string |
| `forwarded_for` | The rate limiter's caller key from `x-forwarded-for` behind 0–7 trusted hops |
| `webhook_url` | The push-webhook SSRF check (`validate_webhook_url`): URI syntax, bracketed IPv6, C-style numeric IPv4 hosts |
| `page_token` | The in-memory store's `ListTasks` page token and the `A2A-Extensions` header |

The server's private parsers are reached through `a2a_protocol_server::fuzzing`,
a module compiled only under `cfg(fuzzing)` (set by `cargo fuzz`) and
`cfg(test)`, which forwards to each parser unchanged.

### Every parser of peer input, and what fuzzes it

A parser of peer input is one that reads bytes a remote party chose. The
2026-09-22 adopter audit found three with no target (escape class 8: JWT, REST
query, `x-forwarded-for`); this is the inventory that replaced a list nobody
kept. Add a row, and a target, with every new one.

| Peer | Input | Parser | Target |
|---|---|---|---|
| client → server | JSON-RPC envelope and params | `serde_json` into the protocol types | `jsonrpc_envelope`, `json_deser` |
| client → server | REST path and query | `strip_tenant_prefix`, `parse_list_tasks_query` | `rest_route` |
| client → server | `Authorization: Bearer` JWT | `JwtValidator::validate` | `jwt_token` |
| client → server | `traceparent` / `tracestate` | `TraceContext::parse` | `trace_context` |
| client → server | `A2A-Extensions` | `parse_extensions_header` | `page_token` |
| client → server | `ListTasks` page token | `decode_order_key` (in-memory store) | `page_token` |
| client → server | `statusTimestampAfter` | `parse_iso8601_to_unix_millis` | `iso8601` |
| client → server | push webhook URL | `validate_webhook_url` | `webhook_url` |
| proxy → server | `x-forwarded-for` | `caller_key` | `forwarded_for` |
| client → server (gRPC) | protobuf messages | `prost` and the proto conversions | `proto_convert` |
| identity provider → server | JWKS document | `Jwks::from_json` | `jwks_parse` |
| server → client | SSE stream | `SseParser` | `sse_parser` |
| server → client | JSON bodies and events | `serde_json` into the protocol types | `json_deser` |

Not yet covered, each a small wrapper away: the client's REST error bodies
(`parse_aip193_error`, `decode_stream_error_frame`) and its `Retry-After`
parser (`parse_retry_after`). The SQL stores' page tokens are split by
`store::cursor::decode`, one `split_once` whose halves are bound as query
parameters; a target for it would measure `split_once`.

Every target named in `.github/workflows/fuzz.yml`'s `matrix.target` runs as a
60-second smoke test on every PR/push and a 10-minute sweep nightly; the
nightly sweep also persists each target's corpus between runs via GitHub
Actions cache, so it builds on previously discovered inputs instead of starting
from empty every night. A target registered here but missing from that matrix
is built by nobody and run by nobody, so adding one is two edits, not one:
`fuzz/Cargo.toml` **and** that matrix.

## Running

```bash
# Install cargo-fuzz (requires nightly)
cargo install cargo-fuzz

# Run any target by name
cargo +nightly fuzz run json_deser
cargo +nightly fuzz run jsonrpc_envelope
cargo +nightly fuzz run sse_parser
cargo +nightly fuzz run proto_convert
cargo +nightly fuzz run iso8601
cargo +nightly fuzz run jwks_parse
cargo +nightly fuzz run trace_context
cargo +nightly fuzz run jwt_token
cargo +nightly fuzz run rest_route
cargo +nightly fuzz run forwarded_for
cargo +nightly fuzz run webhook_url
cargo +nightly fuzz run page_token

# Run with a time limit (e.g., 5 minutes)
cargo +nightly fuzz run json_deser -- -max_total_time=300

# Run with a corpus
cargo +nightly fuzz run json_deser corpus/json_deser/
```

## Prerequisites

- Rust nightly toolchain (`rustup install nightly`)
- `cargo-fuzz` (`cargo install cargo-fuzz`)

## Adding New Targets

1. Create a new file in `fuzz_targets/`
2. Add the target to `fuzz/Cargo.toml` and to `matrix.target` in
   `.github/workflows/fuzz.yml` (`scripts/check_fuzz_matrix.py` fails until
   all three agree)
3. Follow the existing pattern: accept `&[u8]`, attempt the parse, ignore
   errors; assert any property the parser promises beyond not panicking
4. Add its row to the inventory above

## License

Apache-2.0
