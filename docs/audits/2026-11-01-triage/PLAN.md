<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Plan: one pull request, built as commit groups

Triage of 2026-11-01 against `main` at `4a8243c`. **The maintainer chose one large pull request**
(2026-11-01), which replaces the handoff's earlier "one pull request per theme". Each group below is a
run of commits in that one pull request, so a reviewer can still read it theme by theme. The pull
request is opened when the groups the maintainer wants in 0.16.0 pass the local gates below; a pushed
branch alone starts no CI (the workflows run on `pull_request` and on pushes to `main`).

The items are the 191 that need work: 139 audit rows (`confirmed`, `fixed (part)`, `needs decision`,
`cannot check`) and the 52 distinct new findings of section N of `../2026-10-29/README.md`.
`plan_map.py`-style check, run when this file was written: every one of the 190 is in exactly one group
and none is in two. Rows already `fixed` or `not a defect` (12) need no group.

(51 of those 52 were found by the triage agents and one by the session.)

**Order** is severity first, then what depends on what, then size. Group A to D come first because
they fix defects in what pull request #185 merged, two of them reproduced live. N (the breaking window)
is last among the code groups on purpose: it touches the most files, so it goes in after the others
have settled. It must still land before 0.16.0 is tagged, because afterwards each break costs a
minor version.

**Bar for every group, before its commits are pushed** (the handoff's, from #184, plus what #185's
mutants taught): the regression test is written first and seen to fail on the unfixed code;
`scripts/preflight.sh`; every script gate; `cargo hack clippy ... --each-feature`; the PostgreSQL
suites; and `cargo mutants --in-diff` on the group's own diff for each crate it touches, with zero
survivors. Live runs are listed per group and are in addition to, not instead of, those.

| Order | Group | Theme | Items | Decision | Size |
|---|---|---|---|---|---|
| 1 | A (wave 1) | Credentials follow the host the caller named | 7 | none | M |
| 2 | B (wave 1) | Claim liveness: the reaper never ends live work | 9 | none (E-F13 is group B2) | L |
| 3 | C (wave 1) | A panicking, cancelled or silent executor always leaves the task and its watchers told | 10 | none | L |
| 4 | D (wave 1) | gRPC deadlines, card selection and trust roots; one TLS identity everywhere | 12 | F7 (pool) for H2's pooling half | L |
| 5 | G (wave 2) | First run works: deploy-agent, manifest, image, README | 18 | F8 (publish an image) | M |
| 6 | H (wave 2) | Documentation drift, and the READMEs that contradict the code | 18 | none | S |
| 7 | E (wave 2) | Proxies, DNS and the network | 12 | F2 (loopback and NO_PROXY) | M |
| 8 | F (wave 2) | WebSocket: bounded writes and liveness on both ends | 9 | F5 (stream keep-alive shared with H4) | M |
| 9 | I (wave 3) | OpenTelemetry in the SDK: logs with trace ids, self-metrics, strict-but-spec-correct env parsing | 12 | F3 (warn and fall back) | L |
| 10 | J (wave 3) | The observability stack: exemplars, alerts that cannot misfire, promtool in CI | 8 | none | M |
| 11 | K (wave 4) | Push delivery: semantics, pinning and fairness | 11 | none for the additive parts; an outbox is H14 | L |
| 12 | B2 (wave 4) | Production profile on a shared store; per-replica limits | 2 | F4 | M |
| 13 | L (wave 4) | Fleet correctness: clocks, caps, audit chain, polling | 14 | E-F18 and E-F25 options in DECISIONS.md | L |
| 14 | M (wave 5) | Delegation: hand-off on shutdown, journal with lease and budget, one options builder | 8 | F5 | L |
| 15 | O (wave 5) | Client resilience: retry budget, jitter, in-flight cap | 6 | F7 | M |
| 16 | R (wave 5) | Swarm control plane (an ADR each; the first four are fixes, not designs) | 15 | F12 | XL |
| 17 | N (wave 6) | The 0.16.0 breaking window | 15 | F1 | L |
| 18 | P (wave 6) | Release policy and the changelog | 2 | F6 F11 | S |
| 19 | Q (wave 6) | Examples that teach the right thing | 3 | none | M |

