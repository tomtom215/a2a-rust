# Generated API Documentation

Full rustdoc for the four published crates is built from `main` on every
deploy and published under [`/dev/api/`](/dev/api/). A release's API
documentation is the one docs.rs builds from the published crate: `/api/` and
`/vX.Y/api/` redirect to it, and old `/api/<crate>/...` links are sent there
by the 404 page.

**→ [Browse the API documentation](/api/)**

| Crate | Documentation |
|---|---|
| `a2a-protocol-sdk` | [/api/a2a_protocol_sdk/](/api/a2a_protocol_sdk/index.html) — umbrella re-export and prelude |
| `a2a-protocol-types` | [/api/a2a_protocol_types/](/api/a2a_protocol_types/index.html) — wire types, `serde` only |
| `a2a-protocol-client` | [/api/a2a_protocol_client/](/api/a2a_protocol_client/index.html) — client and transports |
| `a2a-protocol-server` | [/api/a2a_protocol_server/](/api/a2a_protocol_server/index.html) — handler, dispatchers, stores |

The `/dev/api/` build uses `--all-features`, so feature-gated items appear
with the flag that enables them; docs.rs builds each crate with all features
too.

## Why this exists

The [API Quick Reference](./api-reference.md) is a hand-curated selection —
useful as a starting point, and deliberately not exhaustive. For the exact
signature of an item, read the generated documentation: rustdoc is generated
from the code, so unlike a hand-written page it cannot drift.

The `/dev/api/` build runs with `-D warnings`, meaning a broken intra-doc link
fails the deploy rather than shipping a dead link.

## `a2a-protocol-slimrpc`

Not included here. The SLIMRPC binding sits outside the workspace with its own
lockfile — see [its chapter](../bindings/slimrpc.md) — and documenting it would
pull in a large dependency tree: 387 transitive dependencies on 2026-10-25
(normal edges, at the binding's locked `agntcy-slim-rpc` 3.4.6, as
`cargo tree --edges normal` counts them). Build it locally
with:

```sh
cd bindings/a2a-protocol-slimrpc && cargo doc --open
```
