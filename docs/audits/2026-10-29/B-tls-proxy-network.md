# Audit B: TLS trust, proxies and network deployment (a2a-rust 0.15.0)

Scope: every outbound HTTP path, server-side TLS, proxies, reverse proxies and load balancers, IPv6, service discovery, DNS.
Method: read only. Every "verified by reading" claim was checked against the source quoted. No build or test was run.
KNOWN means one of two things. Either `docs/handoff.md` *Open engineering* lists the issue, or the earlier adopter audit (`docs/adopter-audit-2026-09-22.md`) reported it and has not marked it fixed. That earlier audit is not on the handoff list. When a finding repeats it, the finding says which source.

Summary: OTLP export is the only outbound path that reads the proxy variables, and it is configured differently from every other path. No path reads the operating system's trust store, `SSL_CERT_FILE` or `SSL_CERT_DIR`.

The A2A client's default bindings, JSON-RPC and HTTP+JSON, give no way to add a CA, use the system store or present a client certificate. Neither do the agent-card resolver, the client-side JWKS fetch or the OAuth token provider. On any network with private PKI, a TLS-inspecting proxy or egress that only works through a proxy, these paths fail. The error they report is `client error (Connect)`, which does not say why.

---

## Matrix: outbound path by trust and proxy settings

"webpki" means the bundled Mozilla roots (`webpki_roots::TLS_SERVER_ROOTS`) only. No path reads the operating system's store, `SSL_CERT_FILE` or `SSL_CERT_DIR`.

