<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0041: Server certificates reload from their files, a proxy may forward the client's, and the crypto provider is the caller's to choose

**Date:** 2026-11-04
**Status:** Accepted (the maintainer's instruction of 2026-11-04: every open
plan item goes into 0.16.0 with the best engineering option, open to a veto in
review; it supersedes F15's "a reloading resolver after 0.16.0"). Built in
0.16.0.
**Author:** Tom F.

---

## Context

**Certificate reload (audit B-14, fork F15).** cert-manager, Vault agent and
ACME clients rotate a certificate by rewriting its files; a Kubernetes secret
volume swaps both files at once through a symlink. Before 0.16.0:

* `Server::with_tls` and `WebSocketDispatcher::with_tls` took a
  `rustls::ServerConfig` built once. Rotation needed a restart, or a
  hand-written `ResolvesServerCert`.
* `GrpcDispatcher::with_tls` took tonic's `ServerTlsConfig`, which holds PEM
  bytes and has no resolver hook (tonic 0.14.6, `transport/server/tls.rs`).
  The built-in gRPC listener could not rotate at all without a restart.

**Crypto provider (audit B-29).** Every default path builds its TLS
configuration on `ring` (`tls::config_from_pem`, the client's `tls.rs`, the
server's `net/roots.rs`, the OTLP exporters). There is no FIPS build, no CRL
or OCSP helper, and no insecure mode. A caller-built `rustls` configuration is
the escape hatch everywhere except the token provider and the gRPC client
(B-03 and B-20, other work packages) and, on the server, the gRPC listener,
whose tonic configuration takes no `rustls::ServerConfig`.

**A client certificate behind a TLS-terminating proxy (audit B-26).** When
Envoy or Istio, nginx or an AWS Application Load Balancer terminates mutual
TLS, the connection the agent sees carries no client certificate, and
`MutualTlsAuthInterceptor` refuses every call. The proxy can forward the
certificate it verified in a header (Envoy's `x-forwarded-client-cert`,
nginx's `$ssl_client_escaped_cert` in a header of the deployer's naming, the
load balancer's `x-amzn-mtls-clientcert`). A header is text any client can
send.

## Options

Certificate reload:

1. **A `ReloadingCertResolver`** that reads the two PEM files, implements
   `ResolvesServerCert`, re-reads them on `reload()` or on a polling
   `watch(interval)`, and keeps the old certificate when a read fails. Usable
   by every listener that takes a `rustls::ServerConfig`; the gRPC listener
   gains `with_rustls(ServerConfig)`, which performs the handshake itself with
   `tokio-rustls` and hands tonic the TLS stream through
   `serve_with_incoming`.
2. As 1, but watching with the `notify` crate (inotify, kqueue,
   `ReadDirectoryChangesW`). A new dependency with platform code; a watch on
   the file misses a Kubernetes symlink swap unless the directory is watched,
   and network file systems deliver no events.
3. **Document restart-on-rotate** and the hand-written resolver.

Crypto provider:

1. **Bring your own configuration.** Every server listener takes a
   `rustls::ServerConfig` (the gRPC one through option 1 above), and the
   reloading resolver takes a `CryptoProvider`. Document `aws-lc-rs`, its FIPS
   build, and rustls' CRL support (`WebPkiClientVerifier::builder(..)
   .with_crls(..)`) as the route.
2. **A `fips` feature** switching every hard-coded `ring` to `aws-lc-rs` with
   FIPS: large, in two copies of the network code, with a native C build in
   every user's tree; nobody has asked for it.
3. **A CRL helper** in this crate: a thin wrapper of the rustls builder call.

Forwarded client certificate:

1. **On the mTLS interceptor:** `MutualTlsAuthInterceptor::with_forwarded(
   ForwardedClientCert)`, read only when the connection's own handshake gave
   no certificate and its peer is in the configured `TrustedProxy` ranges; the
   naming (fingerprint or `with_names`) is the interceptor's, unchanged.
2. **A separate interceptor** that writes the certificate into the call
   context for every later interceptor. `CallContext`'s certificate is read
   from the headers map, which interceptors cannot change, so this needs a
   write-once field on `CallContext` and an ordering rule between
   interceptors.
3. **In the dispatchers,** recording the forwarded certificate as the peer
   certificate at dispatch: every binding (REST, JSON-RPC, axum, WebSocket,
   gRPC) changes, and the trust configuration moves into `DispatchConfig`.

## Decision

Certificate reload: option 1, polling. One `std::fs::read` of two small files
per interval costs nothing, needs no dependency, works on every platform and
file system, and sees a symlink swap. A reload compares a SHA-256 digest of
the two files first, so an unchanged pair is not parsed again and the key's
bytes are not kept to compare against. A pair that fails to load (a rotation
caught between its two writes, a key that is not the certificate's) keeps the
certificate already served and is retried on the next tick. Connections
already established keep the certificate they were opened with. `SIGHUP` is
left to the application, which can call `reload()` from its own handler:
signals are Unix-only and a library should not install a handler.

The gRPC listener's own handshake under `with_rustls` is bounded (10 s), runs
on a task per connection so a slow one does not hold up the accept loop, and
counts against `with_max_connections` from the accept, as the plaintext and
tonic-TLS paths do. Because tonic's `TlsConnectInfo` is the connection info,
`CallContext::peer_certificate` and the peer address reach interceptors as
they do under `with_tls`.

Forwarded client certificate: option 1. It is the smallest change, keeps the
trust decision next to the one consumer that needs it, and reuses the
interceptor's naming. The header is ignored from any peer outside the ranges,
and with no ranges it is never read; the handshake's own certificate always
wins. The forwarded certificate is not verified again here: the proxy must
verify it and must replace a header of that name the client sent. Only the
leaf is read (the `Cert` of the last XFCC element, which the nearest proxy
wrote).

Crypto provider: option 1. With the gRPC listener taking a
`rustls::ServerConfig`, every server listener accepts a configuration built on
any provider, with any client verifier (CRLs included). The book's TLS page
shows how. Options 2 and 3 are not built: 2 waits for a user who needs FIPS,
3 would wrap one builder call.

## Consequences

* New: `ReloadingCertResolver` (feature `tls-rustls`),
  `GrpcDispatcher::with_rustls` (features `grpc-tls` and `tls-rustls`), and
  `auth::ForwardedClientCert` with `MutualTlsAuthInterceptor::with_forwarded`.
  Additive; nothing existing changes behaviour.
* `with_tls(tonic::ServerTlsConfig)` stays; the later of `with_tls` and
  `with_rustls` wins.
* Not built: a file-event watcher; a built-in `SIGHUP` handler; a `fips`
  feature; a CRL helper; OCSP stapling (rustls servers can staple through a
  resolver that sets `CertifiedKey::ocsp`, which a caller's own resolver can
  do). The client-side paths without a configuration hook (token provider,
  gRPC client) are B-03 and B-20.
