<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0046: Readiness stays a dependency check; load is a separate signal; saturation may drive readiness only when asked

**Date:** 2026-11-04
**Status:** Accepted (0.16.0; audit H7). Built: the `/load` route and the
`endpoint-load-metrics` header (opt-in, `RequestHandlerBuilder::with_load_reporting`),
opt-in saturation readiness with hysteresis
(`HandlerLimits::with_saturation_readiness`), `"halted"` in the `/ready` body,
and a warning from `ServeConfig::production` when `max_connections` is unset.
**Author:** Tom F.

---

## Context

`/ready` (`RequestHandler::readiness`) answers 503 while the replica is
draining, while its task store does not answer within `READINESS_TIMEOUT`, and,
since `a8bf45e` (audit NEW-h-designs-7), while a halt store it was given has
never been read. It says nothing about load:

* a replica holding every one of its 1024 queues still answers ready, and a
  balancer keeps sending it sends it will refuse `Overloaded`;
* a replica under a halt of everything answers ready and refuses every send
  `Halted`;
* no number a balancer could use is exported: `a2a.server.queue_depth` is,
  but the cap it is measured against is not;
* `ServeConfig::production` does not require or mention `max_connections`
  (default `None`).

The triage's options (DECISIONS.md, H7):

* **A.** `/ready` 503 above a high-water mark, with hysteresis. Balancers stop
  sending, but replicas saturate together, so readiness flaps across the
  fleet and takes capacity away when it is most needed; probe periods of
  1-10 s are too slow to balance with; Endpoints churn.
* **B.** A load signal that is not readiness: a response header, a JSON
  endpoint, a gauge.
* **C.** Status quo plus documentation.

For halts: a halted replica must stay reachable for reads and cancels
(`handler/halt.rs` module docs), so it stays ready, and says it is halted.

## Decision

B, with A available behind an explicit setting, never by default.

1. **`GET /load`** (HTTP+JSON dispatcher, the axum router, and the combined
   dispatcher), when `with_load_reporting()` is set: `200` with
   `{"running":…,"queued":…,"capacity":…,"utilization":…,"draining":…,"halted":…}`.
   `running` is the live event queues (one per running turn), `queued` the
   sends waiting in the admission gate (ADR 0045, 0 without one), `capacity`
   the gate's capacity when there is a gate, else the queue cap, else `null`
   (unbounded), and `utilization` `(running + queued) / capacity`, which goes
   past 1 when sends are queued, or `null` with no capacity. It touches no
   store: it is cheap enough to scrape every second.
2. **`endpoint-load-metrics`** on every response the crate's server sends
   (`serve`, `Server`: the HTTP+JSON, JSON-RPC and combined dispatchers), and
   on every response of the axum router, when `with_load_reporting()` is set.
   It is added where the response leaves, so a dispatcher called directly from
   an application's own hyper loop does not carry it:
   `TEXT application_utilization=<u>, named_metrics.running=<r>,
   named_metrics.queued=<q>`. This is the ORCA header a client-side weighted
   balancer reads from ordinary responses (Envoy's client-side weighted round
   robin; ADR 0032's endpoint resolver could read the same). The format is
   taken from Envoy's documentation of ORCA headers and was not exercised
   against Envoy here. gRPC responses do not carry it (ORCA over gRPC is a
   trailer with its own encoding; not built).
3. **Saturation readiness**, off by default:
   `HandlerLimits::with_saturation_readiness(unready_at, ready_at)`, in
   percent of capacity. `/ready` answers 503 `"saturated"` once utilization
   reaches `unready_at`, and stays so until it is at or below `ready_at`.
   The two-threshold band is the hysteresis; there is no probe counter,
   because the band already keeps one noisy probe from flapping the state.
   The build refuses `ready_at >= unready_at` and `unready_at` of 0.
4. **Halts are visible, not unready.** `/ready`'s body carries
   `"halted":true` while a halt of everything is held. A tenant's halt does
   not: the replica serves every other tenant.
5. **`max_connections`.** `ServeConfig::production` warns at startup when it
   is unset. A warning and not a refusal: the right number depends on the
   host, and ADR 0025's refusals are for settings with one right answer.

## Consequences

* Nothing changes unless `with_load_reporting` or saturation readiness is
  set, except the additive `halted` field and the production warning.
* The header costs a few atomic loads and one formatted header per response.
  The live queue count is kept in an atomic beside the manager's map, so
  neither `/load` nor the header takes the map's lock.
* Load is per replica and instantaneous. It is a hint for a balancer that
  can use one, not a schedule.

## Not built

* ORCA out-of-band reporting and gRPC trailers.
* A gauge for the queue cap in `OtelMetrics` (the metrics surface belongs to
  the observability work; `/load` carries the cap).
* A readiness that waits for a number of consecutive probes.
