<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Draft comment for `open-telemetry/opentelemetry-rust` #2008 — the `https://` default still enables no roots

**Status: DRAFT, not sent.** #2008 ("OTLP exporter fails to send data to
backend requiring TLS") was open on 2026-10-06 per GitHub's page. Read from
source, not reproduced against a public endpoint (none reachable here): the
behaviour below is a consequence of the two files cited, and `a2a-rust`'s
workaround is tested against a private-CA gRPC collector.

---

Still present in `opentelemetry-otlp` 0.33.0 with tonic 0.14.6, and with the
`tls-webpki-roots` (or `tls-roots`) feature on. For an `https://` endpoint
and no explicit TLS configuration the exporter builds

```rust
// opentelemetry-otlp 0.33.0, src/exporter/tonic/mod.rs:313-314
None if is_https => endpoint.tls_config(ClientTlsConfig::new())
```

and in tonic 0.14.6 `ClientTlsConfig::new()` is `Default::default()`, with
`with_webpki_roots` and `with_native_roots` both `false`
(`src/transport/channel/tls.rs`). The roots features compile the roots in;
nothing turns them on. tonic 0.14 has the method for exactly this:

```rust
None if is_https => endpoint.tls_config(ClientTlsConfig::new().with_enabled_roots())
```

`with_enabled_roots` enables whichever roots the features compiled in
(`tls.rs:119`). Until then, callers have to pass
`ClientTlsConfig::new().with_webpki_roots()` themselves, as `a2a-rust` does.
