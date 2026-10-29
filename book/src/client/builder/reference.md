<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Builder reference

Every `ClientBuilder` method, its default, and what it changes. The pages in this chapter show the common ones in use.

| Method | Default | Description |
|--------|---------|-------------|
| `new(url)` | — | Base URL of the agent (required) |
| `from_card(&AgentCard)` | — | Build from an agent card, using the first interface the card lists that this client supports (spec §8.3.2; until 0.15 it preferred JSON-RPC). Only interfaces whose `protocolVersion` has major 1 count (`1.0` and `v1.0` count; an empty version does not), so a v0.3 endpoint listed first is skipped; a card with none is refused with what it offers. If the chosen interface cannot be built (gRPC under sync `build()`, an unknown binding, a bad URL), `build()` moves to the next one. The client also checks the card's optional capabilities before each call that needs one (spec §3.3.4); see `without_capability_checks` |
| `from_card_preferring(&AgentCard, &[String])` | — | Same, with your own binding order. The first preference the card offers wins; matching is case-insensitive |
| `with_protocol_binding(str)` | `"JSONRPC"` | Force transport: `"JSONRPC"`, `"HTTP+JSON"` (or `"REST"`), or `"GRPC"`, in any case. On a builder made from a card, moves the endpoint and tenant to that binding's interface too, and turns off `build()`'s fallback to other interfaces |
| `with_custom_transport(impl Transport)` | None | Use a custom transport (e.g., `GrpcTransport`) |
| `without_capability_checks()` | checks on | A client from a card refuses, before sending, a call whose capability the card does not set to `true`: `stream_message` and `subscribe_to_task` need `capabilities.streaming`, the push-config methods and a send with an inline push config need `pushNotifications`, `get_extended_agent_card` needs `extendedAgentCard`. The refusal is `ClientError::CapabilityNotDeclared`; a conformant agent would refuse the call too. This sends them anyway, for an agent that serves a capability without declaring it. Once `get_extended_agent_card` succeeds, the checks use the extended card's capabilities (spec §3.1.11). A client built with `new(url)` has no card and checks nothing |
| `with_timeout(Duration)` | 30s | Per-request timeout |
| `with_connection_timeout(Duration)` | 10s | TCP connection timeout |
| `with_stream_connect_timeout(Duration)` | 30s | Establishing a stream (headers, or an error body) |
| `with_stream_first_event_timeout(Duration)` | 5 min | Wait for a stream's first data once established |
| `with_max_event_size(usize)` | 16 MiB | Largest single stream event accepted; larger ones are refused and skipped |
| `with_stream_idle_timeout(Option<Duration>)` | 5 min | Silence allowed between chunks after a stream's first data; SSE keep-alives reset it; `None` disables |
| `with_retry_policy(RetryPolicy)` | None | Retry on transient errors with jittered exponential backoff |
| `with_accepted_output_modes(Vec<String>)` | `["text/plain", "application/json"]` | MIME types the client handles |
| `with_history_length(u32)` | None | Messages to include in responses |
| `with_return_immediately(bool)` | false | Don't wait for task completion |
| `with_tenant(str)` | None (auto from `AgentCard`) | Default tenant for multi-tenancy |
| `with_interceptor(impl CallInterceptor)` | Empty chain | Add request/response hook |
| `with_extensions(uris)` | None | Extensions to ask the agent for, sent as `A2A-Extensions` on every request |
| `with_max_response_size(usize)` | 32 MiB | Largest buffered (non-streaming) response body |
| `without_tls()` | TLS on with the `tls-rustls` feature | Disable TLS (plain HTTP only) |
| `with_peer_honouring_idempotency(bool)` | false (true when the card advertises the extension) | Let a keyed send be retried after an ambiguous failure; see [Idempotent Sends](../idempotency.md) |
| `with_grpc_bare_address_scheme(GrpcBareAddressScheme)` | `HttpsExceptLoopback` | How `build_grpc` dials a bare `host:port` |
| `with_grpc_tls_config(ClientTlsConfig)` | None (bundled Mozilla roots) | TLS settings for `build_grpc` (`grpc-tls` feature) |
| `skip_required_extension_check()` | checks on | Build even when the card marks an extension `required` that `with_extensions` does not ask for; use it when an interceptor sends `A2A-Extensions` |
| `require_https_credentials()` | warn once per client | Refuse, before sending, a call that carries a credential (`Authorization`, `Proxy-Authorization`, `Cookie`, an API-key header or query parameter) to an `http://` or `ws://` host other than loopback (spec §13.4); `ClientError::InsecureTransport` |
| `require_https_webhooks()` | warn | Refuse a push config whose webhook URL is `http://` to a host other than loopback (spec §13.2); `ClientError::InsecureWebhook` |
| `with_credential_preflight(CredentialPreflight)` | `Warn` | What a client built from a card does with a call whose credentials satisfy none of the card's `securityRequirements` (spec §3.1.11): `Off`, `Warn` or `Refuse` |
| `with_trace_propagation(bool)` | true | Whether a call's `traceparent` names the call's own span (has an effect with the `otel` feature) |
| `with_content_keys(Jwks)`, `require_signed_content()` | off | (feature `signing`) Verify the provenance signature of received artifacts and agent messages; the second also refuses unsigned content |
| `chosen_interface()` | — | The card interface the builder will connect to; `None` after `new(url)` |
| `build()` | — | Construct a JSON-RPC or REST client |
| `build_grpc()` (async) | — | Construct a gRPC client (`grpc` feature) |
