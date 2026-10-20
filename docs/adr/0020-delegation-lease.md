<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0020: A lease stops the children of a parent that went quiet

**Date:** 2026-10-18
**Status:** Accepted
**Author:** Tom F.

---

## Context

`a2a_protocol_client::delegation` cancels a child task when its parent
cancels, loses the child's stream, or drops the handle. It cannot cover a
parent that crashes, hangs, or is partitioned away: the handle dies with the
parent. `docs/swarm-orchestration.md` called this G1-B and recommended a
lease. The control map's ASI08 row (OWASP Agentic Security, "Cascading
Failures") was `partial` for this one gap: "a parent that crashes leaves its
children running".

A2A v1.0.1 has no lease, and `GetTask` carries no metadata. Any lease is
therefore an extension in the style of the idempotency, failure-class and
approval extensions.

## Options

1. **Renew by holding a stream.** A child stays alive while some
   subscription to it is open. That needs no new call, but the
   subscription can land on a different replica from the executor, and an
   SSE disconnect is detected only on the next write. Neither gives a
   bound.
2. **Renew with an explicit call, the deadline held where the executor
   runs.** Simple, but behind a load balancer a renewal landing on another
   replica never reaches the executor's. That would stop a child whose
   parent is alive, which is worse than no lease.
3. **Renew with an explicit call, the deadline held in a shared store.**
   Any replica records a renewal, and the executor's replica reads the
   deadline. It costs a store write per renewal and a store read per check.

## Decision

Option 3.

- **Grant.** A message carries `{"ttlMs": n}` in `Message.metadata` under
  `a2a-rust.com/lease`, and `https://a2a-rust.com/extensions/lease/v1` in
  `Message.extensions` (`a2a_protocol_types::lease::Lease`). A server with
  a `LeaseStore` (`RequestHandlerBuilder::with_lease_store`) declares the
  extension on its card. It refuses a grant outside 1 s
  (`MIN_LEASE_TTL`) to `with_max_lease_ttl` (default one hour) before any
  work, refuses the send if the grant cannot be stored, and stores the time
  to live and a deadline per tenant and task.
- **Renewal.** A `GetTask` of the task whose request activates the
  extension (`A2A-Extensions`) moves the deadline to now plus the time to
  live, after the read succeeds, so only a caller allowed to read the task
  can renew it. A plain read renews nothing, so a dashboard polling tasks
  cannot keep an orphan alive. `A2aClient::renew_lease` sends that request,
  keeping any extensions the client already asks for.
- **Lapse.** The replica running the turn checks the deadline every quarter
  of the time to live, at most once a second. Past the deadline it stops
  the turn as a halt does, and the task ends `canceled`. A turn that ends
  terminal releases the lease. One that ends parked keeps it for the turn
  that continues the task, whose grant replaces it.
- **Store outage.** A store that cannot be read counts as a live parent. A
  database blip must not cancel every leased child. The child then runs
  until the store answers or its own executor timeout ends it.
- **Client.** `Delegation::start_leased` attaches the grant and renews
  every third of the time to live until the handle settles, is cancelled,
  detached or dropped.
- **Stores.** `InMemoryLeaseStore` for one process, `PostgresLeaseStore`
  for replicas (`a2a_leases`, keyed by tenant and task).

## Consequences

- A child of a parent that went quiet stops within about one time to live
  plus one check. `delegation_lease_tests.rs` measures that against a real
  server, with a parent that detaches as a crashed one would, and checks
  that a live parent keeps its child running past three times the TTL.
- Over WebSocket, `A2A-Extensions` is sent only with the upgrade.
  `renew_lease` renews only on a connection that asked for the extension.
  The client docs say so.
- What stays open on ASI08: a peer that does not implement the extension
  ignores the grant, and its children run as before. Leases are opt-in on
  both sides. G5 (deadline and budget propagation) is not built.
- Tests: the store contract (in-memory and PostgreSQL), ten handler tests
  (lapse, renewal on two replicas, a plain read not renewing, bounds, a
  malformed grant, no store, an unreadable store, a refused grant,
  release, the card), three client end-to-end tests, and the types
  round-trip.
