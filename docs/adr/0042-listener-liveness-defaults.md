<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0042: The gRPC and WebSocket listeners probe their peers by default

**Date:** 2026-11-04
**Status:** Accepted (the maintainer's instruction of 2026-11-04: every open
plan item goes into 0.16.0 with the best engineering option, open to a veto in
review). Built in 0.16.0.
**Author:** Tom F.

---

## Context

Audit D-20: the HTTP listener ([`Server`]) bounds every connection by default
(a header-read timeout of 30 s and an idle timeout of 75 s; its connection
cap is opt-in), but the
gRPC and WebSocket listeners bound nothing unless the deployer opts in.
Measured on 2026-08-19: 400 TCP connections opened against `GrpcDispatcher`
and left silent were all accepted and all alive twelve seconds later; the
ceiling was the process's file-descriptor table. The same held for
`WebSocketDispatcher` before `with_idle_timeout` existed, and holds still with
its default.

Both knobs were left off on purpose, and the reasons are written in their
rustdoc:

* A WebSocket subscription may be silent for hours while it waits for an
  event, so a plain idle timeout would close healthy connections.
* HTTP/2 keep-alive "starts pinging the clients of every deployment that
  upgrades", and a client may be configured to object to PINGs.

Neither knob is a plain idle timeout. `WebSocketDispatcher::with_idle_timeout`
sends a Ping at half the budget, and every conformant WebSocket library
answers it without the application; `GrpcDispatcher::with_http2_keepalive`
sends HTTP/2 PINGs that the peer's HTTP/2 stack answers. Each closes a peer
that does not answer: one that went away without a close (a NAT that dropped
its mapping, a laptop that slept), or one that stopped reading. A quiet,
healthy subscription answers and stays open.

The objection to server PINGs is narrower than the rustdoc says. gRPC's
"too many pings" enforcement (`GOAWAY ENHANCE_YOUR_CALM`, grpc-go's
`keepalive.EnforcementPolicy`) is a server option that polices client PINGs;
this ADR found no client-side equivalent in grpc-go or tonic (read, not
tested against every client). An HTTP/2 stack answers a PING without the
application, which is what RFC 9113 §6.7 requires of it.

## Options

1. **Probe by default, as the HTTP listener bounds by default.** WebSocket:
   `idle_timeout = DEFAULT_WS_IDLE_TIMEOUT` (75 s; a Ping at 37.5 s). gRPC:
   HTTP/2 keep-alive every 60 s, closing a connection whose PING is not
   answered within 20 s. An opt-out on each (`without_idle_timeout`,
   `without_http2_keepalive`). The connection cap stays opt-in.
2. **Keep both off, warn at `serve` when unset,** and have the production
   recipe set them. Nothing changes for anyone who reads logs; a warning on
   every default start is noise, and ADR 0025 records why a warning nobody
   reads protects nobody.
3. **Keep both off.** The 2026-08-19 measurement stands.

A default connection cap was considered for option 1 and not taken: the right
number depends on memory per connection and the deployment's fan-in, a cap
too low refuses legitimate peers, and a cap holds the excess in the kernel's
backlog rather than closing anything. The book's production recipe sets it.

## Decision

Option 1. Both knobs close only a peer that does not answer a probe; that is
the cost the "off" rationale worries about least, and it is what a deployer
who measured the 400 silent connections would set.

* `WebSocketDispatcher::new` starts with `idle_timeout` at
  `DEFAULT_WS_IDLE_TIMEOUT`. `with_idle_timeout(d)` changes it;
  `without_idle_timeout()` turns it off.
* `GrpcDispatcher::new` starts with HTTP/2 keep-alive at
  `DEFAULT_GRPC_KEEPALIVE_INTERVAL` (60 s) and
  `DEFAULT_GRPC_KEEPALIVE_TIMEOUT` (20 s). `with_http2_keepalive(i, t)`
  changes it; `without_http2_keepalive()` turns it off. It applies to every
  `serve*` method, not to `into_service`, whose server the caller builds.
* `max_connections` and `max_connection_age` stay off by default.

## Consequences

* Behaviour change: a WebSocket peer that answers no Ping within 75 s of
  silence, and a gRPC peer that answers no PING within 20 s, is closed. Peers
  using any conformant library are unaffected.
* Every connected gRPC client receives one HTTP/2 PING per minute of
  idleness, and every idle WebSocket client one Ping per 37.5 s: a few dozen
  bytes.
* Behind an L7 load balancer that terminates HTTP/2, the PING reaches the
  balancer, not the client (B-10); this bounds the balancer's connection to
  the server, which is the one this listener holds.
* What is not built: a default connection cap; an application-level heartbeat
  on gRPC streams (B-10's option 3, a wire decision of its own).

[`Server`]: ../../crates/a2a-protocol-server/src/serve/mod.rs
