<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Draft: two extensions for human approval and content provenance

**Status: draft, not sent.** Intended for a discussion in `a2aproject/A2A`.
The maintainer decides whether and where to post it.

## Why

Two things that regulated deployments are asked for have no place in A2A
v1.0:

1. **Asking a person before an action, and binding the answer to that
   action.** `input-required` can pause a task. Nothing says what is being
   approved, who may approve it, or whether an answer was to this question
   or to an earlier one. EU AI Act Article 14(4)(d) asks that a person be
   able to override an AI system's output; OWASP ASI09 asks that sensitive
   actions need explicit human confirmation.
2. **Marking and signing content.** Nothing in a message or artifact says
   which agent produced it, survives forwarding through another agent, or
   shows it was not altered by an intermediary that terminated TLS. EU AI
   Act Article 50(2) asks providers of generative systems to mark output as
   machine-generated in a machine-readable format.

Both are in production in a2a-rust as declared extensions, in the style the
spec already allows for extensions (§3.3.4): values in `metadata`, the URI in
`extensions`. Neither changes the core protocol, and a peer that does not
implement either one is served as before.

## Approval: `https://a2a-rust.com/extensions/approval/v1`

**Request.** On the status message of a task in `input-required`,
`metadata["a2a-rust.com/approval"]` holds:

```json
{ "requestId": "…", "summary": "Refund EUR 40 to order 1182",
  "digest": "sha256:<hex of the RFC 8785 JSON of the action>",
  "requestedBy": "<authenticated caller whose run asks>" }
```

**Decision.** On the message that continues the task:

```json
{ "requestId": "…", "decision": "approve" | "deny",
  "digest": "<the digest the approver was shown>", "comment": "…" }
```

**Server semantics** (when the server enforces the extension). A continuation
carrying a decision is refused before the agent runs when any of these holds:

- the task is not waiting on that request;
- the digest differs: the approver saw a different action;
- the caller is not authenticated, or not an allowed approver;
- by default, the caller is the one whose run asked (four eyes).

**Open questions.**
- Should the digest's canonical form be fixed to RFC 8785, as here, or left
  to the agent?
- Should `requestedBy` be signed by the server, so a client can rely on it
  without trusting the transport?

## Provenance: `https://a2a-rust.com/extensions/provenance/v1`

On a message or artifact, `metadata["a2a-rust.com/provenance"]` holds:

```json
{ "aiGenerated": true, "generator": "billing-agent/2.3",
  "signature": { "protected": "<b64url {alg, kid}>", "signature": "<b64url>" } }
```

**Signature.** A JWS (RFC 7515) with a detached payload: the RFC 8785
canonical JSON of the whole message or artifact, with only `signature`
removed. It therefore covers the marker. ES256 and EdDSA (RFC 8037) are
supported. The key is found by `kid` in a JWK Set the verifier trusts.

**Open questions.**
- Canonicalising the whole object means a peer that adds fields the verifier
  does not model breaks verification after the verifier round-trips it.
  Should the spec define signing over the received bytes instead?
- Should agent cards publish the JWK Set location for content keys, beside
  the card signature's?

## Reference implementation

- **Approval:** `crates/a2a-protocol-types/src/approval.rs` and
  `crates/a2a-protocol-server/src/approval.rs`.
- **Provenance:** `crates/a2a-protocol-types/src/provenance.rs`.
- **Tests:** each module's `tests.rs`, and
  `crates/a2a-protocol-server/tests/content_provenance_e2e.rs`.
