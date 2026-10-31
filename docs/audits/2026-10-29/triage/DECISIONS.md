<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Decisions the triage needs from the maintainer

Collected 2026-10-31 from the nine triage reports and the lead's own work, against `main` at `a0e6673`. Each line is one decision, with the rows it closes and the report that holds the options, their trade-offs and what would prove the choice. **A recommendation here is the triage's, not a decision**; nothing below has been built on one. "0.16.0 is unreleased" means a breaking change costs nothing now and a 0.17 later.

Where the reports disagree, this file says so rather than picking.

## 0. Answered by the maintainer, 2026-10-31

| ID | Answer |
|---|---|
| X-4 | **A branch per theme, and open the pull requests.** The maintainer's concern: every pull request takes 60 to 90 minutes of CI, stacking them multiplies that by rebases and updates, and CI has not passed first time for a long while (mutants above all). So: each branch starts from `main`, not from another branch; branches are kept to disjoint files so they merge in any order; and the local gates, including `cargo mutants --in-diff` for every crate a diff touches, run before the first push. |
| X-1 | **Take all four** (`#[non_exhaustive]` on `AgentCard`, `AgentSkill`, `AgentInterface`, `AgentProvider`; `tls-rustls` in the server's defaults; `ContextId` on `RequestContext`; a quieter `Telemetry::init()`), **and look for other breaking changes worth taking while they are free**, for production at scale and the best developer experience. The candidates found are in PLAN.md, pull request 10. |
| X-2 | **Resume then cancel, with `Delegation::reattach`, and also the durable journal now** (ADR first; the ADR says where the database-backed implementations live). |
| X-6 | **Agent Skills before 0.16.0 is tagged, with tool-loop activation.** The maintainer cites Anthropic's announcement of the donation to the Agentic AI Foundation (<https://www.anthropic.com/news/donating-agent-skills-to-the-agentic-ai-foundation>); what that page says is recorded in `S1-agent-skills.md` once fetched. |
| X-3, X-5, X-7 | Not asked. Taken as recommended, and reversible: X-3 (an opt-in `HttpPool`, additive) when the client pull request needs it; X-5 (the release files say 0.15.0 is the newest and 0.16.0 is in preparation) in the docs pull request; X-7 (stay on A2A v1.0.1; `v1.1.0-rc` is a tag, not a release). |

## 1. Decisions that change what the next pull requests contain

| ID | Rows | Question | Recommendation | Detail |
|---|---|---|---|---|
| X-1 | G-10, G-06b, G-18, G-14.2, D-20, D-10a | Which breaking changes go into 0.16.0, while they are free? (`#[non_exhaustive]` on `AgentCard`, `AgentSkill`, `AgentInterface`, `AgentProvider`; `tls-rustls` in the server's default features; `RequestContext.context_id` as `ContextId`; `Telemetry::init()` exporting only where configured; liveness defaults on WebSocket and gRPC; `Retry-After` beyond `max_backoff` returned to the caller) | Take the first four and the WebSocket default; hold the gRPC default (see the conflict below). Each needs an upgrade-guide line | GC.md D-GC-1, -2, -6, -8; D.md D-10, D-20; B.md D-B1 |
| X-2 | H4, H18 | Delegation on a stream loss: resume, then cancel | Resume from `last_event_id` under a bounded policy before cancelling, plus `Delegation::reattach`; the journal (G2) as a later ADR | S0.md |
| X-3 | H2 (pool half), H6, H8, H12 | A shared `HttpPool` handed to `ClientBuilder` | Opt-in `HttpPool`, default unchanged; it unblocks H6's breaker state, H8 and H12 | S0.md |
| X-4 | none (process) | Delivery shape: the session was told to push only to `claude/jolly-wozniak-zc1vbe`, while the plan is one pull request per theme, which needs one branch per theme | Ask which: commits per theme on the designated branch, or one pushed branch per theme (named `claude/jolly-wozniak-zc1vbe-<theme>`: git cannot create `claude/jolly-wozniak-zc1vbe/<theme>` while the branch `claude/jolly-wozniak-zc1vbe` exists) and pull requests only when asked | — |
| X-5 | N-GD-2, G/C-02 | `CITATION.cff` says version 0.15.1, released 2026-10-26; `ROADMAP.md` says 0.15.1 is prepared; `STABILITY.md` says 0.15.0 is the newest; crates.io has 0.15.0 as its newest (checked 2026-10-31). What do the files say until 0.16.0 is tagged? | Say 0.15.0 is the newest release and 0.16.0 is in preparation; set `CITATION.cff` at the tag, not before | GD.md |
| X-6 | none | Agent Skills (agentskills.io): adopt? | See section 3 | below |
| X-7 | none | A2A `v1.1.0-rc` (a tag of 2026-10-29, not a release): track? | See section 3 | below |

**The one conflict between reports.** B.md (D-B1) recommends WebSocket server pings on by default and gRPC server keepalive left opt-in, because the dispatcher's own comment says clients may object to server PINGs and the client avoids HTTP/2 PINGs (grpc-go sends `GOAWAY` for frequent ones). D.md (D-20) recommends both default on (gRPC 60 s/20 s, WebSocket idle 120 s) with `without_*` opt-outs. They agree on WebSocket. Neither was tested against a strict peer.

## 2. Decisions by group

Recommendation column is the report's. "Doc" means documentation only.

### Deployment (AL.md)

| ID | Rows | Question | Recommendation |
|---|---|---|---|
| D-AL-1 | A14a, A22b | Environment-variable names in `deploy-agent` | Read `A2A_DATABASE_URL` instead of the generic `DATABASE_URL`; warn on an ignored `DATABASE_URL` or `A2A_BIND_ADDR`; one table of every example's variables |
| D-AL-2 | A15d | Should `/ready` probe the push-config store? | No; document why |
| D-AL-3 | A19b | Reproducible apt layer in the runtime image | Drop apt from the runtime stage; copy the CA bundle from the pinned builder |
| D-AL-4 | A24 | Ship a ready-to-run server image or binary | No, until signing and an image pipeline exist |
| D-AL-5 | L11 | Short-lived clients on the client dashboard | Guidance now; a Collector step is needed for anything stronger |

### Network and TLS (B.md)

| ID | Rows | Question | Recommendation |
|---|---|---|---|
| D-B1 | B-10 | Keepalive defaults, WebSocket and gRPC | WebSocket ping by default (independent of the idle close); gRPC opt-in with docs (conflicts with D-20) |
| D-B2 | B-11 | A blocking send longer than an LB idle timeout | Doc (stream, or `returnImmediately`, or raise the LB timeout above `send_timeout`) |
| D-B3 | B-25 | WebSocket on the HTTP port | Doc the limit; build the upgrade route when asked |
| D-B4 | B-26 | Forwarded client certificates | Doc recipe now; an opt-in interceptor after B-09's trusted-proxy list |
| D-B5 | B-29 | FIPS provider, CRL | Non-goals; document the escape hatches; add `from_oidc_issuer_with_tls_config` |

### Observability (C.md)

| ID | Rows | Question | Recommendation |
|---|---|---|---|
| C-08 | C-08 | Unrecognised `OTEL_*` values: fatal (today, test-pinned) or warn and ignore (the spec) | Warn and fall back for the three enum variables; keep errors for TLS, files, headers |
| C-10 | C-10 | Baggage and `OTEL_PROPAGATORS` on the A2A wire | Doc now; opt-in baggage later |
| C-14b | C-14 | Where do alerts go | A commented `alerting:` block and a README paragraph |
| C-17 | C-17 | The per-call WebSocket credential warning | Rate-limit it; raise fail-closed as a separate security item |
| C-22 | C-22 | Kubernetes assets | Generate a `PrometheusRule` and dashboard ConfigMaps from the existing files, checked in CI; no Helm chart |

### Resilience and replicas (D.md, E.md)

| ID | Rows | Question | Recommendation |
|---|---|---|---|
| D-05 | D-05 | Webhook durability and dead-webhook handling | Doc now plus a delivery-id header; an outbox only when a deployment asks |
| D-10a | D-10 | `Retry-After` longer than `max_backoff` | Stop retrying and return the error with `retry_after()` readable; jitter the short case |
| D-11 | D-11 | Retries and stream resume | Opt-in stream resume; `follow_task` resumes regardless |
| D-18 | D-18 | In-memory eviction of live tasks | Evict in tiers (terminal, then running, then parked last) and warn |
| D-20 | D-20 | Liveness bounds on by default | See the conflict above |
| D-21b | D-21 | Hand a running task to another replica on shutdown | No; revisit with a resumable-executor trait |
| E-F12 | E-F12 | Tenant `max_concurrent_tasks` is per replica | Doc "per replica; divide by replicas" |
| E-F13 | E-F13 | `Profile::Production` accepts in-memory halt and lease stores | Refuse them when the task store is shared, with an opt-out |
| E-F25 | E-F25 | Idle reattach polling is unbounded | Doc the cost, and a backoff knob with the old 250 ms as its floor |

### Security (SEC.md)

| ID | Rows | Question | Recommendation |
|---|---|---|---|
| D-1 | SEC-11 | `BearerTokenTenantResolver::new()` and `Default` use the raw token as tenant | Hash it (no compile break) and deprecate `new()` |
| D-2 | SEC-14 | Webhook ownership verification | No handshake (the spec defines none); a default per-host rate and in-flight cap |
| D-3 | SEC-17, SEC-13 | Who screens a webhook at delivery | A public `PublicOnlyResolver` plus the trait's contract in rustdoc; one change covers both rows |
| N-SEC-8 | none | File URLs unchecked by default although spec 14 says references must be validated | Warn at `build()` when the card has URL-capable input modes and the guard is off; leave the default |

### API shape and docs (GC.md, GD.md)

| ID | Rows | Question | Recommendation |
|---|---|---|---|
| D-GC-1 | G-10 | `#[non_exhaustive]` on generated wire structs | The four card types now; fix `STABILITY.md` 4 for the rest |
| D-GC-2 | G-06b | Server default features lack `tls-rustls` | Add it |
| D-GC-3 | G-07c | Streaming off by default in `AgentCard::new` | Keep (spec 3.3.4); say so in the docs |
| D-GC-4 | G-09 | Too many ways to serve | Doc now; an ADR for 0.17 |
| D-GC-5 | G-13 | Message-id generator | An SDK-level `new_message_id()`; the types crate stays dependency-free |
| D-GC-6 | G-14.2 | `Telemetry::init()` export default | Export only where configured; upgrade-guide line |
| D-GC-7 | G-15 | Naming sweep | Additive aliases only |
| D-GC-8 | G-18 | `RequestContext.context_id` type | `ContextId`, in the 0.16 batch |
| G/D-10 | G/D-10 | hello-agent framing | Reword with the measured numbers |
| G/C-01 | G/C-01 | CHANGELOG shape (785,059 bytes) | A Highlights block per release, required by `check_release_tree.py` |
| G/C-02 | G/C-02 | Patch releases that change behaviour | Amend `STABILITY.md` and make the cadence gate refuse a patch with `Behaviour Changes` and no exception line |

### Swarm scale (H.md; the handoff says each needs an ADR)

| ID | Rows | Question | Recommendation |
|---|---|---|---|
| H-D1 | H5b | Lease control-plane cost | Measure first (a `swarm_scale` arm), then a per-replica sweeper; lease groups wait for H19 |
| H-D2 | H6 | Retry budget, breaker, in-flight cap | Per-client opt-in now, with an overall deadline; a process-wide registry after X-3 |
| H-D3 | H7 | Readiness and saturation | Keep `/ready` as reachability plus draining; add informational body fields and a utilisation gauge |
| H-D4 | H9 | Admission queue, priority, fairness | Per-tenant reservations with a burst pool, plus priority shedding; a queue only if measured |
| H-D5 | H10 | Task lineage and job-scoped halt | A lineage extension with `root` defaulting to the originating context |
| H-D6 | H11 | Tenant dimension on metrics | Opt-in top-N tenants on refusal and admission counters; cost goes to a ledger |
| H-D7 | H12 | Client load balancing | Decline in core; document L4/L7 and headless services |
| H-D8 | H13 | Byte budget and blob offload | Lower defaults and publish a sizing formula; measure before building a budget |
| H-D9 | H14 | Push durability | Doc now; a PostgreSQL outbox behind a feature later |
| H-D10 | H16 | Rate-limiter capacity policy | Keep fail-closed; warn at 80% full; an overflow bucket as a setter |
| H-D11 | H19 | Polyglot swarm controls | A language-neutral spec page with conformance vectors, then upstream proposals |

## 3. Two questions from the session brief, answered from what was fetched

### Agent Skills (agentskills.io)

Facts, from a web fetch of the site and the `agentskills/agentskills` repository on 2026-10-31 (a model read the pages; the quotes below are its quotations, not re-fetched by the lead):

- It is an open format: "a skill is a folder containing a `SKILL.md` file", with YAML frontmatter. Required: `name` (1 to 64 characters, lowercase letters, digits and single hyphens, equal to the directory name) and `description` (1 to 1024 characters). Optional: `license`, `compatibility` (up to 500 characters), `metadata` (string map), `allowed-tools` (marked experimental). Optional directories `scripts/`, `references/`, `assets/`.
- Clients load only name and description at startup, the body on activation, other files on demand. The client guide is advisory (no RFC 2119 conformance section) and says to consider gating project-level skills on a trust check.
- **Not found on the pages read:** a specification version or date; a statement that the format was donated to or is hosted by the Linux Foundation (the pages say it "was originally developed by Anthropic, released as an open standard"); any mention of A2A. `CONTRIBUTING.md` says the specification is "still iterating". Code is Apache 2.0, documentation CC-BY-4.0; the reference library was not opened. The brief's statement that it moved to the Linux Foundation may be true; these pages do not show it.
- Nothing in this repository mentions it today (`grep` for `SKILL.md|agentskills` found no file).

How it relates to A2A: `AgentSkill` (generated type, `id`, `name`, `description`, `tags`, `examples`, modes, security) advertises what an agent can do to its peers. An Agent Skill is a package of instructions an agent loads into its own context. They meet in three places: a card could list the skills an agent has; a model-backed executor (`ToolLoopExecutor`) could use them; a CLI could validate them.

Options:

1. **Nothing in 0.16.0.** The format has no version to pin and is still changing.
2. **A parser, validator and card bridge** behind a cargo feature (`agent-skills`) in the server crate: read a skill directory, check the frontmatter against the rules above, produce `AgentSkill` entries; `a2a skills validate`. No script execution. Size S to M, additive.
3. **Option 2 plus tool-loop support**: `ToolLoopExecutor::with_skills`, catalogue in the system prompt, an `activate_skill` tool, project-level skills behind a trust opt-in. Size M.
4. **Option 3 plus running bundled scripts.** The client guide does not require sandboxing or approval for scripts, which makes this the risky part; size L and security-heavy.

Recommendation: option 2 after 0.16.0, then 3, never 4 in core. Pin the specification by repository commit. Open for the maintainer: whether it should come before the 0.16.0 release.

### A2A specification versions

From a web fetch on 2026-10-31 (also a model's reading, with some pages unread by it): the newest **published release** is **v1.0.1** (28 May 2026), the version this repository targets. `v1.1.0-rc` exists only as a **tag** (29 Oct 2026) with no release notes. Read from the files at that tag, it is additive: a `SendLiveMessage` RPC, `generation` and `timeline` fields on `Task` and events, `AgentCapabilities.user_artifacts`, `TaskMessageUpdateEvent`, with `history` fields deprecated for removal in 2.0, and `SendMessageRequest.message` no longer marked `REQUIRED`. `main`'s `a2a.proto` has none of it. The agent found no official TCK in the specification repository, but this repository's `.github/workflows/official-tck.yml` clones `a2aproject/a2a-tck`, so that line of its report is a gap in its search, not a fact.

Recommendation: stay on v1.0.1 for 0.16.0; when 1.1 is released, write an ADR and run the official TCK against it before changing anything.
