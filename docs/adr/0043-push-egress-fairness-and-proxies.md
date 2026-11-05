<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0043: Push egress is shared fairly, bounded by the address it reaches, and decided by name behind a proxy

**Date:** 2026-11-04
**Status:** Accepted (0.16.0). The maintainer may veto in review.
**Author:** Tom F.

---

## Context

Since 2026-10-31 every webhook delivery passes a gate (`push::gate`) that
bounds how many run at once: `push_max_in_flight` (512) overall and
`push_max_in_flight_per_host` (64) per destination name and port (audit
N-SEC-6, SEC-14). The 2026-11-01 triage found four ways it, and the sender
behind it, still fall short:

* **No fairness between callers (NEW-f-2).** One caller with slow webhooks,
  eight ports on one tarpit, held all 512 overall permits; every other
  tenant's deliveries waited. And a delivery's wait for a permit counted
  against `push_delivery_timeout`, so they ended as `timeout` without a
  request being made, which also counted against their webhooks' health
  (D-05), suspending healthy receivers for another caller's tarpit.
* **The destination bound is a name (NEW-d-3).** `a1..aN.evil.example`, all
  pointing at one victim through a wildcard record, were N destinations: the
  victim got up to 512 connections, not 64.
* **The pre-flight lookup assumes direct egress (NEW-b-1).** The SSRF screen
  resolves the webhook's host in this process and fails the delivery when it
  cannot. In a network whose resolver answers only internal names and whose
  every external request goes through a proxy that resolves names itself,
  every external webhook fails.
* **`NO_PROXY` saw an address (NEW-b-4).** An `http://` delivery's URI was
  rewritten to the checked address before it reached the proxy connector, so
  `NO_PROXY=.partner.example` was matched against `203.0.113.5`, and the
  request went to the proxy as `POST http://203.0.113.5/...`.
* **The build without `tls-rustls` had no proxy (NEW-b-3).** Its client was a
  bare `HttpConnector` (tuned since B-30, but reading no proxy variable).

## Decisions

### 1. A share per partition (NEW-f-2)

A third bound, `push_max_in_flight_per_partition` (256, half the overall),
keyed by `TenantContext::partition()`: the tenant, and the caller where
callers are isolated. Permits are taken name, partition, overall, so a caller
over its share waits without holding an overall permit.

Options weighed:

* **A per-partition semaphore (chosen).** Twenty lines, the same mechanism as
  the per-name bound, and it bounds the worst case: one partition leaves at
  least half the gate to the rest. It is a share, not a reservation: a lone
  partition still runs 256 at once, so a single-tenant deployment that needs
  more raises both numbers.
* **Weighted fair queueing across partitions** (round robin over a queue per
  partition). Fairer under contention, but a scheduler with its own task and
  queue memory per partition to bound, for a gate whose waits are already
  rare at the defaults.
* **A per-caller quota on configs.** Bounds how much work a caller can create,
  not how much of the gate it holds; `max_push_configs_per_task` already caps
  one task.

### 2. The wait is not the webhook's time (NEW-f-2)

A delivery now carries an `Admission` (`push::gate::admission`): its clock
starts at `push_delivery_timeout` when it is queued, and starts again from
the whole of it when the gate lets it through. The wait is bounded by the
same length, and a delivery the gate never lets through ends as the new
outcome `throttled`, not `timeout`. The health watch ignores a delivery cut
off while it waited, since it says nothing about its webhook.

The cost: one delivery can now take up to twice `push_delivery_timeout` (more
if it waits again for an address, below). The event's budget
(`push_delivery_budget`) sets concurrency from the timeout, not this bound;
under a full gate the budget is exceeded rather than webhooks reported as
failing for time they never had.

### 3. The destination bound also holds the address (NEW-d-3)

`HttpPushSender` calls `admission::hold_addresses` with every address its
SSRF screen checked, and the gate counts the delivery against each (key
`ip:<address>`, port aside) under `push_max_in_flight_per_host`. The name
bound stays, in front of any lookup.

The address is known only after the delivery holds its partition and overall
permits. Waiting for an address while holding them would let one victim's
queue hold every overall permit, the starvation the name bound was ordered
to prevent; so a delivery whose address is full gives its partition and
overall permits back, waits for the address, then takes them again, and its
clock restarts when it is let through. Addresses are taken sorted and
without repeats, so two deliveries to overlapping sets cannot deadlock.

Options weighed: resolving before the gate (a lookup per queued delivery, the
lookup pool's 32 slots then shared by every waiting delivery); keying by
registrable domain (a public-suffix list to ship and keep current, and it
misses unrelated domains pointing at one victim). Neither is better.

A custom sender that resolves the name itself can call the same function;
one that does not is bounded by name only, as before.

### 4. The proxy is decided by the webhook's name (NEW-b-4)

The checked addresses are pinned in the client's resolver for every scheme,
as they already were for `https://` (D-13), and the URI keeps the name. The
connector matches the proxy rules, `NO_PROXY` included, against the name the
operator wrote them for. A direct connection dials only the pinned addresses
(the connector tries them in turn, one connect bound shared between them);
a proxied one goes to the proxy by name, as `https://` always did. The URI
rewrite and its per-attempt address rotation (B-13) go: the connector's own
rotation covers B-13 within each attempt.

### 5. Proxy-resolved names, by opt-in (NEW-b-1)

`HttpPushSender::with_proxy_resolved_names()`: when a proxy will carry the
request and this process cannot resolve the name (no answer, `NXDOMAIN`, the
lookup bound), the delivery goes to the proxy by name instead of failing. The
name is still screened by its form (`localhost`, `*.internal`, numeric IPv4,
...), and a name that does resolve here is still refused if any answer is
private.

Off by default, because the screen then rests on the proxy: a name that only
the proxy can resolve, say an internal one through split-horizon DNS, would
be delivered to. The proxy has to deny private ranges itself, which the
book's proxy section already asks for. The error a failed lookup gives names
the option when a proxy applied.

Options weighed: always skip the local lookup behind a proxy (silently moves
the SSRF screen to the proxy for every deployment that sets `HTTPS_PROXY`);
never (the current fault). The opt-in is the only one that is safe by default
and workable where it is needed.

### 6. One client in both builds (NEW-b-3)

The proxy rules, the resolver with pins, and the tuned TCP connector are
compiled without `tls-rustls`, and the plaintext build's push client reads
`HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY` and forwards through an `http://`
proxy, as the TLS build does for `http://` webhooks.

## Not built

* **A per-destination rate** (a token bucket on requests per second). The
  concurrency bounds cap how many connections a third party sees; a rate
  would cap how fast a fast receiver is served, and the events that cause
  deliveries are already behind the server's own request rate limit. A
  deployment that needs one can wrap the sender.
* **A webhook verification handshake** (the receiver proving it wants the
  deliveries before the first one). The A2A specification defines none, so
  no receiver would answer it; it belongs in the specification first.
* **A per-caller config quota** across tasks: as in 1, it bounds stored work,
  not the gate.

## Consequences

* `push_max_in_flight_per_partition` is a new `HandlerLimits` field and
  builder; `throttled` a new `push_outcome`.
* A deployment with one partition runs at most 256 deliveries at once instead
  of 512 until it raises the new bound.
* A delivery can take up to twice `push_delivery_timeout` end to end.
* An `http://` delivery with several addresses tries them within an attempt
  rather than one per attempt.
* The plaintext build follows the proxy variables, which it ignored.
