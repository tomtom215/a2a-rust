<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Slice b: TLS, CAs, proxies and the network (B-03 … B-30, 21 rows)

Baseline `4a8243c` (confirmed with `git rev-parse HEAD`). Everything was read; no cargo, no live probe was run, and no test was run.

## Counts

| Verdict | Rows |
|---|---|
| already_fixed (2) | B-04, B-22 |
| partly_fixed (5) | B-03, B-09, B-15, B-18, B-30 |
| confirmed (10) | B-08, B-12, B-13, B-14, B-19, B-20, B-25, B-26, B-27, B-28 |
| needs_decision (4) | B-10, B-11, B-23, B-29 |

There are 5 new findings: two medium (NEW-b-1, NEW-b-2) and three low.

Severity changes: B-03 high → medium. B-09 medium → low. B-13 medium → low (only `http://` webhooks are pinned). B-15 medium → low (the environment covers most cases now). B-20 low → medium (see below).

## The five rows that matter most

1. **B-20 (medium, confirmed): which roots are trusted depends on the path, and the gap has grown.** `SSL_CERT_FILE` / `SSL_CERT_DIR` now reach every HTTP path and WebSocket, but two paths ignore them:
   - The gRPC client trusts tonic's webpki roots only (`grpc.rs:363-366`, `grpc-tls` = `tls-webpki-roots`).
   - OTLP trusts Mozilla's roots plus `OTEL_EXPORTER_OTLP_CERTIFICATE` only (`exporters.rs:140-150`).

   `ClientBuilder::with_tls_config` does not reach gRPC, which needs a separate tonic type, and that type replaces Mozilla's roots instead of adding to them. Result: in a container whose private CA comes from `SSL_CERT_FILE`, JSON-RPC works and gRPC and OTLP fail.
2. **B-03 (partly fixed): the token provider still cannot take a TLS config.** `CardFetchOptions::with_tls_config` (c4404e8) covers the card resolver and `fetch_jwks`, and its tests would fail on revert. `OAuth2ClientCredentials`, `TokenExchange` and `discover_token_endpoint` have no TLS setter at all: no in-code CA, and no client certificate (RFC 8705). Only `SSL_CERT_FILE` reaches them.
3. **B-10 / B-11 (needs decision): load-balancer idle timeouts.**
   - Quiet WebSocket and gRPC streams carry no traffic by default. The gRPC default is a recorded decision, pinned by a test at `dispatcher.rs:452-457`.
   - Blocking sends now wait 600 s by default, so behind a 60 s load balancer a 504 is the usual failure.
   - HTTP/2 PINGs do not cross an L7 load balancer that terminates HTTP/2. The only end-to-end fix for gRPC is a heartbeat on the stream, which is an ADR question.
4. **B-08 (confirmed, medium): a path prefix becomes a tenant.** A reverse proxy that keeps a path prefix makes HTTP+JSON read it as a tenant, silently. There is no base-path option and no book guidance. The fix is additive (`with_base_path`).
5. **B-14 (confirmed, medium): no certificate hot reload.** The built-in gRPC TLS (tonic's `ServerTlsConfig`) cannot rotate without a restart. HTTP and WebSocket can reload only through a hand-written resolver.

**B-09** is partly fixed (c76299c): trusted-proxy ranges and port stripping work and are tested. But the ranges are opt-in, so the default with hops set still believes any peer. `Forwarded` is not done.

## New findings

- **NEW-b-1 (medium):** the push SSRF check resolves the webhook host locally even when a proxy will carry the request. In a network where only the proxy can resolve external names, every webhook fails. The only bypass, `allow_private_urls`, turns the SSRF check off entirely.
- **NEW-b-2 (medium):** loopback is never exempt from `HTTP(S)_PROXY`. Go's `net/http` exempts it; hyper-util's `Matcher` does not. The SDK allows plaintext only to loopback, so a loopback token endpoint, JWKS fetch or credentialed A2A call would send its secret in cleartext to the proxy. The token provider's own error message names this risk. The client test `proxy_and_cert_env.rs` relies on `https://localhost` being proxied.
- **NEW-b-3 (low):** in the server crate's non-TLS build (its default features), the push and model clients have no proxy and no connect timeout.
- **NEW-b-4 (low):** a pinned `http://` webhook is matched against `NO_PROXY` by its IP, not its name.
- **NEW-b-5 (low):** the server treats `*.localhost` as loopback for plaintext JWKS, but that name can resolve through DNS.

## What each path does now

Read only. "env roots" means Mozilla's roots plus `SSL_CERT_FILE` / `SSL_CERT_DIR`.

| Path | TLS config in code | env roots | `*_PROXY` | mTLS | Notes |
|---|---|---|---|---|---|
| JSON-RPC / HTTP+JSON | `ClientBuilder::with_tls_config` | yes | yes | yes | happy eyeballs; keepalive |
| WebSocket client | `WebSocketTransportConfig::with_tls_config` | yes | yes (CONNECT) | yes | no happy eyeballs (B-12); no pings (B-10) |
| gRPC client | tonic `ClientTlsConfig` (replaces Mozilla) | **no** | yes (CONNECT) | yes | ALPN h2 required (B-19); TCP keepalive only |
| Card fetch / client JWKS | `CardFetchOptions::with_tls_config` | yes | yes | yes | new client per fetch (B-28) |
| Token provider / exchange / client OIDC | **none** | yes | yes | **no** | B-03 |
| Push sender | `HttpPushSender::with_tls_config` | yes | yes¹ | yes | `http://` pinned to one address (B-13); SSRF check needs local DNS (NEW-b-1) |
| Server JWKS / OIDC discovery | `from_*_with_tls_config` | yes | yes¹ | yes | B-04 fixed |
| Model client | `OpenAiChatModel::with_tls_config` | yes | yes¹ | yes | |
| OTLP | `OTEL_EXPORTER_OTLP_CERTIFICATE` / code | **no** | yes | yes | fails hard on a bad PEM |
| SLIMRPC | SLIM's own `slim_config` | SLIM's choice | SLIM's own `ProxyConfig` | yes | outside this crate; SLIM has its own certificate reload |

¹ Only with the server crate's `tls-rustls` feature (NEW-b-3).

## What I did not check

- Nothing was run: no cargo, no tests, and no live probe (no IPv6, load balancer, Squid or Kubernetes).
- Each row's `probe` field gives the exact steps where a running process would settle or prove it.
- **Read, not re-verified:**
  - hyper's zone-ID grammar (B-27).
  - That `*.localhost` goes to DNS in a given libc (NEW-b-5).
  - Load-balancer behaviour, including whether an L7 load balancer forwards HTTP/2 PINGs (B-10, B-11).
- **SLIMRPC:** read only far enough to see that TLS, proxy, keepalive and certificate reload are delegated to SLIM's configuration. SLIM's own code was not audited.
