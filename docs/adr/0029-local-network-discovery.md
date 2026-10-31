<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0029: Agents on the local network are found with mDNS, in a crate of its own

**Date:** 2026-10-30
**Status:** Proposed. Built and tested (below); the crate is not published,
and whether it is, and under what name, is the maintainer's call.
**Author:** Tom F.

---

## Context

An agent is found by its card URL, and nothing else: the A2A specification's
discovery mechanisms (§8.2) are the well-known URI, a curated registry, and
direct configuration. Several agents on one LAN, or in one compose project,
had to be given each other's URLs by hand (audit 2026-10-29, B-24 and
swarm-scale item 15). The maintainer asked for mDNS discovery for local
agent swarms (2026-10-29).

The project has decided before to keep registries, scheduling and meshes out
of the protocol crates (`docs/handoff-history.md`, "What not to chase"): the
four crates are a protocol implementation, not a runtime.

## Decision

- **A fifth crate, `a2a-protocol-discovery`,** that depends on the client and
  types crates; no protocol crate depends on it. The decision above stands:
  this finds agents on one link, and is not a registry.
- **DNS-SD over multicast DNS** (RFC 6762, RFC 6763), service type
  `_a2a._tcp.local.`, one instance per agent, port in the SRV record, TXT
  `txtvers=1`, `path=` (the card's path, default
  `/.well-known/agent-card.json`) and `scheme=` (`http`/`https`).
  `_a2a._tcp` is not registered with IANA; registering it, or a name in its
  place, is a step for whoever publishes this.
- **`mdns-sd` 0.21.6, pinned exactly.** Pure Rust, no async runtime of its
  own (a thread), Apache-2.0 OR MIT. The wire handling is this crate's whole
  trust surface, so an upgrade is a reviewed change.
- **Announcements are claims, not facts.** mDNS has no authentication. A
  browse yields where a card claims to be; the card is fetched through the
  client's own card discovery, which verifies signatures when given keys.
  Card URLs leave out loopback and IPv6 link-local addresses.

## Consequences

- Local swarms find each other without configuration:
  `Announcer::announce(&Advertisement::new(name, port))` on the agent,
  `discover(window)` or `Browser` on the client.
- The push sender still refuses `.local` names and private addresses by
  default; a discovered agent gets webhooks only once the sender allows
  private URLs.
- One link only: no discovery across subnets, no catalogue, no capability
  search, no health. A Kubernetes cluster has DNS and Services already, and
  multicast rarely crosses its pod network.
- Not in the CLI yet: `a2a` is published and cannot depend on an unpublished
  crate. `cargo run -p a2a-protocol-discovery --example browse` lists agents.

## Evidence

- `tests/mdns_round_trip.rs`, over real multicast: an agent served and
  announced is found and its card fetched through the announced address; a
  withdrawal is seen as lost (1.9 s); the TXT record's path and scheme reach
  the browser (the test fails with path parsing removed).
- Live, 2026-10-30, two probe agents as separate processes: both found with
  their cards; the one stopped with `SIGTERM` gone from the next browse; an
  empty link reported as such. That run found an announcement carrying
  `127.0.0.1`, which is why loopback is left out of card URLs.
- Not run: discovery between containers on a Docker bridge, or across hosts.
