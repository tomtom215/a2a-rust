<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0031: Agents are found across networks through sources behind one interface

**Date:** 2026-10-31
**Status:** Proposed. Nothing here is built or run. The maintainer approved
all four sources on 2026-10-31 ("I like every one of those ideas"); the
placement and the dependency choices below are this ADR's, open to change.
**Author:** Tom F.

---

## Context

ADR 0029 finds agents on one link with mDNS and says why it stops there:
multicast is link-local, and as far as is generally documented it is not
routed between Kubernetes pods or across cloud subnets (not tested here, no
cluster). The A2A specification gives three ways to find an agent (§8.2): the
well-known URI, a curated registry, and direct configuration. This repository
has decided to keep registries out of the protocol crates
(`docs/handoff-history.md`, "What not to chase"), and ADR 0029 repeats it.

So for a cluster, a VPC or several of them, the SDK today offers a card URL
and `resolve_agent_card`, and nothing that produces the URLs.

## Decision

All in `a2a-protocol-discovery`; no protocol crate depends on it.

1. **`AgentSource`**: an async interface that yields `DiscoveredAgent` (the
   type ADR 0029 already has), and for a source that can watch, a stream of
   added and removed agents. **`Merged`** combines sources: union, de-duplicated
   by the canonical card URL, a per-source timeout so one slow source does not
   hold the rest, and a TTL cache with jitter (audit H8: a fleet cold start
   must not fetch every card at once).
2. **`StaticSource`**: a list of agent base URLs from an environment variable or
   a file. Fetches each card. This is the source GitOps and Helm-managed fleets
   need, where the topology is already configuration.
3. **`DnsSdSource`**: DNS-SD over ordinary DNS (RFC 6763 §4–§7, unicast): browse
   `_a2a._tcp.<domain>` (PTR), then SRV, TXT, A and AAAA for each instance. The
   TXT keys are the ones ADR 0029 defines (`txtvers`, `path`, `scheme`). Works
   wherever DNS works: a managed private zone, Consul's DNS interface,
   CoreDNS, peered VPCs.
4. **Kubernetes DNS SRV**, as `DnsSdSource` with a convention: a Service port
   named `a2a` or `a2a-https` gets an SRV record, one per ready pod for a
   headless Service. Kubernetes DNS serves no TXT for these, so the card path
   is the well-known one and the scheme comes from the port name. (Documented
   behaviour of Kubernetes DNS, not verified here.)
5. **`KubernetesSource`**, behind a cargo feature `kubernetes`, off by default:
   list and watch `EndpointSlice`s by label, with the card path and scheme in
   annotations. It gives add and remove events and metadata DNS cannot carry.
   It needs RBAC (`get`, `list`, `watch` on `endpointslices`).

**Dependencies, pinned exactly like `mdns-sd` (the wire handling is the trust
surface).** `hickory-resolver` 0.26.3 for 3 and 4. `kube` 4.2.0 with
`k8s-openapi` 0.29.0 for 5, optional. `kube`, not a hand-written watch, because
the hard part is the watch protocol: resource versions, bookmarks, a `410 Gone`
that forces a relist, reconnects. Those are what a hand-rolled version gets
wrong without a cluster to show it. Versions are those `cargo search` reported
on 2026-10-31; their APIs have changed between releases, so read the current
docs at implementation time. Licences are to be checked against `deny.toml`.

### Security: every source

A discovered address is a claim, not proof (ADR 0029 says it for mDNS; DNS and
the Kubernetes API are no different to a card fetch). Each source takes:

* **A signed-card requirement**: `CardFetchOptions::with_signature_keys`.
* **A host allow-list** by suffix. `DnsSdSource` and `KubernetesSource` refuse
  to run without one or an explicit `allow_any_host()`: a DNS answer can name
  anything, including a metadata address. `StaticSource` is operator-written
  and defaults open.
* **Caps**: a size cap on the card body, a time cap on each fetch, a maximum
  number of agents per source (a hostile answer with ten thousand PTRs), and
  clamped TTLs. No redirect to another host. Credentials the caller configured
  for its own services are not sent to a host a discovery answer named (the
  rule the CLI got for `--header`, audit SEC-09).
* The existing mTLS and private-CA support covers the transport.

## Alternatives

| Option | Why not |
|---|---|
| A registry service in the crates | Decided against in ADR 0029 and `handoff-history.md`. A deployment that wants a catalogue runs one; a reference directory can be an example. |
| Cloud Map, Consul or etcd SDKs as sources | Their DNS interfaces are covered by option 3; a native SDK per vendor is a dependency each. |
| Only the Kubernetes API source | Needs RBAC and a heavy client for what DNS already gives; and does not help outside Kubernetes. |
| A hand-written watch instead of `kube` | See above. |

## Consequences

* The same `DiscoveredAgent` comes from every source, so code written against
  mDNS discovery works unchanged.
* `a2a` still cannot depend on the unpublished crate (ADR 0029); a CLI `discover`
  waits for publication.
* The crate stays `publish = false` until the maintainer registers it.

## What would settle it (nothing is run)

* DNS: an in-process authoritative server (`hickory-server`) on loopback over
  UDP and TCP, with a TXT-less answer, truncation and the TCP retry, `NXDOMAIN`,
  a CNAME loop, and an oversized answer set hitting the cap.
* Kubernetes: a fake API server answering list and watch, with bookmarks and a
  `410 Gone` that must relist.
* Not testable here: real CoreDNS, Route 53 or Cloud DNS, a cluster's RBAC and
  DNS behaviour. An optional CI job on `kind` would be the first real check;
  until then each claim about a managed service is documentation, not
  verification.

## Open

* The Service port-name convention (`a2a` / `a2a-https`) is this ADR's invention
  for DNS-only clusters.
* Whether `_a2a._tcp` is registered with IANA (ADR 0029 left it open).
* Whether the Kubernetes source belongs in this crate or in one of its own.
