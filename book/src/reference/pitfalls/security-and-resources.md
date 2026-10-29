<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Security and resource limits

Part of [Pitfalls & Lessons Learned](../pitfalls.md).

## Push Notification Pitfalls

### SSRF via webhook URLs

Push notification `url` fields can point to internal services (e.g., `http://169.254.169.254/` — the cloud metadata endpoint).

**Solution:** The `HttpPushSender` resolves the URL and rejects private/loopback IP addresses *after DNS resolution*, not on the URL string alone. The `validate_webhook_url_with_dns()` function performs DNS resolution before IP validation, preventing DNS rebinding attacks where a hostname resolves to a public IP during validation but a private IP during the actual HTTP request.

### Header injection via credentials

Push notification `credentials` can contain newlines that inject additional HTTP headers.

**Solution:** The push sender validates that credential values contain no `\r` or `\n` characters before using them in HTTP headers.

## Push Config Store Pitfalls

### Per-resource limits are not enough — add global limits

A per-task push config limit (e.g., 100 configs per task) does not prevent an attacker from creating millions of tasks with 100 configs each. Always pair per-resource limits with a global cap (e.g., `max_total_configs = 100_000`). Enforce both limits on every create. The order is not what matters: in this SDK's handler the per-task check runs first and the global one second, and the global one is approximate under concurrent creates for different tasks.

### Webhook URL scheme validation

Checking for private IPs and hostnames in webhook URLs is insufficient. URLs like `ftp://evil.com/hook` or `file:///etc/passwd` bypass IP-based SSRF checks entirely. Always validate that the URL scheme is `http` or `https` before performing any further validation.

## Performance Pitfalls

### Deserialization allocation overhead

Standard `serde_json::to_vec` and `serde_json::from_str` allocate fresh buffers on every call. For hot paths (SSE frame building, transport payload serialization), this overhead compounds. The `serde_helpers` module in `a2a-protocol-types` provides optimized alternatives:

- **`SerBuffer`** — Thread-local reusable serialization buffer. `SerBuffer::serialize(&value)` serializes through it and returns the bytes as a new `Vec<u8>` (a copy of the buffer), which saves the buffer's growth reallocations on small payloads; `SerBuffer::serialize_into(writer, &value)` writes straight into your own writer with no intermediate buffer.
- **`deser_from_str` / `deser_from_slice`** — Thin wrappers over `serde_json::from_str` / `from_slice`. They help only for types with borrowed (`&str` / `Cow`) fields, which the A2A types are not.

Use these in performance-sensitive paths where serialization/deserialization is called repeatedly (e.g., per-event SSE frame building, transport payload encoding).

### `Vec<u8>` vs `Bytes` in retry loops

Cloning a `Vec<u8>` inside a retry loop allocates a full heap copy each time. Use `bytes::Bytes` (reference-counted) so that `.clone()` is just an atomic reference count increment. This matters for push notification delivery where large payloads may be retried 3+ times.

### Serialize once before retry loops

Deep-cloning a `serde_json::Value` tree on every retry attempt is expensive. Serialize the params to bytes once before the retry loop, then deserialize from bytes for each attempt. Deserialization from bytes is cheaper than a recursive deep-clone of the `Value` tree.

### Agent card fetch responses need a body size limit

Fetching an agent card from an untrusted URL without a body size limit allows a malicious endpoint to send an arbitrarily large response, causing OOM. Always enforce a body size limit (e.g., 2 MiB) on agent card fetch responses.

## Database Pitfalls

### Always use parameterized queries for user-controlled values

Using `format!` to interpolate caller-supplied values into SQL allows SQL injection; bind them. sqlx 0.9 enforces the rule on the type level: a statement built at run time must be wrapped in `sqlx::AssertSqlSafe`, which is the author vouching that only static text and placeholder positions went in. The one value the SDK's own stores interpolate is the page-size `LIMIT`, an integer clamped by `max_page_size`; every caller-supplied value is bound.
