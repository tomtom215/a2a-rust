# Generated API Documentation

Full rustdoc for the four published crates is built from the source on every
deploy and published alongside this book.

**→ [Browse the API documentation](/api/)**

| Crate | Documentation |
|---|---|
| `a2a-protocol-sdk` | [/api/a2a_protocol_sdk/](/api/a2a_protocol_sdk/index.html) — umbrella re-export and prelude |
| `a2a-protocol-types` | [/api/a2a_protocol_types/](/api/a2a_protocol_types/index.html) — wire types, `serde` only |
| `a2a-protocol-client` | [/api/a2a_protocol_client/](/api/a2a_protocol_client/index.html) — client and transports |
| `a2a-protocol-server` | [/api/a2a_protocol_server/](/api/a2a_protocol_server/index.html) — handler, dispatchers, stores |

Built with `--all-features`, so feature-gated items appear with the flag that
enables them.

## Why this exists

Until now the site carried no API reference at all. The
[API Quick Reference](./api-reference.md) is a hand-curated selection — useful
as a starting point, and deliberately not exhaustive — which left "what is the
exact signature of this method?" answerable only by reading the source or
waiting for a crates.io release to reach docs.rs.

rustdoc is generated from the code, so unlike a hand-written page it cannot
drift. The build runs with `-D warnings`, meaning a broken intra-doc link fails
the deploy rather than shipping a dead link.

## `a2a-protocol-slimrpc`

Not included here. The SLIMRPC binding sits outside the workspace with its own
lockfile — see [its chapter](../bindings/slimrpc.md) — and documenting it would
pull 379 transitive dependencies into the docs build. Build it locally with:

```sh
cd bindings/a2a-protocol-slimrpc && cargo doc --open
```
