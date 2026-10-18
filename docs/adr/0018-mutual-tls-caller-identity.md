<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0018: A mutual-TLS client certificate names the caller

**Date:** 2026-10-18
**Status:** Accepted
**Author:** Tom F.

---

## Context

The control map's ASI07 row (OWASP Agentic Security, "Insecure Inter-Agent
Communication") was `partial`, and engineering could close one of its gaps.
`Server::with_tls`, `WebSocketDispatcher::with_tls` and
`GrpcDispatcher::with_tls` take a rustls or tonic configuration as built, so
a client verifier gives mutual TLS on every listener
(`tests/serve_tls.rs::a_listener_requiring_client_certificates_serves_only_clients_with_one`).
But no binding passed the certificate on. Mutual TLS decided whether a
connection was admitted, and nothing downstream knew who had made it:
`CallContext::caller_identity`, which task partitions, rate limits, approver
lists and the audit trail key on, stayed empty.

The handoff recorded the gap as "needs an X.509 parser, a new dependency".
That assumes the caller has to be named by a field inside the certificate.

## Options

1. **Parse the certificate in the SDK** (`x509-parser` or similar) and name
   the caller by subject CN or a SAN. Readable names, but it adds a DER/ASN.1
   parser of attacker-supplied input to every TLS build, and the SDK has to
   choose which field is the name. That choice differs per deployment:
   SPIFFE puts the identity in a URI SAN, internal PKIs often in the CN, and
   service meshes in either.
2. **Name the caller by the certificate's SHA-256 fingerprint** and pass the
   DER to a deployment-supplied function for anything else. No parser and no
   new crate: `ring`, which computes the digest, is already in the graph
   through rustls (`cargo tree -i ring` shows v0.17.14 under `rustls`
   before this change). The name is exact. It changes when the certificate
   is renewed, which a deployment that renews often can avoid by naming
   callers itself.
3. **Expose only the DER**, with no interceptor. Correct but incomplete: the
   gap is that nothing names the caller, and every deployment would write
   the same interceptor.

## Decision

Option 2.

- Every listener that terminates TLS carries the leaf of the chain its
  verifier accepted. The HTTP `Server` attaches it to each request of the
  connection as a `PeerCertificate` extension, which the JSON-RPC, REST and
  axum dispatchers read. The WebSocket listener takes it from its own
  handshake, gRPC from tonic's `Request::peer_certs`.
- It reaches interceptors as `CallContext::peer_certificate()`, through the
  headers map under `:peer-certificate` (base64). That is how the peer
  address already travels: a colon cannot start an HTTP field name or a gRPC
  metadata key, so no client can supply the entry. As a second guard, every
  binding removes the key when the server attached no certificate.
- `MutualTlsAuthInterceptor::new()` names the caller
  `mtls:sha256:<64 hex digits>` (`caller_id::certificate_fingerprint`).
  `with_names(f)` names it `mtls:<f(der)>`, and `f` returning `None`
  refuses the call. A call with no certificate is refused: `401` with
  `WWW-Authenticate: MutualTLS`, or gRPC `UNAUTHENTICATED`. The scheme
  recorded is `"mtls"`, and the interceptor claims the card's
  `mtlsSecurityScheme`.
- The interceptor exists under `tls-rustls` or `grpc-tls`, the features
  that terminate TLS. Both now name `dep:ring`, which they already linked.

## Consequences

- ASI07's gap narrows to what engineering cannot close: signing content is
  opt-in, and verifying it is the receiver's call.
- Behind a TLS-terminating proxy there is no client certificate. The
  listener sees the proxy's handshake. A deployment there authenticates
  callers another way, or has the proxy forward the certificate in a header
  that a custom interceptor trusts only from the proxy.
- Naming by subject is the deployment's code, with the parser it trusts.
  The SDK takes on no ASN.1 parsing of attacker-supplied input.
- Tests: `tests/mtls_caller_identity.rs` covers JSON-RPC and REST over
  HTTP/1.1 and HTTP/2, WebSocket over TLS, and the axum extension;
  `a2a-protocol-sdk/tests/grpc_mtls_caller_e2e.rs` covers gRPC. Each client
  sends its chain with the CA, so the leaf, not the last certificate, is
  what names the caller.
