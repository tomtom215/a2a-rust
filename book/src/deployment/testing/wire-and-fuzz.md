<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Wire Format and Fuzz Tests

Tests for what crosses the network: that the JSON matches the A2A
specification, and that the parsers survive input nobody would send on
purpose.

## Wire format tests

Verify JSON serialization matches the A2A spec:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
#[test]
fn task_state_wire_format() {
    let status = TaskStatus::new(TaskState::Completed);
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("\"TASK_STATE_COMPLETED\""));
}

#[test]
fn message_role_wire_format() {
    let msg = Message::user_text("m1", "hi");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"ROLE_USER\""));
    assert!(json.contains("\"messageId\""));
}
```

## Fuzz testing

The `fuzz/` directory holds 17 fuzz targets for the parsers of peer input:
JSON deserialization, the JSON-RPC envelope, the SSE parser, JWT and JWKS
parsing, the webhook URL check and more (`fuzz/README.md` lists them all):

```bash
# Requires nightly Rust and cargo-fuzz (one-time install)
cargo install cargo-fuzz --version '^0.12' --locked

# From the repository root, as CI and fuzz/README.md run it
cargo +nightly fuzz run json_deser
```

Fuzz testing helps find edge cases in the parsers that unit tests miss.
