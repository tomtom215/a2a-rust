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

## Running

```bash
# Install cargo-fuzz (requires nightly)
cargo install cargo-fuzz

# Run the JSON deserialization fuzzer
cargo +nightly fuzz run json_deser

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
