<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Browsers and Webhooks

Two ways an agent talks to the web outside A2A clients: browsers calling it
across origins, and the agent calling out to the webhooks its callers
register. Both are off or restricted until you configure them. What the push
sender guards against in full is on [Security](../security.md#outbound-requests-are-the-dangerous-ones);
this page is the configuration.

## CORS

Configure CORS for browser-based clients. It is off until configured: without
`with_cors`, no response carries CORS headers (an `OPTIONS` preflight
included), so a browser refuses the cross-origin call. Name the origin you
expect rather than using `CorsConfig::permissive()`:

```rust,no_run
use std::sync::Arc;
use a2a_protocol_sdk::server::{CorsConfig, JsonRpcDispatcher, RequestHandler};

fn dispatcher(handler: Arc<RequestHandler>) -> JsonRpcDispatcher {
    JsonRpcDispatcher::new(handler).with_cors(CorsConfig::new("https://app.example.com"))
}
```

`RestDispatcher` and `CombinedDispatcher` have the same `with_cors`. The
WebSocket listener has its own origin check,
`WebSocketDispatcher::with_allowed_origins`.

## Push notification security

The built-in `HttpPushSender` includes:

- **SSRF protection** — Rejects private/loopback IPs at config creation and delivery time (defense-in-depth), including IPv4-in-IPv6 smuggling, and pins the validated IP against DNS rebinding
- **Header injection prevention** — Validates credentials for `\r`/`\n` characters
- **Timeouts** — Each push delivery HTTP request is capped at 10 seconds, and a whole delivery, retries included, at 30 (`HandlerLimits::push_delivery_timeout`), inside the 10-30 seconds spec §4.3.3 recommends
- **HTTPS only, if you ask** — `HttpPushSender::new().https_only()`, handed to `with_push_sender`, refuses an `http://` webhook when the config is created and again at delivery. `with_allowed_hosts([...])` limits delivery to the hosts you name

> **Transport note:** with the `tls-rustls` feature (enabled by default when
> using `a2a-protocol-sdk`) the bundled `HttpPushSender` delivers to both
> `http://` and `https://` webhooks. Without the feature it is plaintext-only
> and rejects an `https://` webhook with a clear error; supply a TLS-capable
> `PushSender` implementation via `with_push_sender` in that configuration —
> `PushSender` is a public, pluggable trait and the SSRF validation is public
> in `a2a_protocol_server::url_guard`.

## Path and size limits

The REST dispatcher's path checks and the request, query-string and event
size caps apply without configuration; they are listed under
[Input handling](../security.md#input-handling).