<!-- status-0.16-start -->

## Status at 0.16.0 (2026-11-05)

The 69 items that were still open when the final 0.16.0 work began (the maintainer's instruction: all of them go into 0.16.0)
now have a final status in `../2026-10-29/README.md`: its `Status` and `Note` for each, and one row per item in section O with
the commit. Rows that earlier work fixed are recorded in section O and in the table dated 2026-11-02 below, which is history and
not touched. The table here reads the register, so it cannot disagree with it. `closed` is `fixed`, `refuted` or `not a defect`;
an item that is `fixed (part)` has its remainder in the register's note (and, for a deliberate limit, in CHANGELOG.md's
`### Known Limitations`). Nothing here is released: there is no tag, and the pull request's CI is what decides.

| Group | Of the 69 | Closed | Still partial or open |
|---|---|---|---|
| B | 2 | 2 |  |
| D | 2 | 1 | B-29 (fixed (part)) |
| G | 13 | 10 | A15 (fixed (part)), A26 (fixed (part)), NEW-a-l-4a (fixed (part)) |
| H | 1 | 1 |  |
| E | 8 | 7 | B-28 (fixed (part)) |
| F | 2 | 2 |  |
| I | 10 | 10 |  |
| J | 8 | 7 | C-05 (fixed (part)) |
| K | 3 | 2 | SEC-14 (fixed (part)) |
| M | 2 | 1 | D-21 (fixed (part)) |
| O | 1 | 1 |  |
| R | 12 | 9 | H5 (fixed (part)), H13 (fixed (part)), H19 (fixed (part)) |
| N | 5 | 1 | G-09 (needs decision), G-12 (confirmed (read)), G-15 (needs decision), G-16 (confirmed (read)) |

54 of the 69 are closed. `scripts/audit/plan_status.py` counts 187 of the 191 plan items with at least one commit.

<!-- status-0.16-end -->

## Status when the pull request was opened (2026-11-02)

96 of the 191 items have at least one commit on the branch; 95 do not. (Counted by
`scripts/audit/plan_status.py`: an item is counted when its ID is named in section O of
`../2026-10-29/README.md`, which is written from the commit messages, and not when section O names it only
to say it is not done. Some of the 96 are done only in part, as the last column says. The first count, 97
and 94, included `NEW-g-docs-8`, which section O mentions only to say it is not done; the pull request's
title still says 97.) The bar above was **not**
fully met: the PostgreSQL suites, the per-feature clippy matrix and every script gate ran on the tip, but
`cargo mutants --in-diff`, `cargo-semver-checks`, the official TCK and the live matrix did not, and group
J's proof needs `promtool`, which could not be run here. The pull request was opened at the maintainer's
request because the session had grown too long to continue in; CI runs the rest.

