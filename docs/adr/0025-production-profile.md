<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0025: A production profile that refuses to start, not warns

**Date:** 2026-10-21
**Status:** Accepted
**Author:** Tom F.

---

## Context

A server built with the defaults starts with no authentication, no halt
shared across replicas, no leases on delegated work, plaintext on every
interface, and a listener for a binding its agent card may not declare. Each
default is deliberate: a first agent should run with one builder call, and
TLS is often terminated in front of the process. Two of them already warn at
startup (spec §7.1, plaintext beyond loopback, and spec §5.2, an undeclared
binding, both added 2026-10-20), but a warning in a log nobody reads protects
nobody. A deployment that other agents delegate to at scale wants the
opposite trade: refuse to start rather than run unsafe.

The maintainer decided on 2026-10-21 to add presets that refuse to start on
plaintext beyond loopback, an undeclared binding, no authenticating
interceptor, or delegation without leases and a halt store, with the
defaults unchanged.

## Decision

Two halves, because the handler and the listener are built separately and
neither sees the other's settings.

- **The handler: `Profile::Production`**, applied with
  `RequestHandlerBuilder::with_profile`. It sets nothing and makes `build()`
  refuse, naming the missing piece and the call that supplies it, unless:
  - an interceptor that authenticates is installed;
  - a halt store is set (`with_halt_store`; ADR 0019);
  - a lease store is set (`with_lease_store`; ADR 0020).

  The handler cannot tell whether its executor delegates or is delegated
  to, so the halt and lease stores are required unconditionally. Any store
  satisfies the check, in-memory ones included: which store fits is the
  deployment's call, and a single-process deployment may want in-memory.
- **Profiles combine.** The builder kept one profile until now; it keeps a
  list, so `Profile::Auditable` and `Profile::Production` together require
  both sets. The `profile` module is no longer behind the `audit` feature;
  `Profile::Auditable` still is.
- **The listener: `ServeConfig::production()`**, the defaults with
  `production: true`. `Server::serve_with_shutdown` then turns both startup
  warnings into refusals: it returns at once, having accepted nothing, with
  the reasons in the new `ServeReport::refused`, and logs them at error
  level. Both reasons are named when both apply.
  - **Plaintext beyond loopback** is refused unless
    `with_tls_terminated_in_front()` says TLS ends before this listener (an
    ingress, a load balancer, a mesh sidecar). That setting also silences
    the warning under the default config. A listener whose address cannot be
    read is refused unless it is TLS, since loopback cannot be checked.
  - **An undeclared binding** is refused when the dispatcher has a handler
    whose agent card is static. A dispatcher without a handler, or a card
    produced per request, is not checked: there is nothing to check
    against.

Alternatives not taken:

- *`RequestHandlerBuilder::production()` as a constructor*, the name in the
  2026-10-21 decision record. The builder already applies presets through
  `with_profile`, and a second mechanism for the same thing would have to
  say how the two combine.
- *`serve_with_shutdown` returning `Result`*. It would break every caller of
  the graceful server for a refusal only the production config can produce.
  A field on the `#[non_exhaustive]` report is additive.
- *Refusing in `Server::with_config`*. The binding is known only once the
  dispatcher is, and TLS can be set after the config.

## Consequences

- With neither preset, nothing changes: the defaults, the warnings and the
  `serve` helpers are as they were.
- Not covered, each stated where it is documented:
  - The gRPC and WebSocket dispatchers serve on their own listeners, not
    `Server`, so `ServeConfig::production()` does not reach them; they keep
    the §5.2 warning.
  - Provenance signing was not wired into the server when this was
    written. ADR 0027 wired it (2026-10-22), and the profile now also
    requires a content signer.
  - A deployment can still call `serve` or a default `ServeConfig`; the
    presets are opt-in.
- Tests that fail without the checks: `profile::production_tests` (each
  missing piece named), `profile::tests::profiles_combine`,
  `serve::graceful::production::tests` (the refusal rules), and
  `tests/production_listener.rs`, where a real `Server` bound to `0.0.0.0`,
  or serving JSON-RPC under a card declaring only HTTP+JSON, accepts
  nothing and reports why. With the checks stubbed out, 9 of the 18 tests
  fail; the other 9 assert what still serves or builds.