| # | Outbound path (file:line) | Root store | Private CA | Self-signed | mTLS client cert | `*_PROXY` env | `NO_PROXY` | Notes |
|---|---|---|---|---|---|---|---|---|
| 1 | A2A client, JSON-RPC (`client/src/transport/jsonrpc.rs:139-143`) | webpki | **No** (hard-coded `default_tls_config()`) | No | **No** | **No** | n/a | ALPN h2/h1 (`enable_all_versions`), TLS 1.2+, ring |
| 2 | A2A client, HTTP+JSON (`client/src/transport/rest/mod.rs:165-169`) | webpki | **No** | No | **No** | **No** | n/a | same as 1 |
| 3 | A2A client, gRPC (`client/src/transport/grpc.rs:347-357`) | webpki via tonic `with_enabled_roots()` (plus the OS store if another crate turns on tonic's `tls-native-roots`) | Yes: `with_grpc_tls_config(ClientTlsConfig)`. **Replaces** the Mozilla roots rather than adding to them | Yes (pin as CA) | Yes (`identity`) | **No** (tonic reads none) | n/a | requires ALPN `h2` (tonic `service/tls.rs:148-150`) |
| 4 | A2A client, WebSocket (`client/src/transport/websocket.rs:636-646`) | webpki | Yes: `with_tls_config(tls_config_with_extra_roots(..))`, adds to Mozilla | Yes | Yes (custom `ClientConfig`) | **No** (`tokio-tungstenite connect.rs:91` raw `TcpStream::connect`) | n/a | no happy eyeballs, no TCP keepalive |
| 5 | Agent-card resolver (`client/src/discovery.rs:576-577`) | webpki | **No** (no TLS field on `CardFetchOptions`, `discovery.rs:108-118`) | No | No | **No** | n/a | new client per fetch (KNOWN C17), no redirects |
| 6 | Client JWKS fetch `fetch_jwks` (`client/src/jwks.rs:72-73`) | webpki | **No** | No | No | **No** | n/a | refuses `http://` beyond loopback (good) |
| 7 | Token provider: client credentials, OIDC discovery, RFC 8693 exchange (`client/src/token_provider.rs:696-710`, `token_provider/exchange.rs:264`) | webpki | **No** | No | **No** (no `tls_client_auth`, RFC 8705) | **No** | n/a | |
| 8 | Push-notification sender (`server/src/push/sender.rs:39-50`, `:321`) | webpki | Yes: `HttpPushSender::with_tls_config` | Yes | Yes | **No**. `http://` is additionally pinned to the resolved IP (`sender.rs:610-617`) | n/a | SSRF guard blocks private ranges unless `allow_private_urls` |
| 9a | Server JWKS (`server/src/auth/jwt.rs:975-1000`) | webpki | Yes: `from_jwks_url_with_tls_config` (`jwt.rs:762-783`) | Yes | Yes | **No** | n/a | accepts `http://` to any host |
| 9b | Server OIDC discovery `from_oidc_issuer` (`jwt.rs:795-798`, `:1052-1062`) | webpki | **No** variant | No | No | **No** | n/a | |
| 10 | Tool-loop `OpenAiChatModel` (`server/src/tool_loop/openai.rs:114`, `:128`) | webpki | Yes: `with_tls_config` | Yes | Yes | **No** | n/a | reuses the push client builder |
| 11 | OTLP/HTTP and OTLP/gRPC export (`server/src/otel/telemetry/exporters.rs:139-160`, `proxy.rs:87-100`) | webpki plus `OTEL_EXPORTER_OTLP_CERTIFICATE` | Yes (env or code; a bad PEM is a hard error) | Yes (CA:FALSE leaf, documented) | Yes (`_CLIENT_CERTIFICATE`/`_CLIENT_KEY`) | **Yes**: `HTTP(S)_PROXY`, `ALL_PROXY`, both cases, upper case wins; `http://` proxies only; `CONNECT` for TLS and gRPC; login in the URL | **Yes**: host, `.domain`, CIDR, `*`, IPv6 bare or bracketed. Not `host:port`, not `*.domain` | the only path with proxy support |
| 12 | MCP bridge (`examples/mcp-bridge/src/bridge.rs:125-131`) | inherits 5 and 1/2 | No | No | No | No | n/a | |
| 13 | `a2a-cli` (`tools/a2a-cli/src/connect.rs:136-182`, `cli.rs:44-85`) | inherits 1-6 | No flag (`--cacert`, `--cert`, `--proxy`, `--insecure` absent) | No | No | No | n/a | |
| 14 | SLIMRPC binding | SLIM's own (`slim_config`) | not audited | | | | | `slim-node` serves `TlsServerConfig::insecure()` without `--tls-cert` (`bindings/.../bin/slim_node.rs:118-123`, with a warning) |

Server listeners:

| Listener | Terminates TLS | mTLS | Hot reload | ALPN | Notes |
|---|---|---|---|---|---|
| `Server` / `serve()` (`server/src/serve/graceful/tls.rs:38-56`) | Yes: `with_tls(rustls::ServerConfig)`, `with_tls_pem` | Yes (verifier in the config; `PeerCertificate` extension) | Not built in. Possible only through a caller-written `ResolvesServerCert` | h2, http/1.1 | `with_tls_pem` = `with_no_client_auth` |
| `WebSocketDispatcher` (`dispatch/websocket.rs:215-233`) | Yes | Yes | same as above | http/1.1 | own port only (no upgrade on `Server` or axum) |
| `GrpcDispatcher` (`dispatch/grpc/dispatcher.rs:122-125`, `:328`) | Yes: tonic `ServerTlsConfig` | Yes (`client_ca_root`) | **No** (tonic config is static) | h2 | `into_service()`: caller's server |
| axum `A2aRouter` (`dispatch/axum_adapter.rs`) | **No** (caller's server) | only if the caller inserts `PeerCertificate` | caller's | caller's | `deploy-agent` serves without `ConnectInfo` |

---

## Findings

### B-01: critical. The default client bindings (JSON-RPC, HTTP+JSON) cannot trust a private CA or the system store, or present a client certificate

- **Where:** `crates/a2a-protocol-client/src/transport/jsonrpc.rs:139-143`; `transport/rest/mod.rs:165-169`; `builder/transport_factory.rs:269-297`; `tls.rs:60-101`.
- **Evidence:**
  ```rust
  // jsonrpc.rs:139
  #[cfg(feature = "tls-rustls")]
  let client = crate::tls::build_https_client_with_connect_timeout(
      crate::tls::default_tls_config(),
      connection_timeout,
  );
  ```
  ```rust
  // tls.rs:97
  fn root_cert_store() -> rustls::RootCertStore {
      let mut store = rustls::RootCertStore::empty();
      store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
  ```
  The public constructors are `new`, `with_timeout`, `with_timeouts`, `with_all_timeouts`, `with_max_response_size` and `endpoint` (`jsonrpc.rs:78-169`; REST `rest/mod.rs:98-195`). None of them takes a `ClientConfig` or `HttpsClient`. `tls::tls_config_with_extra_roots` and `build_https_client_with_config` are public. A grep shows their only callers are tests (`tests/tls_integration_tests.rs`) and the WebSocket transport. `ClientBuilder` only has `with_grpc_tls_config` (`builder/mod.rs:459`). The only remaining route is `with_custom_transport`, which means reimplementing the `Transport` trait, JSON-RPC framing and SSE included.
- **Docs that are wrong:**
  - `book/src/deployment/containers.md:46-50` says: "A private CA reaches the agent through the SDK's CA options." No such option exists for these bindings.
  - `TlsConfig::Rustls` says "Enable TLS using the system's default configuration" (`config.rs:43`), but the bundled Mozilla roots are what is used.
- **Failure scenario:**
  1. An enterprise runs agents behind an internal CA (Vault PKI, AD CS, cert-manager CA issuer), or behind a transparent TLS-inspecting proxy such as Zscaler, Netskope or Palo Alto.
  2. `ClientBuilder::from_card(&card)?.build()` then every call fails with `client error (Connect)` (B-05).
  3. Mounting `/etc/ssl/certs` or setting `SSL_CERT_FILE` changes nothing.
  4. The only workarounds are gRPC, WebSocket or plaintext.
- **Fix:**
  - Add `ClientBuilder::with_tls_config(rustls::ClientConfig)`, used by every HTTP transport the builder makes, the card resolver and the token provider, plus `JsonRpcTransport::with_https_client` and `RestTransport::with_https_client`.
  - Add an opt-in `tls-native-roots` feature (`rustls-native-certs`, which also honours `SSL_CERT_FILE`/`SSL_CERT_DIR`), or `rustls-platform-verifier`.
  - Add an env-driven CA, for example `A2A_CA_CERTIFICATE`, mirroring `OTEL_EXPORTER_OTLP_CERTIFICATE`.
  - Make `TlsConfig` carry the config instead of being a dead enum (see B-17).
- **Status:** verified by reading. Overlaps the KNOWN adopter-audit C13 ("only the bundled webpki roots are trusted"). Not in handoff *Open engineering*.

### B-02: critical. Only OTLP export honours `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY`; every other outbound path ignores them

- **Where:**
  - Client: `transport/connector.rs:44-52` (plain `HttpConnector`), `tls.rs:122-138`.
  - WebSocket: `tokio-tungstenite-0.30.0/src/connect.rs:91` `TcpStream::connect(addr)`.
  - gRPC: tonic 0.14.6 has no proxy support (grep finds none in `transport/channel`).
  - Push: `push/sender.rs:55-70`.
  - JWKS: `auth/jwt.rs:975-1000`.
  - Model: `tool_loop/openai.rs:114`.
  - Contrast with OTLP: `otel/telemetry/proxy.rs:87-100`:
  ```rust
  .all(first(env, &["ALL_PROXY", "all_proxy"]))
  .http(first(env, &["HTTP_PROXY", "http_proxy"]))
  .https(first(env, &["HTTPS_PROXY", "https_proxy"]))
  .no(unbracketed(&first(env, &["NO_PROXY", "no_proxy"])))
  ```
  A grep for `proxy` in `crates/a2a-protocol-client/src` finds only comments.
- **Failure scenario:**
  1. A pod's only egress is a Squid or corporate proxy, set through `HTTPS_PROXY`.
  2. Telemetry exports fine, so the deployment looks healthy.
  3. `JwtAuthInterceptor::from_oidc_issuer("https://login.microsoftonline.com/...")` cannot reach the IdP, so every authenticated request fails.
  4. `ClientCredentialsProvider` cannot reach its token endpoint.
  5. Push delivery and `OpenAiChatModel` time out after their full timeouts, and calls to remote agents fail.
  6. The same process reads the same variable differently on different paths: one process, two behaviours.
- **Fix:**
  - Move `telemetry/proxy.rs`'s `ProxyConnector` (hyper-util `Matcher` plus `CONNECT` tunnel) into a shared internal module, or the client crate, and wrap every `HttpConnector` with it.
  - For tonic, use `connect_with_connector` with the tunnelling connector, as `exporters.rs:211-237` already does.
  - For WebSocket, dial through the same tunnel and pass the stream to `client_async_tls_with_config`.
  - For push, decide how pinning interacts with a proxy (pin the proxy and let the proxy resolve, or refuse). Document it.
- **Status:** verified by reading. KNOWN adopter-audit C13 ("`HTTPS_PROXY`/`NO_PROXY` are ignored"), still open. The 0.15.1 OTLP fix widened the inconsistency.

### B-03: high. The card resolver, client JWKS fetch and token provider have no TLS option at all, so private-CA users of WebSocket and gRPC still cannot bootstrap

- **Where:**
  - `discovery.rs:576-577` `let client = crate::tls::build_https_client();`
  - `jwks.rs:72-73` (same)
  - `token_provider.rs:696-710`, `token_provider/exchange.rs:264` `client: build_token_http_client()`
  - `CardFetchOptions` has only `headers`, `timeout` and signing fields (`discovery.rs:108-118`).
- **Failure scenario:**
  1. A team configures `WebSocketTransport::with_tls_config(tls_config_with_extra_roots(..))`, or `with_grpc_tls_config`, for its internal CA.
  2. `resolve_agent_card("https://agent.corp")` fails before either transport is ever built.
  3. The same happens to `a2a card verify --jwks https://...` and to an OAuth client-credentials flow against an internal Keycloak.
- **Fix:** add `CardFetchOptions::with_tls_config`, a `ClientCredentialsProvider::with_tls_config`/`with_http_client` builder, and a `fetch_jwks` options field, or route all of them through the builder-wide config proposed in B-01.
- **Status:** verified by reading.

### B-04: medium. Server-side OIDC discovery cannot use a private CA, and JWKS over `http://` is accepted from any host

- **Where:** `auth/jwt.rs:795-798`, `:1052-1062`, `:1005-1020`.
- **Evidence:**
  ```rust
  pub async fn from_oidc_issuer(issuer: &str, validator: JwtValidator) -> A2aResult<Self> {
      let jwks_url = discover_jwks_uri(issuer).await?;
      Ok(Self::from_jwks_url(validator, jwks_url))
  ```
  `discover_jwks_uri` uses `build_jwks_client()` (webpki only), and the discovered `jwks_uri` is fetched with the webpki client even if the issuer is internal. `http_get_json` checks no scheme. By contrast, the client's `fetch_jwks` refuses `http://` beyond loopback (`client/src/jwks.rs:59-64`).
- **Failure scenarios:**
  - A Keycloak or ADFS issuer on an internal CA cannot use `from_oidc_issuer`. The user must hand-copy `jwks_uri` into `from_jwks_url_with_tls_config`, which breaks if the IdP moves it.
  - `from_jwks_url("http://idp.example/keys")` silently trusts keys fetched over plaintext. An on-path attacker substitutes keys and mints tokens.
- **Fix:** add `from_oidc_issuer_with_tls_config`. Apply the client's `is_trustworthy_url` rule to both URLs, with an explicit opt-out.
- **Status:** verified by reading.

### B-05: high. Connection, DNS and TLS failures surface only as `client error (Connect)`, losing the cause

- **Where:**
  - `jsonrpc.rs:252-255`, `:376`; `rest/request.rs:192`; `rest/streaming.rs:54`
  - `discovery.rs:625`; `jwks.rs:90`
  - `token_provider.rs:515`, `:671`
  - `push/sender.rs:741-743` (`format!("push notification failed: {e}")`)
  - `auth/jwt.rs:1019`
- **Evidence:**
  ```rust
  .map_err(|e| {
      trace_error!(method, error = %e, "HTTP client error");
      ClientError::HttpClient(e.to_string())
  })?;
  ```
  hyper-util `client/legacy/client.rs:1670-1673`:
  ```rust
  impl fmt::Display for Error {
      fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
          write!(f, "client error ({:?})", self.kind)
  ```
  The cause sits in `source()`, which `to_string()` drops. A `chain()` helper exists (`error/mod.rs:386-397`) but only the `A2aError` conversion uses it. OTLP, by contrast, prints the chain plus a hint: "it trusts Mozilla's roots, not the operating system's" (`book/.../debugging.md:115`).
- **Failure scenario:** `UnknownIssuer` (a private CA or MITM proxy), `NotValidForName` (an IP SAN mismatch), `failed to lookup address information` (DNS, or a k8s `ndots`/search miss), `Connection refused` and a proxy-only network all print the same line: `HTTP client error: client error (Connect)`. Operators cannot tell a DNS failure from a trust failure, and both are exactly what these deployments hit first.
- **Fix:** build the message from the full `source()` chain everywhere, and reuse the OTLP `hints.rs` table for TLS and DNS hints.
- **Status:** verified by reading. KNOWN adopter-audit C21 ("`HttpClient(String)` throws away the error chain"), unfixed.

### B-06: high. SSE responses lack `X-Accel-Buffering: no`, so events stall behind a default nginx

- **Where:** `server/src/streaming/sse.rs:514-518`.
- **Evidence:**
  ```rust
  hyper::Response::builder()
      .status(200)
      .header("content-type", "text/event-stream")
      .header("cache-control", "no-cache")
      .header("transfer-encoding", "chunked")
  ```
  A grep for `x-accel` across the crates and the book finds nothing. The book has no `proxy_buffering` guidance.
- **Failure scenario:**
  1. A plain nginx sits in front with its default `proxy_buffering on`.
  2. nginx buffers the upstream response, so a few small status events, and 15-byte `: keep-alive` comments every 30 s, sit in its buffer instead of reaching the client.
  3. The client sees nothing until the stream ends or the buffer fills.
  4. `DEFAULT_STREAM_FIRST_EVENT_TIMEOUT` (5 min) eventually fires, or streaming silently degrades into "everything at the end".
  - ingress-nginx defaults `proxy-buffering: off`, so it works there. Plain nginx, Apache `mod_proxy` with buffering, and some CDNs do not.
  - `cache-control: no-cache` without `no-transform` also lets compressing intermediaries gzip-buffer the stream (conjecture).
- **Fix:** add `x-accel-buffering: no` and `cache-control: no-cache, no-transform` to every SSE response (JSON-RPC, REST, axum). Add a book section "Behind nginx / ALB", covering `proxy_buffering off`, `proxy_read_timeout`, `proxy_http_version 1.1`, and the WebSocket `Upgrade` headers.
- **Status:** headers verified by reading. The nginx buffering behaviour is documented nginx behaviour, not measured here.

### B-07: medium. Client pool idle timeout (90 s) exceeds this server's idle timeout (75 s) and AWS ALB's (60 s), so stale connections get reused

- **Where:**
  - `client/src/tls.rs:135-137` `.pool_idle_timeout(Duration::from_secs(90))` (and the plaintext twin at `jsonrpc.rs:134-137`)
  - `server/src/serve/graceful/config.rs:44` `DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(75)`
  - push client: hyper-util default (90 s)
- **Failure scenario:**
  1. A client calls an agent every ~70 s.
  2. The server, or the ALB at 60 s, closes the idle keep-alive connection at the same moment the client reuses it.
  3. hyper reports `IncompleteMessage` or a connection reset.
  4. For `SendMessage` without an idempotency key this is an ambiguous failure the retry layer correctly refuses to retry. The result is sporadic, unreproducible send errors in production, exactly at LB idle boundaries.
- **Fix:** set the client pool idle to under 60 s (for example 50 s) and document the rule "client idle < LB idle < server idle".
- **Status:** values verified by reading. The race is a well-known hyper/LB behaviour; its frequency is conjecture.

### B-08: medium. A path prefix from a reverse proxy becomes a tenant on HTTP+JSON, and the card is served only at the root

- **Where:** `dispatch/rest/query.rs:36-55`; `dispatch/rest/mod.rs:183`; `dispatch/jsonrpc/mod.rs:116`; `dispatch/well_known.rs:24`.
- **Evidence:**
  ```rust
  if let Some(no_slash) = path.strip_prefix('/')
      && let Some(slash_pos) = no_slash.find('/')
  {
      let first = &no_slash[..slash_pos];
      ...
      if !first.is_empty() && !is_route_head(first) && is_route_head(head) {
          return (Some(first), remaining);
  ```
  The card match is an exact compare: `if path == "/.well-known/agent-card.json"`. No base-path or prefix option exists (a grep for `path_prefix|base_path|mount|nest` finds nothing).
- **Failure scenario:**
  1. nginx has `location /a2a/ { proxy_pass http://agent:8080; }`. There is no trailing slash on `proxy_pass`, so the prefix is kept.
  2. `POST /a2a/message:send` is parsed as tenant `a2a`.
  3. Tasks are created in tenant `a2a`, while the same agent reached directly (or via another route) sees tenant `None`, so `GetTask` returns `TaskNotFound` across routes.
  4. With a tenant allowlist or resolver, every request is refused.
  5. Two-segment prefixes (`/agents/foo/...`) give 404.
  6. The client's `resolve_agent_card("https://gw/a2a")` asks for `/a2a/.well-known/agent-card.json`, which neither dispatcher serves.
- **Fix:** add `DispatchConfig::with_base_path("/a2a")`, stripped before tenant parsing and card matching, and document the prefix-stripping requirement.
- **Status:** verified by reading. The tenant capture follows from the quoted code and the unit test `strip_tenant_bare_canonical_form`.

### B-09: medium. Trust in `X-Forwarded-For` is a hop count only; there is no trusted-proxy address check, `ip:port` entries make a new key per connection, and `Forwarded` is unsupported

- **Where:** `rate_limit/identity.rs:67-82`, `:93-103`; `rate_limit/config.rs:28-42`; `auth/throttle.rs:30`, `:79`.
- **Evidence:**
  ```rust
  let xff = ctx.http_headers().get("x-forwarded-for")?;
  ...
  (entries.len() >= hops).then(|| canonicalize_caller_ip(entries[entries.len() - hops]))
  ```
  ```rust
  let trimmed = entry.trim().trim_start_matches('[').trim_end_matches(']');
  match trimmed.parse::<IpAddr>() {
      ...
      Err(_) => trimmed.to_string(),
  ```
- **Failure scenarios:**
  1. **Bypass through a direct path.** `trusted_proxy_hops = 1` is set for an ALB, but the pod is also reachable directly (in-cluster `ClusterIP`, a NodePort, a sidecar). A caller sending `X-Forwarded-For: <random>` on each request gets a fresh bucket each time. Nothing checks that the immediate peer (`:peer-addr`) is a trusted proxy.
  2. **Port in the entry.** Azure Application Gateway appends `ip:port` to XFF. `"203.0.113.7:51234"` does not parse as an `IpAddr`, so the raw string, port included, becomes the key. Every new TCP connection gets a new rate-limit bucket, which defeats the limiter. IPv6 `[2001:db8::1]:443` is also mis-trimmed.
  3. **Shared `anon` bucket.** A request with fewer entries than `hops` (a direct caller sending no XFF) lands in the shared `anon` bucket, so one direct abuser exhausts it for all direct callers.
  4. **`Forwarded` (RFC 7239) is ignored**, so proxies that emit only `Forwarded` (some Envoy and Traefik configurations) get the shared fallback.
- **Fix:** take a `trusted_proxies: Vec<IpNet>` and walk XFF from the right while each hop is trusted, as Go `realip` and Rails `RemoteIp` do. Strip `:port` and `[..]:port`. Parse `Forwarded: for=`.
- **Status:** verified by reading. The Azure `ip:port` behaviour is from vendor documentation (knowledge), not tested here.

### B-10: medium. Long-lived streams are not kept alive through L7 idle timeouts on the WebSocket and gRPC bindings (off by default)

- **Where:**
  - `dispatch/websocket.rs:177` `idle_timeout: None`. Pings are only sent when an idle timeout is configured (`:775-777` returns early: `let Some(idle) = idle_timeout else { return reader.next().await; };`).
  - The client WebSocket transport sends no pings (grep).
  - `dispatch/grpc/dispatcher.rs:47` `keepalive: None` (HTTP/2 PING off by default).
  - The gRPC client sets TCP keepalive only (`client/src/transport/grpc.rs:294-300`).
- **Failure scenario:**
  1. Behind an AWS ALB (60 s idle), GCP or Azure LB, a WebSocket connection carrying a long-running task, or an idle client between calls, is closed by the LB after 60 s of silence.
  2. gRPC server streams that are quiet for over 60 s are reset in the same way.
  3. TCP keepalive probes do not count as traffic at an L7 LB, which terminates TCP.
  4. SSE survives because of its 30 s comments. The other two bindings do not, which is an inconsistency between bindings.
- **Fix:**
  - WebSocket: ping every ~25-30 s by default, independent of the idle-close budget.
  - gRPC: enable HTTP/2 keepalive at about 30 s, or send a periodic no-op status on idle streams.
  - Document LB idle settings.
- **Status:** defaults verified by reading. LB behaviour is from vendor documentation (conjecture as to specific LBs).

### B-11: medium. A blocking `SendMessage` longer than the LB idle timeout returns 504; nothing keeps it alive and the docs do not warn

- **Where:** the JSON-RPC and REST blocking path (the response is written only on completion). The client default `request_timeout` is 30 s (`config.rs:327`). The book's `tool-loop-agent.md:141` mentions only the server's own 75 s.
- **Failure scenario:**
  1. A user raises the client timeout to 5 minutes for a slow model turn.
  2. nginx `proxy_read_timeout 60s`, or ALB at 60 s, returns 504 while the task keeps running. The client classifies the 504 as an ambiguous send failure.
  3. GCP's external HTTP(S) LB backend-service timeout defaults to 30 s and, as far as I know, also bounds streaming responses (conjecture).
- **Fix:** document "use `return_immediately` + `GetTask`/subscribe, or raise the LB timeout". Optionally write leading JSON whitespace periodically on long blocking JSON-RPC responses (valid JSON).
- **Status:** conjecture (behavioural), with configuration values verified.

### B-12: medium. The WebSocket client dials addresses one at a time with no happy eyeballs, so dual-stack hosts with broken IPv6 fail inside the connect timeout

- **Where:** `client/src/transport/websocket.rs:636-656`; `tokio-tungstenite-0.30.0/src/connect.rs:88-91`:
  ```rust
  let addr = format!("{domain}:{port}");
  let socket = TcpStream::connect(addr).await.map_err(Error::Io)?;
  ```
- **Failure scenario:**
  1. `wss://agent.example` has AAAA and A records. The client host has an IPv6 address but a broken IPv6 route, which is common in Docker and some VPNs.
  2. tokio tries the IPv6 address first and waits for the OS SYN timeout (about 2 min).
  3. The outer `connect_timeout` (10 s in the CLI and default config) expires before IPv4 is tried, so the connection always fails.
  4. The HTTP transports (hyper-util `HttpConnector`, `happy_eyeballs_timeout: Some(300ms)`, `http.rs:228`) and tonic would succeed.
  5. The socket also gets no TCP keepalive, unlike every other client transport (`connector.rs:44-52`).
- **Fix:** dial with `hyper_util::client::legacy::connect::HttpConnector` (happy eyeballs, keepalive, the B-02 proxy tunnel), then `client_async_tls_with_config` on the stream.
- **Status:** verified by reading (tokio sequential connect is documented behaviour).

### B-13: medium. HTTP push delivery pins only the first resolved address, with no failover across A/AAAA records or retries

- **Where:** `server/src/url_guard.rs:360-375`; `push/pinning.rs:76-85`; `push/sender.rs:600-617`.
- **Evidence:**
  ```rust
  for socket_addr in resolved {
      ...
      if pinned.is_none() {
          pinned = Some(socket_addr);
  ```
  The pinned URI is reused for every attempt (`sender.rs:666-672`).
- **Failure scenario:** a webhook host with two A records (two LB nodes), where the first resolved node is down or is an unreachable IPv6 address. All 3 attempts go to the same dead address and delivery fails. Without pinning, hyper's happy-eyeballs connector would have succeeded.
- **Fix:** keep all validated addresses, try each per attempt or in happy-eyeballs order, and re-resolve per attempt (re-validating each time).
- **Status:** verified by reading.

### B-14: medium. No certificate hot-reload; the gRPC listener cannot rotate certificates without a restart

- **Where:**
  - `server/src/tls.rs` (`Tls(tokio_rustls::TlsAcceptor)` built once)
  - `serve/graceful/tls.rs:38-56`
  - `dispatch/websocket.rs:215-233`
  - `dispatch/grpc/dispatcher.rs:122-125` and `:328` (`server.tls_config(tls.clone())`)
  - The book says so itself: `containers.md:126-128`, "the example does not show a certificate source or a reload path".
- **Failure scenario:**
  - cert-manager renews a 90-day Let's Encrypt or internal certificate by rewriting the mounted Secret.
  - The HTTP and WebSocket listeners keep serving the old certificate until restart. They can be made to reload only by a user who writes a `rustls::server::ResolvesServerCert`, which `with_tls_pem` cannot express.
  - The gRPC listener takes tonic's static `ServerTlsConfig`, so it can only rotate by restarting. Clients start failing with `CertificateExpired` on day 90.
- **Fix:** ship a `ReloadingCertResolver` (watches PEM paths, swaps an `ArcSwap<CertifiedKey>`) usable with `with_tls`. For gRPC, serve through `serve_with_listener` using `tokio-rustls` with the same resolver instead of tonic's TLS.
- **Status:** verified by reading.

### B-15: medium. The CLI has no TLS or proxy flags, so it cannot reach private-PKI, self-signed or proxy-only agents

- **Where:** `tools/a2a-cli/src/cli.rs:44-85` (global options: `binding`, `timeout`, `header`, `tenant`, `grpc_plaintext`, `no_capability_check`); `connect.rs:136-182`.
- **Failure scenario:** a developer runs `a2a card https://agent.internal` or `a2a send https://localhost:8443 ...` against a self-signed development agent. It fails with `client error (Connect)` and there is no `--cacert`, `--cert/--key`, `--proxy` or `--insecure` to try. curl users expect at least `--cacert` and `HTTPS_PROXY`.
- **Fix:** add `--cacert FILE` (repeatable), `--cert`/`--key`, honour `*_PROXY`, and possibly `SSL_CERT_FILE`. All depend on B-01, B-02 and B-03.
- **Status:** verified by reading.

### B-16: medium. The `deploy-agent` template binds `0.0.0.0` only, so it is unreachable in IPv6-only clusters, and it serves without `ConnectInfo`

- **Where:** `examples/deploy-agent/src/main.rs:263` and `:283`.
- **Evidence:**
  ```rust
  let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port)).await?;
  ...
  axum::serve(listener, router)
  ```
- **Failure scenarios:**
  - **IPv6-only cluster.** On EKS in IPv6 mode, or other single-stack IPv6 Kubernetes, the pod IP is IPv6 only. The process listens on IPv4 only, so kubelet probes and Service traffic get connection refused and the pod never becomes Ready. There is no `HOST`/`BIND` variable to change it. `[::]` would be dual-stack on Linux by default.
  - **No peer address.** `axum::serve(listener, router)` without `into_make_service_with_connect_info::<SocketAddr>()` gives no peer address (`axum_adapter.rs:289-308` relies on it). Anyone who adds the documented rate limiter or auth throttle to this template gets everyone in one `anon` bucket.
- **Fix:** bind `[::]` (falling back to `0.0.0.0` when IPv6 is unavailable), or read `BIND_ADDR`. Use `into_make_service_with_connect_info`.
- **Status:** verified by reading. The IPv6-only failure follows from socket semantics; it was not run (the handoff notes no IPv6 in the sandbox).

### B-17: low. `TlsConfig` is dead configuration: `without_tls()` is a no-op and `Rustls` claims to use the system's configuration

- **Where:** `client/src/config.rs:40-48`; `builder/mod.rs:506-511`. A grep for `config.tls` / `.tls` in the client finds only the setter and tests.
- **Evidence:**
  ```rust
  /// Disables TLS (plain HTTP only).
  pub const fn without_tls(mut self) -> Self {
      self.config.tls = TlsConfig::Disabled;
  ```
  `http_transport()` (`transport_factory.rs:269-297`) never reads `config.tls`.
- **Failure scenario:**
  - A user calls `.without_tls()` to forbid TLS (for example, to make sure only a mesh-encrypted plaintext path is used). `https://` still dials.
  - A user reads "system's default configuration" and assumes the corporate root in the OS store is trusted (B-01).
- **Fix:** make `TlsConfig` carry the `rustls::ClientConfig` (or `Native`/`Webpki`/`Custom`) and honour it, or deprecate it.
- **Status:** verified by reading.

### B-18: low. `tls_config_with_extra_roots` silently drops an unparsable CA, while OTLP fails hard

- **Where:** `client/src/tls.rs:84-88`:
  ```rust
  if let Err(_err) = store.add(cert) {
      trace_warn!(error = %_err, "failed to add custom CA certificate to root store");
  ```
  OTLP (`exporters.rs:143-147`) returns `TelemetryError::Tls("extra CA certificate refused")`. The client helper also takes DER only, with no PEM helper, whereas OTLP and the server take PEM.
- **Failure scenario:** a wrong file or a PEM passed as DER is dropped with a warning (invisible without the `tracing` feature). The process then starts and fails every handshake with an opaque error (B-05).
- **Fix:** return `Result`, and add `tls_config_with_extra_roots_pem(&[u8])`.
- **Status:** verified by reading.

### B-19: low. gRPC client through L7 or TLS-terminating proxies: ALPN `h2` is mandatory, plaintext means h2c prior-knowledge, and one connection pins one replica

- **Where:**
  - tonic `transport/channel/service/tls.rs:148-150`: `if !(alpn_protocol == Some(ALPN_H2) || self.assume_http2) { return Err(TlsError::H2NotNegotiated.into()); }`. The client never sets `assume_http2` (grep).
  - `client/src/transport/grpc.rs:290-301`: one `Channel`, no `balance_list`.
- **Failure scenarios:**
  - A TLS-terminating proxy or inspection device that does not offer `h2` in ALPN (older nginx `listen 443 ssl` without `http2`, some corporate inspection boxes) fails every gRPC dial with `H2NotNegotiated`, while JSON-RPC works.
  - Plaintext `http://` gRPC through an L7 LB that accepts no h2c on the frontend fails.
  - Behind a Kubernetes ClusterIP (L4), all calls from one client go to one replica for the life of the connection, so there is no load spread (conjecture as to impact).
- **Fix:** document these. Optionally expose `assume_http2`. Document headless Services or an L7 mesh for gRPC balancing.
- **Status:** verified by reading (tonic code); deployment impact is conjecture.

### B-20: low. Root-store semantics differ between transports

- **Where:** `client/src/transport/grpc.rs:349-353` (`config.tls_config.clone().unwrap_or_else(|| ClientTlsConfig::new().with_enabled_roots())`) versus `tls.rs:80-94`.
- **Evidence:** for HTTP and WebSocket, the extra roots are added to Mozilla. For gRPC, a caller-supplied `ClientTlsConfig::new().ca_certificate(..)` replaces Mozilla, because `with_enabled_roots` is applied only when the caller supplies none. `with_enabled_roots` also turns on native roots for gRPC only, if any crate in the graph enables tonic's `tls-native-roots` (tonic `channel/tls.rs:119-127`), so cargo feature unification silently changes gRPC trust and nothing else.
- **Failure scenario:**
  - A user sets one internal CA for gRPC and later calls a public gRPC agent, which fails.
  - Adding an unrelated dependency changes gRPC trust.
- **Fix:** document the difference, or build the gRPC TLS from the same `rustls::ClientConfig` through `connect_with_connector`, as OTLP already does.
- **Status:** verified by reading.

### B-21: low. The SSE response sets `transfer-encoding: chunked` explicitly, which is illegal in HTTP/2

- **Where:** `streaming/sse.rs:518`.
- **Evidence:** hyper strips it on h2 with `warn!("Connection header illegal in HTTP/2: ...")` (`hyper-1.12.0/src/proto/h2/mod.rs:50-55`).
- **Failure scenario:** harmless on hyper's own servers. A user who embeds the response in another HTTP stack, a tower middleware or a proxy in Rust that re-emits headers verbatim may send an invalid h2 response, or a duplicate or contradictory framing header on h1.
- **Fix:** drop the header and let hyper choose the framing.
- **Status:** verified by reading.

### B-22: low. The SSRF classifier misses some special ranges

- **Where:** `server/src/url_guard.rs:66-72`, `:150-188`.
- **Evidence:** `is_private_v4` covers loopback, RFC 1918, link-local, `0.0.0.0` and CGNAT. `is_private_ip` for IPv6 covers `::1`, `::`, `fc00::/7`, `fe80::/10`, and embedded IPv4 for `::ffff:0:0/96`, `64:ff9b::/96` and IPv4-compatible addresses. Not covered:
  - `0.0.0.0/8` (beyond `0.0.0.0` itself)
  - `192.0.0.0/24`, `198.18.0.0/15`, `240.0.0.0/4`, `255.255.255.255`, multicast `224.0.0.0/4`
  - 6to4 `2002::/16` (embeds IPv4, for example `2002:a9fe:a9fe::` → 169.254.169.254)
  - Teredo `2001::/32`, `64:ff9b:1::/48` (local-use NAT64), `fec0::/10`, `ff00::/8`
- **Failure scenario:** on a host with a 6to4 or NAT64 local gateway, a webhook `http://[2002:a9fe:a9fe::]/` might reach the metadata service. This is environment-dependent and unlikely (conjecture).
- **Fix:** extend the classifier (6to4 embedding, special-purpose registries), or switch to a maintained crate's `is_global` equivalent.
- **Status:** coverage verified by reading; exploitability is conjecture.

### B-23: low. OTLP proxy handling differs from curl and Go in a few places, and some `NO_PROXY` forms are ignored

- **Where:** `otel/telemetry/proxy.rs:4-30`, `:87-100`; hyper-util `matcher.rs:444-466`, `:515-535`.
- **Evidence:**
  - Upper case wins over lower case (curl prefers lower case; documented in `tls-proxies.md:47-50`).
  - `NO_PROXY=collector:4318` is treated as a domain and never matches, where Go matches host:port.
  - `NO_PROXY=*.svc.cluster.local` (glob form, common in Java tooling) never matches. Only bare `*` is a wildcard.
  - `socks5://` and `https://` proxies are ignored with one warning and the export goes direct (`proxy.rs:25-27`). In a proxy-only network that means silent loss after one warning.
- **Fix:** document the unsupported forms. Accept `*.domain` as `.domain`. Strip `:port` entries, or match them.
- **Status:** verified by reading. Partly documented; the handoff says IPv6 `NO_PROXY` is only unit-tested (KNOWN handoff, line 85-86).

### B-24: low. No service discovery (mDNS, DNS-SD or registry)

- **Where:** a grep for `mdns|dns-sd|zeroconf|_a2a._tcp|consul|etcd|registry` in `crates`, `examples` and `bindings` finds only unrelated proto and registry words.
  - `examples/swarm/src/topology.rs:20-25` wires peers by passing URLs in process. It also binds port 0, drops the socket and rebinds the same port, which is a TOCTOU race in which another process can take the port (`probe` dropped at `:24`, `serve_with_addr(addr, ...)` at `:37`).
- **Failure scenario:**
  - Local agent swarms (several processes on a LAN or in compose) must be configured by hand with URLs.
  - The push SSRF guard refuses `*.local` and private addresses by default (`url_guard.rs:229-237`), so mDNS-named agents also cannot receive pushes without `allow_private_urls()`.
- **What would be needed:**
  1. A `discovery::Registry` trait (static list, DNS SRV/TXT, mDNS `_a2a._tcp` with a TXT `card=/.well-known/agent-card.json`, Kubernetes Endpoints).
  2. A server-side announcer.
  3. A resolver that yields card URLs to `CachingCardResolver`.
  4. A trust model: signed cards (the `signing` feature exists) should be required when discovery is unauthenticated, since mDNS is spoofable.
  5. An explicit `allow_private_urls` scope per discovered peer.
- **Status:** verified by reading (absence).

### B-25: low. The WebSocket binding cannot share the HTTP port or router

- **Where:** a grep for `hyper::upgrade|on_upgrade` in the server finds nothing. `WebSocketDispatcher::serve*` binds its own listener (`dispatch/websocket.rs:403-447`).
- **Failure scenario:**
  - On platforms that expose exactly one port (Cloud Run, Heroku, App Runner, many PaaS), the WebSocket interface cannot be offered alongside JSON-RPC.
  - In Kubernetes it needs a second Service port and ingress rule.
- **Fix:** an axum/hyper upgrade handler that hands the upgraded stream to the WebSocket connection loop.
- **Status:** verified by reading.

### B-26: low. mTLS behind a TLS-terminating proxy has no forwarded-client-certificate support

- **Where:** `MutualTlsAuthInterceptor` reads only the `PeerCertificate` extension or `:peer-certificate` key (`call_context.rs:97-106`). The book admits "there is no client certificate to read" (`authentication/mtls.md:68-69`).
- **Failure scenario:** behind Istio or Envoy (`x-forwarded-client-cert`), nginx (`ssl_client_escaped_cert`) or AWS ALB mTLS passthrough (`x-amzn-mtls-clientcert`), the interceptor sees nothing. Users either write their own interceptor or naively trust a client-settable header.
- **Fix:** an opt-in `ForwardedClientCertInterceptor` keyed on a trusted-proxy check (see B-09), parsing XFCC and URL-encoded PEM.
- **Status:** verified by reading.

### B-27: low. IPv6 edge cases

- **Where:** `client/src/loopback.rs` (`is_loopback_host`); `otel/telemetry/prometheus.rs:57-69`.
- **Evidence and failure scenarios:**
  - **IPv4-mapped loopback.** `::ffff:127.0.0.1` is not loopback per `Ipv6Addr::is_loopback`. A gRPC bare target `[::ffff:127.0.0.1]:50051` is dialled over TLS by `HttpsExceptLoopback`, and the plaintext-credential warning fires spuriously. Minor.
  - **Zone IDs.** Link-local IPv6 with zone IDs (`http://[fe80::1%25eth0]:8080`) does not parse in `hyper::Uri`, so peer-to-peer swarms on link-local cannot be addressed.
  - **Prometheus bind address.** `OTEL_EXPORTER_PROMETHEUS_HOST` is passed to `TcpListener::bind((host.as_str(), port))`. `[::]` (bracketed) fails resolution; it must be `::`. The default `localhost` may resolve to `::1` only and be unscrapable from another pod (documented knob).
  - **Shell bind addresses.** `Server::bind("[::]:8080")` works and is dual-stack unless `net.ipv6.bindv6only=1`. With `bindv6only=1` set, the server is IPv6 only, and the SDK offers no `IPV6_V6ONLY=false` helper (`Server::from_listener` exists for this, `serve/graceful/mod.rs:195`).
- **Status:** verified by reading; the zone-ID point is from hyper's URI grammar (conjecture).

### B-28: low. DNS: no caching, the connect timeout does not cover resolution, and Kubernetes `ndots:5` amplifies lookups; push delivery has a lookup bound but transports do not

- **Where:** hyper-util `HttpConnector` (GaiResolver: blocking `getaddrinfo` on the blocking pool; `connect_timeout` applies to TCP connect after resolution); `connector.rs:44-52`; `discovery.rs:577` (a fresh client per card fetch, so a fresh DNS lookup and TLS handshake each time, KNOWN C17); `push/sender.rs:344-347` (push has `with_dns_timeout`, 5 s).
- **Failure scenario:**
  1. In Kubernetes with `ndots:5`, `agent.example.com` (2 dots) first tries the 5 search domains, 10 or more queries, before the absolute name.
  2. A slow or broken upstream DNS then consumes most of `request_timeout`, and the failure is reported as `request timed out` or `client error (Connect)` (B-05), not as DNS.
  3. Every card resolve repeats it.
- **Fix:**
  - Expose a resolver hook (or `hickory-resolver` behind a feature) with caching and its own timeout.
  - Pool the card-fetch client.
  - Document trailing-dot FQDNs or a `dnsConfig.options ndots:1` override.
- **Status:** verified by reading; amplification is standard resolver behaviour (conjecture for impact).

### B-29: low. Crypto provider and cipher constraints

- **Where:** every rustls config is `builder_with_provider(ring)` with `with_safe_default_protocol_versions()`: `client/src/tls.rs:60-66`, `push/sender.rs:39-50`, `auth/jwt.rs:983-990`, `server/src/tls.rs` `config_from_pem`, `exporters.rs:148-152`.
- **Evidence and failure scenarios:**
  - TLS 1.2 and 1.3 only (good). Only ECDHE AEAD suites, with no RSA key exchange, so legacy appliances that offer only `TLS_RSA_*` fail the handshake.
  - No FIPS option (`aws-lc-rs` with `fips`), which some regulated deployments require.
  - No OCSP or CRL checking.
  - There is **no insecure skip-verify mode anywhere** (good; confirmed by grep for `danger|ServerCertVerifier`). Self-signed certificates work safely by pinning a CA:FALSE leaf as the root, where a CA can be injected at all (paths 3, 4, 8, 9a, 10, 11).
- **Status:** verified by reading.

### B-30: low. The push sender's connector sets no TCP connect timeout or keepalive

- **Where:** `push/sender.rs:55-70`:
  ```rust
  let mut http = HttpConnector::new();
  http.enforce_http(false);
  http.set_nodelay(true);
  ```
  Unlike the client (`connector.rs:44-52`), there is no `set_connect_timeout` and no keepalive. The same applies to `from_jwks_url_with_tls_config`'s `.build()` (`jwt.rs:769-774`, default `HttpConnector`).
- **Failure scenario:** a blackholed webhook IP holds each attempt for the full `request_timeout` (10 s) instead of failing fast. Bounded, so low.
- **Status:** verified by reading.

---

## Inconsistencies between paths (summary)

1. **Proxy.** OTLP honours `*_PROXY`; the A2A client, card resolver, JWKS (both crates), token provider, push, model client, CLI and MCP bridge do not (B-02).
2. **CA injection.** OTLP (env and code, PEM, hard error), push, server JWKS, model client, gRPC and WebSocket can inject a CA. JSON-RPC, REST, card resolver, client JWKS, token provider, OIDC discovery and the CLI cannot (B-01, B-03, B-04). DER and silent drop in the client versus PEM and hard error in OTLP (B-18).
3. **Root semantics.** HTTP and WebSocket add to Mozilla; gRPC replaces it; tonic feature unification can add OS roots to gRPC only (B-20).
4. **JWKS scheme check.** The client refuses `http://` beyond loopback; the server accepts it (B-04).
5. **Keepalive.** SSE sends 30 s comments; WebSocket and gRPC send nothing by default (B-10). Client TCP keepalive is set on HTTP and gRPC, not on WebSocket or push (B-12, B-30).
6. **Happy eyeballs.** Present on hyper and tonic paths; absent on WebSocket and on pinned `http://` push (B-12, B-13).
7. **Error reporting.** OTLP chains causes and gives hints; every other path prints `client error (Connect)` (B-05).

## Notes on what was not covered

- The SLIMRPC binding's TLS (delegated to `slim_config`) was not audited beyond `slim_node.rs`.
- Nothing was run: no IPv6, LB or proxy behaviour was measured. The handoff confirms the sandbox has no IPv6.
