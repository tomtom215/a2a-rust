<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# gRPC golden wire-compatibility fixtures

Binary protobuf fixtures proving the `lf.a2a.v1` binding is
wire-compatible with the **official A2A Python SDK** (`a2a-sdk`), an
independent implementation (generated protobuf-python classes on Google's
protobuf runtime — no code shared with prost).

## Layout

- `corpus/*.json` — one message per file:
  - `message`: the `lf.a2a.v1` message type name;
  - `proto_json`: the ProtoJSON document the official SDK parses to build
    the message;
  - `domain_json` (optional): the expected serde JSON of the converted
    domain value, when it legitimately differs from ProtoJSON (the
    OpenAPI-style `SecurityScheme` tagging and the derived `AgentCard.url`
    are the only such cases). Defaults to `proto_json`.
- `bin/*.bin` — the corpus serialized by the official SDK
  (`SerializeToString(deterministic=True)`). **Checked in; never edit by
  hand.**

## What gets proven (CI job `grpc-wire-compat`, plus locally)

1. `grpc_wire_compat.py check-golden` — the checked-in bytes still match
   what the currently installed official SDK produces (schema-drift guard).
2. `cargo test -p a2a-protocol-types --features proto --test
   proto_golden_fixtures` — prost decodes the official bytes; the decoded
   messages convert to exactly the expected domain JSON; re-encoding the
   same values yields bytes that decode equal to the official message; the
   re-encoded bytes are emitted to `target/grpc-wire-compat/`.
3. `grpc_wire_compat.py verify-rust` — the official SDK parses the
   Rust-encoded bytes and must observe the corpus-expected messages.

## Regenerating

Only needed when the corpus changes or the canonical schema revision moves:

```sh
pip install a2a-sdk
python3 tck/scripts/grpc_wire_compat.py generate
```

Commit the corpus and `bin/` changes together. The fixture test refuses to
pass with fewer than 30 fixtures, so a wiped directory cannot silently
green-light.
