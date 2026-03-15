<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# LESSONS — Pitfalls Catalog

Running list of non-obvious problems encountered during development.
Each entry is added at the time the issue is first hit.

---

## Serde Pitfalls

### Untagged enums hide inner errors

`#[serde(untagged)]` on `SendMessageResponse` and `StreamResponse` swallows
the real deserialization error and replaces it with a generic "data did not
match any variant" message. When debugging, temporarily switch to an externally
tagged enum to see the real error.

### `#[serde(default)]` vs `Option<T>`

Using `#[serde(default)]` on a `Vec<T>` field means the field is present but
empty when omitted from JSON. Using `Option<Vec<T>>` means it is absent
(`None`). The A2A spec distinguishes between "omitted" and "empty array" for
fields like `history` and `artifacts`, so `Option<Vec<T>>` is correct.

### `#[non_exhaustive]` on enums breaks downstream matches

Adding `#[non_exhaustive]` to `TaskState`, `ErrorCode`, etc. forces
downstream crates to include a wildcard arm. This is intentional for forward
compatibility but must be documented clearly.

## Hyper 1.x API Pitfalls

### `Body` is not `Clone`

Hyper 1.x `Incoming` body is consumed on read. You cannot read the body twice.
Use `http_body_util::BodyExt::collect()` to buffer it into `Bytes` first,
then work from the buffered copy.

### Response builder panics on invalid header values

`hyper::Response::builder().header(k, v)` silently stores the error and panics
when `.body()` is called. Always use `unwrap_or_else` with a fallback response,
or validate header values with `.parse::<HeaderValue>()` first.

### `size_hint()` upper bound may be `None`

`hyper::body::Body::size_hint().upper()` returns `None` when Content-Length is
absent. Always check for `None` before comparing against the body size limit.

## SSE Streaming Pitfalls

### SSE parser must handle partial lines

SSE events may arrive split across TCP frames. The parser must buffer partial
lines and only process complete `\n`-terminated lines. The `EventBuffer` in
`a2a-client` handles this, but naive `lines()` iterators will break.

### Memory limit on buffered SSE data

A malicious server can send an infinite SSE event (no `\n\n` terminator).
The client parser enforces a 16 MiB cap on buffered event data to prevent OOM.

## Push Notification Pitfalls

### SSRF via webhook URLs

Push notification `url` fields can point to internal services (e.g.
`http://169.254.169.254/`). The `HttpPushSender` resolves the URL and rejects
private/loopback IP addresses before sending. This check must happen after DNS
resolution, not on the URL string alone.

### Header injection via credentials

Push notification `credentials` can contain newlines that inject additional
HTTP headers. The push sender validates that credential values contain no
`\r` or `\n` characters.

## Async / Tokio Pitfalls

### Object-safe async traits need `Pin<Box<dyn Future>>`

Rust does not yet support `async fn` in traits that are used as `dyn Trait`.
The `TaskStore` and `AgentExecutor` traits use explicit
`Pin<Box<dyn Future<Output = ...> + Send + 'a>>` return types. Implementors
use `Box::pin(async move { ... })`.

### Cancellation token cleanup

`CancellationToken`s that are never cancelled accumulate in the token map.
The handler cleans up already-cancelled tokens before inserting new ones, and
enforces a hard cap of 10,000 tokens.

### Amortized eviction vs test expectations

Running O(n) eviction on every `save()` call is expensive. The task store
amortizes eviction to every 64 writes. Tests that depend on eviction must
call `run_eviction()` explicitly or account for the amortization interval.

## Workspace / Cargo Pitfalls

### `cargo-fuzz` needs its own workspace

The `fuzz/` directory contains its own `Cargo.toml` with `[workspace]` to
prevent cargo-fuzz from conflicting with the main workspace. The fuzz crate
references `a2a-types` via a relative path dependency.

### Feature unification across workspace

Enabling a feature in one crate (e.g. `signing` in `a2a-types`) enables it
for all crates in the workspace during `cargo test --workspace`. Use
`--no-default-features` or per-crate test commands when testing feature gates.

## Testing Pitfalls

### HashMap iteration order is non-deterministic

The `InMemoryTaskStore` uses a `HashMap` internally. `list()` must sort
results by task ID before applying pagination; otherwise, page boundaries
shift between runs and tests become flaky.

### Percent-encoded path traversal

Checking `path.contains("..")` is insufficient. Attackers can use `%2E%2E`
to bypass the check. The REST dispatcher percent-decodes the path before
checking for `..` sequences.
