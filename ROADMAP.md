<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Roadmap

Current release: **0.7.0**. MSRV **1.93**.

## What this file is

Every item below is derived from something already committed to this
repository — a `#[deprecated]` attribute, a documented removal, a measured
gap, a workflow that does not yet do what it claims. Nothing here is an
invented milestone, and no dates are asserted, because sequencing and
priority are the maintainer's call, not a thing to infer from the tree.

Items are grouped by whether the work is *already decided* (0.8 removals that
the code and docs commit to), *decided but unscheduled*, or *open questions*
that need a maintainer decision before any work starts.

## 0.8 — removals this repository has already committed to

These are announced in the code and docs, so 0.8 is a breaking release
regardless of what else lands. Each is a `#[deprecated]` attribute or an
explicit "removal planned for 0.8" note today.

| Item | Where it is announced |
|---|---|
| Remove the `grpc-legacy-json` feature and the pre-0.7 JSON-tunnel gRPC service it serves | `crates/README.md`, `proto/README.md`, `book/src/building-agents/dispatchers.md`, `docs/adr/0009-protobuf-native-grpc.md` |
| Remove `with_event_queue_write_timeout` — a deprecated no-op; queue writes never block, and slow consumers get an explicit lag error | `crates/a2a-protocol-server/src/builder.rs`, `.../streaming/event_queue/manager.rs`, `book/src/reference/configuration.md`, `book/src/building-agents/handler.md` |
| Stop sending the legacy bare `a2a-notification-token` header; keep only the canonical `X-A2A-Notification-Token` | `CHANGELOG.md` (0.7.0 entry) |

Removing the legacy gRPC tunnel also deletes `dispatch/grpc/service.rs`,
which is the file whose thin test coverage prompted the 2026-07-31 defect
hunt. Worth sequencing so that effort is not spent twice.

## Verification debt

Work where the project's own gates do not yet measure what they claim to.
This is the category most worth clearing before any external review.

* **Land one complete mutation sweep.** No full sweep has ever finished. The
  only scheduled run (2026-07-27) lost two `a2a-server` shards to the
  120-minute job timeout while the summary job still reported success. A
  completeness gate and 12-way sharding landed 2026-07-31; the next run is
  the first that can produce a trustworthy number.
  See [`book/src/reference/mutation-history.md`](book/src/reference/mutation-history.md).
* **Record the first real mutation score**, then keep the ledger current.
  Until a row exists, the project has no mutation-adequacy history at all.
* **Raise coverage on the genuinely weak files.** After the 2026-07-31 pass,
  the weakest are `handler/event_processing/background/mod.rs` (54.2%),
  `serve.rs` (67.5%), and `background/push_delivery.rs` (72.8%). The first
  was on the previous shortlist and is still untouched; `serve.rs` was not
  on any list and should have been.
* **Decide whether `A2aRouter` should route `/tenants/{tenant}/…`.** The
  built-in REST dispatcher strips that prefix and threads the tenant
  through; the axum adapter registers no such routes. Verified to fail
  closed — such a request 404s rather than being served from the default
  partition — and pinned by a test, but the asymmetry between the two
  dispatchers is undocumented behaviour that a user will eventually hit.

## Release engineering and supply chain

* **Signed tags.** All ten release tags (`v0.2.0` … `v0.7.0`) are lightweight:
  no tagger, no date, no signature, despite `RELEASING.md` prescribing
  `git tag -a`. Adopting `git tag -s` needs a maintainer key and a documented
  way for adopters to obtain it — an unmade decision, not just a missing
  step. See [`RELEASING.md`](RELEASING.md).
* **PGP key for security reports.** `SECURITY.md` has none, so emailed
  vulnerability reports cannot be encrypted. GitHub Security Advisories is
  the recommended channel in the meantime.
* **Register `a2a-rust.dev`.** The domain is unregistered (NXDOMAIN), so both
  `conduct@a2a-rust.dev` and `security@a2a-rust.dev` are undeliverable. Both
  documents now point at the maintainer address instead; the dedicated
  addresses can be restored once the domain is live.

## Conformance

Measured against the official `a2a-tck` suite: **92 of 114 MUST requirements
passing, 0 failing.** The remaining 22 are not defects in this SDK — 21 have
no test function upstream, and `CARD-EXT-002` is structurally inapplicable.
Full analysis and reproduction steps in
[`docs/official-tck-findings.md`](docs/official-tck-findings.md).

* **Report `SSE-001` upstream.** A reproduction and analysis are prepared but
  unfiled, pending a maintainer decision on whether to open the issue.
* **Track the 13 open upstream backlog items** that would move requirements
  out of `NOT TESTED` if `a2a-tck` implements them. Nothing to do here except
  re-measure when upstream moves; the ceiling is not this project's to raise.
* **WebSocket** remains a custom binding under spec §12 and is deliberately
  outside the official suite's scope. It is covered by this repository's own
  feature-gated tests.

## Open questions

Genuinely undecided — listed so they are not mistaken for oversights.

* Whether to adopt signed tags at all, or to rely solely on the SLSA build
  provenance attestations already produced for release artifacts
  (see [`PROVENANCE.md`](PROVENANCE.md)).
* Whether `0.8` should also raise MSRV, and what support window to state.
* Whether the axum adapter should reach parity with the REST dispatcher on
  tenant routing, or whether the split is intentional and should simply be
  documented as such.

## Maintaining this file

Add an item when the repository commits to it — a deprecation, a documented
removal, a measured gap. Remove it when the work lands, and say where it
landed. Do not add speculative milestones: a roadmap that lists intentions
nobody has committed to is worse than no roadmap, because it cannot be
checked against anything.
