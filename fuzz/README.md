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

All six run as a 60-second smoke test on every PR/push and a 10-minute sweep
nightly (`.github/workflows/fuzz.yml`); the nightly sweep also persists each
target's corpus between runs via GitHub Actions cache, so it builds on
previously discovered inputs instead of starting from empty every night.

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
2. Add the target to `fuzz/Cargo.toml`
3. Follow the existing pattern: accept `&[u8]`, attempt deserialization, ignore errors

## License

Apache-2.0
