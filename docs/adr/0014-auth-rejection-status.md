<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0014: A refused credential answers each binding's own status

**Date:** 2026-09-25
**Status:** Accepted
**Author:** Tom F.
**Supersedes:** the "Rejections map to `InvalidRequest`" consequence of
[ADR 0010](0010-auth-integration.md)

---

## Context

ADR 0010 chose to answer every auth interceptor's refusal as `InvalidRequest`:
HTTP `400`, gRPC `INVALID_ARGUMENT`, JSON-RPC `-32600` in a `200`. Its reason
was that A2A defines no unauthenticated error code, and it sent anyone who
needed a `401` to a gateway.

Two measurements on 2026-09-25 (audit N36) showed what that costs:

- **This SDK's own client could not recover from a revoked or rotated token
  against this SDK's own server.** The client's `BearerAuthInterceptor`
  tells its `TokenProvider` to drop a token when the agent answers `401`
  (gRPC `UNAUTHENTICATED` maps to `401`), and on nothing else — which is how
  OAuth clients generally behave. The server never answered `401`, so the
  refused token was sent again on every call until the provider's own cache
  expired, each call failing as a malformed request. Reproduced on JSON-RPC,
  HTTP+JSON and gRPC (`a2a-protocol-sdk/tests/auth_rejection_e2e.rs`, which
  fails on the old mapping).
- **The ACTS conformance suite fails it.** SEC-AUTH-006 and
  SEC-EXTCARD-001/002/004 require `401` or `403`; SEC-EXTCARD-001 is a MUST.

Spec §3.3.2 and the server requirements under §7 say servers SHOULD use
binding-specific codes for authentication challenges and rejections, and
give HTTP `401`/`403`, gRPC `UNAUTHENTICATED`/`PERMISSION_DENIED` and "a
JSON-RPC custom error" as the examples. The premise of ADR 0010 — that the
spec puts authentication at the transport layer — is right; the HTTP and gRPC
bindings *are* that layer here, and nothing else in front of them is required
to exist.

## Decision

1. `A2aError` gains two constructors, `unauthenticated(message, challenge)`
   and `permission_denied(message)`, which record an `AuthRejection` on the
   error. The rejection is a private, `#[serde(skip)]` field with an
   accessor: never on the wire, never readable from the wire. The error's
   code stays `InvalidRequest`.
2. Each binding answers a rejection with its own status, decided in one place
   per binding (`ServerError::http_status`, `ServerError::status_name`, the
   gRPC `grpc_code`):
   - HTTP+JSON (both dispatchers): `401` with the rejection's
     `WWW-Authenticate` challenge (RFC 9110 §15.5.2 requires one on every
     `401`), or `403`; the AIP-193 body's `status` is `UNAUTHENTICATED` or
     `PERMISSION_DENIED`.
   - JSON-RPC over HTTP: the same `401`/`403` and challenge, with the body
     unchanged — a JSON-RPC error `-32600`. A body-only client sees what it
     saw before; a status-aware one can refresh.
   - gRPC: `UNAUTHENTICATED` or `PERMISSION_DENIED`.
   - JSON-RPC batches answer `200`, since one response answers many calls,
     and each refused call's entry stays `-32600`.
3. The built-in interceptors refuse with `unauthenticated`. The bearer-token
   and JWT interceptors send `Bearer realm="a2a"`; the API-key interceptor
   sends `ApiKey header="<its header>"`, since no registered scheme names an
   API key. The challenge is fixed per interceptor: RFC 6750's
   `error="invalid_token"` would tell a caller whether its token was absent
   or wrong, which these interceptors deliberately never do, and it is a
   SHOULD.

## Consequences

- **HTTP statuses change for every refused credential** — a Behaviour Change
  in 0.14.0. A client or monitor that matched `400` for an auth failure now
  sees `401` or `403`. The JSON-RPC body does not change.
- **WebSocket is not covered.** The WebSocket binding runs interceptors per
  message after the upgrade, when there is no HTTP status to send; a refusal
  there is still only the `-32600` body. Authenticating at the upgrade is the
  fix, and a separate change.
- **A custom interceptor opts in** by returning `A2aError::unauthenticated` or
  `A2aError::permission_denied`; one returning any other error keeps its
  status, as before.
- A gateway in front of the agent remains a valid deployment; it is no longer
  the only way to get a `401`.

## Alternatives considered

- **New `ErrorCode` variants** (`Unauthenticated`, `PermissionDenied`). Cleaner
  to match on, but they would invent JSON-RPC codes in the range A2A reserves
  for its own errors — a later spec revision could assign those numbers — and
  change the JSON-RPC wire for no client that asked for it.
- **Keep `400` and document it** (ADR 0010 as written). No behaviour change,
  but the token-refresh failure above stays, on this SDK's own client, and
  every conformance report carries a MUST failure.
- **Leave JSON-RPC at `200`.** Keeps the JSON-RPC exchange byte-identical, and
  keeps the token-refresh failure on the default binding, which is where most
  adopters are.
