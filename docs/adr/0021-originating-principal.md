<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0021: The originating principal travels as an RFC 8693 delegated token

**Date:** 2026-10-19
**Status:** Accepted
**Author:** Tom F.

---

## Context

The control map's ASI03 row (OWASP Agentic Security, "Identity and
Privilege Abuse") was `partial`. A delegated call carried the delegating
agent's credential, not the identity of the person or system the work was
for. Each hop's audit record named only its immediate caller. The chain
could be joined afterwards through the trace id, but no call carried it.

## Options

1. **An unsigned claim in message metadata** ("on behalf of X"). Cheap,
   but any agent can write any name. It is useful for attribution and
   useless for authorization, and an executor that trusted it would turn
   a convenience into privilege escalation, the very abuse ASI03 names.
2. **An assertion the SDK signs and verifies itself.** A format, a trust
   model and key distribution of our own: a new security protocol. The
   maintainer's rules say to bring that to them rather than build it.
3. **RFC 8693 OAuth 2.0 Token Exchange.** The delegating agent exchanges
   the token it was called with for one the identity provider issues for
   the downstream agent. The new token's `sub` is still the originating
   principal, and its `act` claim names the agent acting for it (nested
   for each further hop, the most recent actor outermost, §4.1). The
   identity provider decides what may be delegated and to whom
   (`may_act`, its own policy). The downstream agent verifies the token as
   it verifies any JWT.

## Decision

Option 3. It is a standard, the identity provider stays the authority, and
the SDK adds no cryptography.

- **Server.** `JwtAuthInterceptor` reads the `act` chain of a token it has
  verified. The caller identity stays `jwt:<iss>:<sub>`, the originating
  principal. `CallContext::actors()` lists the actors, nearest first, each
  as `jwt:<iss>:<sub>`; an `act` without `iss` takes the token's issuer.
  The chain is capped at 16 hops. A token whose `act` is not an object with
  a string `sub` is refused, since a chain that cannot be read cannot be
  attributed. With the `audit` feature, a call record carries the chain in
  `detail.actors`, so each hop's record names both whom the work was for
  and who did it.
- **Client.** `token_exchange::exchange` performs the RFC 8693 request:
  `grant_type=urn:ietf:params:oauth:grant-type:token-exchange`, the
  subject token, an optional actor token, audience, resource and scopes,
  authenticated as `OAuth2ClientCredentials` authenticates. It returns the
  issued token and its type and lifetime. The exchanged token belongs to
  one principal, so it is the caller's to attach to the delegated call
  (`StaticTokenProvider`). A client-wide provider would mix principals.

## Consequences

- With an identity provider that supports token exchange, the originating
  principal reaches every hop verified, and every hop's audit record names
  it and the chain of agents that acted.
- Without one, nothing changes: the gap stays as the control map
  describes, now with the standard route named. Option 2 is not built.
  Building it is a protocol decision for the maintainer.
- Scope attenuation, the second half of "not escalated", is the identity
  provider's: it sets the exchanged token's scopes and audience. The SDK
  enforces `require_scope` on the result as on any token.
