<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# A2A Technology Compatibility Kit (TCK)

Standalone conformance test runner that validates any A2A v1.0 server implementation.

## Overview

- Tests wire format interoperability across all A2A implementations
- Language-agnostic: tests servers written in Rust, Python, Go, JS, Java, or any language
- Validates all 11 A2A methods via JSON-RPC and REST bindings
- Checks ProtoJSON naming conventions, discriminated union serialization, security scheme formatting

## Usage

```bash
# Test a JSON-RPC server
cargo run -p a2a-tck -- --url http://localhost:3000 --binding jsonrpc

# Test a REST server
cargo run -p a2a-tck -- --url http://localhost:3000 --binding rest
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All tests passed |
| 1 | One or more tests failed |
| 2 | Configuration error |

## What It Tests

| Area | Details |
|------|---------|
| Methods | All 11 A2A v1.0 method request/response formats |
| Naming | ProtoJSON naming conventions (`SCREAMING_SNAKE_CASE` with type prefix) |
| Security | `SecurityRequirement` / `StringList` wrapper format |
| Unions | Discriminated union serialization (`SendMessageResponse`, `StreamResponse`) |
| Discovery | Agent card discovery (`/.well-known/agent.json`) |
| Errors | Error responses for invalid requests |

## Integration Test Kit (ITK)

The TCK is used by the ITK (`itk/`) to cross-check agents written in Python, Go, JS, and Java.
See `itk/README.md` for Docker Compose orchestration.

## License

Apache-2.0