| Group | Status | Commits | Left open |
|---|---|---|---|
| A | done | `a3a5dde` `78fcd2c` | |
| B | part | `c902c46` | NEW-h-s0-6, NEW-h-s0-8; H3 in part |
| C | part | `aa6e4ff` `6355a0c5` | E-F8 documented only (fork F13); D-03 (d) (fork F14) |
| D | part | `6d746dd` | B-14 (fork F15), B-29 (a decision), the rest of B-18, H2's pooling (fork F7) |
| G | part | `636eaf0` | A14, A15, A19, A21, A22, A23, A24, A25, A26, NEW-a-l-2, NEW-a-l-4a, NEW-a-l-5, NEW-a-l-6 |
| H | part | `065f0a0` | NEW-g-docs-8 |
| E | part | `4b6b382` | NEW-b-1 and NEW-b-5 (fork F16), NEW-b-3, NEW-b-4, B-08, B-09, B-26, B-28 |
| F | part | `e2712ec` `f532f245` `3ce8d69a` | B-10 in part (gRPC only), B-11 in part (the book page is done; the client folding a blocking send into a stream, option 2, is the maintainer's call), D-20, B-25 |
| I | part | `4fd80d0` | C-06, C-09, C-10, C-12, C-13, C-16, C-22, C-23, NEW-c-3, NEW-c-4 |
| J | **not started, blocked** | | all 8: the proof is `promtool test rules` and a live stack, and running `promtool` here was refused |
| K | part | `2b83f88` | SEC-14, NEW-f-2, NEW-d-3; D-05 (the id, the book and the dead-webhook policy; the outbox is ADR 0037, not built), D-13 and SEC-13 (direct connections only; fork F16), SEC-17 and NEW-f-4 (documented only) |
| B2 | done | `9c69ef6` `10333c48` | |
| L | part | `93dbb05` `af747a4` `7bdf55b2` `f9a4b347` `7e3a995d` `30e017be` `459b9d23` `6cc38533` `021408e7` `` `7efcdee1` `e9799881` `4d4ad27e` `673c2b18` | D-19, E-F14, E-F16, E-F17, E-F18, E-F19, E-F21, E-F22, E-F23, E-F24, E-F26 done; E-F15 documented only; E-F25 in part ((a) the back-off; (b) `LISTEN`/`NOTIFY` and (c) a read pool are not done); D-18 in part (a task waiting for a person is not evicted; an abandoned one never expires, the maintainer's call) |
| M | part | `61b5555b` `ae5d774e` `9bf8a90e` `998773c7` | D-21, NEW-h-s0-4 (neither is claimed; NEW-h-s0-4 has an earlier CHANGELOG entry that was not checked against this row). The live proof is done and found three defects, all fixed (`998773c7` and the book); see `docs/audits/2026-11-01-triage/evidence/m-live-proof/` |
| O | part | `0a8038cb` `f532f245` `3ce8d69a` | H12; D-12's per-origin in-flight cap and circuit breaker (ADR 0032 decision 5) and the card resolver's sharing of the pool (decision 6); NEW-h-designs-1 was done under R |
| R | part | `a8bf45e` | the four fixes are done; open: NEW-h-designs-3, NEW-h-designs-6, H5, H7, H8, H9, H10, H11, H13, H14, H17, H19 |
| N | part | `41756f1` `7b36d51` `4ec89f3` `b9c1304` `70fdf52c` `ba01ce60` | G-09, G-12, G-14, G-15, G-16; G-23 in part |
| P | done | `0b06ec2` `d75efc9` | |
| Q | done | `c4c5c29` | |

(Superseded by the status above; kept as written on 2026-11-02.) **Build next, in this order:** the rest of I and G (observability and deploy), then J (it needs `promtool`,
which the maintainer has allowed). The client pull request that carried `HttpPool`, HTTP/2 PING and a
connection lifetime for the HTTP bindings, B-11 and the live proof of M is the pull request that carries this
note; what it leaves is in the rows above. Rows that wait on the maintainer are in `DECISIONS.md` (forks F14 to F16, and the choice of
whether an abandoned parked task should ever expire, D-18).

### A. Credentials follow the host the caller named

- **Items (7):** SEC-08, SEC-09, NEW-f-1, NEW-f-5, NEW-f-6, SEC-18, G/D-05
- **Decision first:** none
- **Proof it needs:** e2e test with the two-interface card I ran live (NOPE binding, ftp:// scheme) fails before the fix; token-provider test that a card's `tokenUrl` on an unexpected host is refused; `--header` from env/file; constant-time compare test; mutants on the diff
- **Breaking:** behaviour (CLI refuses a card that steers a header) · **size:** M

### B. Claim liveness: the reaper never ends live work

- **Items (9):** H3, E-F7, E-F11, NEW-e-1, NEW-h-s0-1, NEW-h-s0-5, NEW-h-s0-6, NEW-h-s0-8, NEW-h-s0-9
- **Decision first:** none (E-F13 is group B2)
- **Proof it needs:** fault-injecting store: renewals fail for 2 TTL while the executor runs, the task must not be ended and the executor must stop itself; live: `scripts/live/partition_live.py` with toxiproxy between a replica and PostgreSQL; `EXPLAIN` of the reaper query on the tenant-aware table; `lock_timeout` on startup DDL
- **Breaking:** behaviour (an owner that cannot renew stops its executor) · **size:** L

### C. A panicking, cancelled or silent executor always leaves the task and its watchers told

- **Items (10):** NEW-d-1, D-03, E-F9, NEW-e-2, E-F8, E-F20, G-21, NEW-g-api-3, D-16, D-23
- **Decision first:** none
- **Proof it needs:** deterministic `current_thread` test that an executor panic stores Failed (NEW-d-1 settled by running); stream gets a terminal frame; cross-replica cancel pushes CANCELED and appends to the log; event positions are not reused after takeover (two-process test)
- **Breaking:** wire (500 to 503 for a store outage during a continuation) · **size:** L

### D. gRPC deadlines, card selection and trust roots; one TLS identity everywhere

- **Items (12):** NEW-g-api-1, G-20, G-04, NEW-g-api-2, B-20, B-19, B-03, B-15, B-18, B-29, B-14, H2
- **Decision first:** F7 (pool) for H2's pooling half
- **Proof it needs:** live: a 40 s executor over gRPC returns, not cut at 30 s (fails before); private-CA gRPC via `SSL_CERT_FILE`; token provider against a private-CA IdP with a client certificate; CLI `--cacert/--cert/--key` e2e
- **Breaking:** additive; `build_grpc` honours `without_tls` and the card (behaviour) · **size:** L

### G. First run works: deploy-agent, manifest, image, README

- **Items (18):** A13, A14, A15, A19, A21, A22, A23, A24, A25, A26, A27, NEW-a-l-1, NEW-a-l-2, NEW-a-l-3, NEW-a-l-4a, NEW-a-l-4b, NEW-a-l-5, NEW-a-l-6
- **Decision first:** F8 (publish an image)
- **Proof it needs:** follow the README line by line on a clean machine, live (Docker; kind is not available here); manifest validated in CI (kubeconform); Dockerfile built in CI; deploy-agent purges terminal tasks on a timer with a test; `search_path=a2a,public` regression test
- **Breaking:** behaviour (deploy-agent retention, env names) · **size:** M

### H. Documentation drift, and the READMEs that contradict the code

- **Items (18):** G/D-02, G/D-03, G/D-04, G/D-06, G/D-07, G/D-08, G/D-09, G/D-10, G/D-11, G/C-03, NEW-g-docs-3, NEW-g-docs-5, NEW-g-docs-6, NEW-g-docs-7, NEW-g-docs-8, NEW-g-api-4, NEW-c-2, NEW-t-1
- **Decision first:** none
- **Proof it needs:** each edit is the exact old/new text the triage recorded; `check_book_code.sh` ratchet fixed to see indented fences; doctests; `STABILITY.md` section 1 recomputed
- **Breaking:** none (the `sign_agent_card` removal is PR-N) · **size:** S

### E. Proxies, DNS and the network

- **Items (12):** NEW-b-2, B-23, NEW-b-1, NEW-b-3, NEW-b-4, NEW-b-5, B-08, B-09, B-12, B-26, B-27, B-28
- **Decision first:** F2 (loopback and NO_PROXY)
- **Proof it needs:** the loopback probe I ran (LR-2) becomes a test; NO_PROXY dialect vectors; push behind a proxy-only egress with a resolver that answers only internal names; Squid in Docker for the CONNECT cases
- **Breaking:** behaviour (loopback exempt by default) · **size:** M

### F. WebSocket: bounded writes and liveness on both ends

- **Items (9):** D-07, D-08, D-09, D-20, NEW-d-2, B-10, B-11, B-25, C-17
- **Decision first:** F5 (stream keep-alive shared with H4)
- **Proof it needs:** a peer that stops reading must not pin the read loop or hang a call; half-open detection by ping with a bound; gRPC heartbeat decision
- **Breaking:** behaviour (defaults for WS/gRPC idle bounds) · **size:** M

### I. OpenTelemetry in the SDK: logs with trace ids, self-metrics, strict-but-spec-correct env parsing

- **Items (12):** C-06, C-08, C-09, C-10, C-12, C-13, C-16, C-22, C-23, NEW-c-3, NEW-c-4, NEW-c-5
- **Decision first:** F3 (warn and fall back)
- **Proof it needs:** export tests against the Collector image; log lines carry trace/span ids; `OTEL_*` bad values warn, never exit; span status for abandoned calls per semconv
- **Breaking:** behaviour (bad env values no longer exit 2) · **size:** L

### J. The observability stack: exemplars, alerts that cannot misfire, promtool in CI

- **Items (8):** C-05, C-14, C-15, C-18, C-19, C-21, NEW-c-1, L11
- **Decision first:** none
- **Proof it needs:** `promtool test rules` in CI (Prometheus image); bring up `deploy/observability` and read every dashboard panel as a screenshot; a caller-error alert case that fails on the old rule
- **Breaking:** none · **size:** M

### K. Push delivery: semantics, pinning and fairness

- **Items (11):** D-05, D-13, D-22, B-13, B-30, SEC-13, SEC-14, SEC-17, NEW-f-2, NEW-f-4, NEW-d-3
- **Decision first:** none for the additive parts; an outbox is H14
- **Proof it needs:** `A2A-Delivery-Id` header; https pinned to the validated address (rebinding resolver test); per-caller fairness test with one caller holding the gate
- **Breaking:** behaviour (https pinning may refuse previously delivered webhooks) · **size:** L

### B2. Production profile on a shared store; per-replica limits

- **Items (2):** E-F12, E-F13
- **Decision first:** F4
- **Proof it needs:** Production refuses a process-local halt/lease/push/audit store when the task store hands out claims, and accepts it with the opt-out; two-pod test of the lease renewal that lands on the other pod
- **Breaking:** behaviour (Production refuses more) · **size:** M

### L. Fleet correctness: clocks, caps, audit chain, polling

- **Items (14):** E-F14, E-F15, E-F16, E-F17, E-F26, E-F18, E-F19, E-F21, E-F22, E-F23, E-F24, E-F25, D-18, D-19
- **Decision first:** E-F18 and E-F25 options in DECISIONS.md
- **Proof it needs:** database-clock deadlines; audit append under an advisory lock; poll back-off; each with the two-process probe the triage wrote
- **Breaking:** none · **size:** L

### M. Delegation: hand-off on shutdown, journal with lease and budget, one options builder

- **Items (8):** H4, H18, D-21, NEW-h-s0-2, NEW-h-s0-3, NEW-h-s0-4, NEW-h-designs-2, NEW-h-designs-5
- **Decision first:** F5
- **Proof it needs:** kill -9 a supervisor with a PostgreSQL journal and run the recovery loop (50 children); graceful redeploy detaches; gRPC idle child survives; resume jitter
- **Breaking:** behaviour (shutdown detaches journaled children) · **size:** L

### O. Client resilience: retry budget, jitter, in-flight cap

- **Items (6):** H6, H12, D-10, D-11, D-12, NEW-h-designs-1
- **Decision first:** F7
- **Proof it needs:** jittered Retry-After; per-client budget; in-flight cap; live overload against `deploy-agent`
- **Breaking:** behaviour (jitter) · **size:** M

### R. Swarm control plane (an ADR each; the first four are fixes, not designs)

- **Items (15):** NEW-h-designs-4, NEW-h-designs-7, H16, NEW-h-designs-3, NEW-h-designs-6, H5, H7, H8, H9, H10, H11, H13, H14, H17, H19
- **Decision first:** F12
- **Proof it needs:** halt store tolerates an unknown scope; ready only when sends are admitted; rate-limiter overflow bucket; then one ADR and one PR per design
- **Breaking:** wire (new extensions, opt-in) · **size:** XL

### N. The 0.16.0 breaking window

- **Items (15):** G-06, G-07, G-08, G-09, G-10, G-12, G-13, G-14, G-15, G-16, G-17, G-18, G-19, G-22, G-23
- **Decision first:** F1
- **Proof it needs:** each break listed by hand in `[Unreleased]` Breaking Changes and the 0.15 to 0.16 guide; `sign_agent_card` removed; semver-checks run against the v0.15.0 tag (the last published release; 0.15.1 was never tagged), not the 0.16.0 base
- **Breaking:** API (that is the point) · **size:** L

### P. Release policy and the changelog

- **Items (2):** G/C-01, G/C-02
- **Decision first:** F6 F11
- **Proof it needs:** `check_release_tree.py` refuses a patch with an unlabelled behaviour change; highlights block present
- **Breaking:** none · **size:** S

### Q. Examples that teach the right thing

- **Items (3):** SEC-15, SEC-16, G-05
- **Decision first:** none
- **Proof it needs:** the 15 hand-rolled accept loops use `Server`; the `agent-team` health monitor refuses non-public targets; the mcp-bridge marks remote skill text as untrusted
- **Breaking:** none · **size:** M

## Beyond the rows

These are not audit rows. They come from the maintainer's brief and from what the triage showed was
missing.

| Track | What | Where it stands |
|---|---|---|
| Live matrix | A permanent, scripted version of the runs: proxy and no proxy, public CA, private CA, self-signed, mTLS, IPv6, Docker, replicas behind a balancer, store outage, disconnect and resume, OTLP to a real Collector, `live-model` against llama.cpp with Qwen3.5 and Gemma 4 | Environment stood up 2026-11-01 (`handoff.md`); two probes run (`live-results.md`); Kubernetes cannot run in this sandbox |
| Agent Skills | Decision F9 in `DECISIONS.md`. Verified: the standard is now an Agentic AI Foundation project under the Linux Foundation | Proposed for 0.17.0 as its own ADR |
| Messaging a running agent | Decision F10: an A2A extension delivering a message to a working task's inbox; the spec is silent, so peers without the extension keep today's refusal | Proposed after group B (it rides on the claim) |
| Fan-out | ADR 0033 (`DelegationGroup`) is written, not built | After group M |
| A2A v1.0.1 conformance | Official TCK (`--min-graded 88`) after any wire change; ledger re-pointed with `check_spec_ledger.py --fix-lines` | Run before the pull request is opened |
| EU AI Act, GDPR | `docs/compliance/control-map.md` rows must keep citing a test that proves them; re-run `check_compliance_map.py --check` and the cited tests after the groups that touch audit, logging or retention (G, L, B2) | Not started |

## Before 0.16.0 is tagged

1. Every row in `../2026-10-29/README.md` has a final status; none is `open`, `confirmed (read)` or `needs decision` unless the maintainer deferred it, with the reason in its note.
2. The pull request's CI is green on every job, and `main` after the merge is green (the `CI` run on `4a8243c` had not finished when this was written).
3. `RELEASING.md`'s checklist, `STABILITY.md` section 1 recomputed, and the 0.15 to 0.16 upgrade guide listing every break. No `**Cadence exception:**` line is needed if 0.16.0 is dated in November or later (0.15.0 is dated 2026-10-25; the check compares calendar months).
4. The live matrix has been run against the merged tree, not the branch.
