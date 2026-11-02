<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Handoff — session state

Working state between sessions: which branches exist and why, what is in flight
outside this repository, and what the next session should pick up.

This is **not** `ROADMAP.md`. That file takes only items the repository has
committed to and refuses speculative milestones; this one records where things
stand, including decisions to *not* do something. When an item here becomes work
the repository commits to, move it there and delete it here.

It is hand-maintained; `git log -1 --format='%ci %s' -- docs/handoff.md` says
when it last changed. Everything before 2026-10-15 — each branch's narrative,
its item list, its verification of record — is kept verbatim in
[`handoff-history.md`](handoff-history.md). This file is the current state
only: when something here is done, delete it rather than annotating it.

## Now

- **Branch `claude/pensive-bardeen-c6ofm8` (2026-11-01), from `main` at `4a8243c`
  (the merge of pull request #185): the 149 open audit rows are triaged and most fix groups are
  committed (below). One pull request, opened only when the groups the maintainer wants pass the bar;
  none is open. The branch was pushed on 2026-11-02 (pushing it runs no CI); the pull request is
  the maintainer's call once the groups they want are in.**
  - **First, next session.** (1) What waits on the maintainer: permission to run `promtool` (group J);
    forks F13 to F16, never asked, with positions in `docs/audits/2026-11-01-triage/DECISIONS.md`;
    whether the other 26 wire structs (G-10 option A) go into 0.16.0; the D-05 dead-webhook policy; the
    options for E-F18, E-F25 and D-18. (2) Then build, in this order: M (delegation hand-off on shutdown,
    F5), O and the HttpPool (F7, ADR 0032), the contained rows of L (D-19, E-F15, E-F14, E-F22, E-F21),
    the rest of I and G. (3) Before the pull request: `cargo mutants --in-diff` through CI's shards,
    `cargo-semver-checks`, the official TCK, the Cadence-exception line and release prep (`PLAN.md`,
    "Before 0.16.0 is tagged").
  - **Fix work, `git log 4a8243c..` (oldest first; every commit is `Tom F.`, signed off, no other
    trailer, for DCO):**
      - `4a3cbe1` docs(audit): triage the 149 open rows of the 2026-10-29 audit against main@4a8243c
      - `a3a5dde` fix(client,cli): --header goes only to the interface that was checked (audit SEC-09, NEW-f-1)
      - `78fcd2c` fix: credentials go only to the host the caller named (audit SEC-08, SEC-09, SEC-18, NEW-f-5, NEW-f-6, G/D-05)
      - `c902c46` fix(server): a replica that cannot renew a claim stops the turn before anyone can take the task (audit E-F7, NEW-e-1, NEW-h-s0-1, NEW-h-s0-5, NEW-h-s0
      - `aa6e4ff` fix(server): a task's ending always reaches its webhooks, its log and its caller (audit NEW-d-1, D-03, E-F9, NEW-e-2, E-F20, D-16, D-23, G-21, NEW-g-a
      - `6d746dd` fix(client,server,cli): gRPC honours its send timeout, its card and the process's roots; one TLS identity reaches every path (audit NEW-g-api-1, G-20,
      - `065f0a0` docs: documentation that disagreed with the code, the book's ignore ratchet, and thirteen rustdoc blocks nothing compiled (audit G/D-02 to G/D-11, G/C
      - `50242e0` docs(adr): Agent Skills and steering a running task, proposed (triage forks F9, F10)
      - `4b6b382` fix(client,server): loopback is never sent to a proxy, NO_PROXY is read as Go reads it, the WebSocket dials like the HTTP transports (audit NEW-b-2, B
      - `9c69ef6` feat(server): the production profile refuses process-local stores over a shared task store (audit E-F13; amends ADR 0025)
      - `4fd80d0` fix(server): a wrong OTEL_* value is noted and the process starts, as the specification asks (audit C-08, NEW-c-5)
      - `636eaf0` feat(deploy-agent): a signed image from the release workflow, a version on the card, retention in PostgreSQL (audit A13, A27, NEW-a-l-1, NEW-a-l-3, NE
      - `41756f1` feat!: RequestContext.context_id is a ContextId, AgentCard::new declares streaming, the server crate defaults to tls-rustls, sign_agent_card is remove
      - `0b06ec2` docs: 0.15.1 was never released, so its notes are 0.16.0's; the release notes open with a Highlights block (audit G/C-01, G/C-02, NEW-t-1; forks F6, F
      - `7b36d51` feat: SendMessageResponse accessors, build_arc, agent_executor! with a receiver, new_message_id, Debug for EventEmitter and A2aRouter (audit G-22, G-1
      - `4ec89f3` fix(client): a tenant the caller set is kept in whichever order the builder is called (audit G-19)
      - `a8bf45e` fix: Retry-After is spread, the rate limiter overflows to one bucket, an unread halt store is retryable and not ready, an unknown halt row halts every
      - `e2712ec` fix(client,server): a WebSocket peer that stopped reading is dropped, one that went silent is found out, a failed handshake says whether to retry (aud
      - `43e5d94` docs(handoff): the branch's commits, what is verified and what is not, two lessons from the WebSocket keepalive and the mutant runner
      - `2b83f88` fix(server): an https webhook is connected to the address that was checked, a host's other addresses are tried, DNS lookups cannot fill the blocking p
      - `b9c1304` feat!(types): thirteen protocol structs are #[non_exhaustive], built with new and with_* (audit G-10; fork F1)
      - `3c3bac5` docs(handoff): group K and G-10 committed, group J blocked on promtool, what was and was not run on the tip
      - `c4c5c29` fix(examples): agents are served with Server, the health monitor probes its roster, the bridge quotes the remote card (audit G-05, SEC-15, SEC-16)
      - `da797f5` docs: the audit register's fix status after K, G-10 and Q, and what was run on the tip (PostgreSQL suites, feature matrix, 6055 tests)
    Each fix has a regression test that failed before it (the commit messages say how each was
    shown, and which hand-applied mutants each test kills).
  - **Verified, and what was not.** On the Q commit (the last code commit; the later ones are
    documentation): `cargo nextest run -p a2a-protocol-types -p a2a-protocol-client -p
    a2a-protocol-server -p a2a-protocol-sdk --all-features --profile ci` 6055 passed, 177 skipped; the
    other 24 workspace members 319 passed; doctests types 54, client 31, server 75, sdk 2, book 316;
    `cargo hack clippy -p <the four crates> --all-targets --each-feature -- -D warnings` 59 of 59, no
    warnings; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean; 36 of 36
    script gates; `cargo fmt --check`; the PostgreSQL suites against the local PostgreSQL 16
    (`scripts/run_postgres_suites.py`: 12 test targets and 11 library modules, then the named CI suites:
    `postgres_store_tests` 72, `multi_replica` 7, `turn_claims` + `replica_processes` +
    `cross_replica_cancel` + `stale_write_position` 22, `encrypted_store` 1). Hand-applied mutants: 14 in
    B2, 2 in I, 12 in F (after its keepalive was rewritten), 19 in K, 16 in G-10's builders, 10 in Q;
    `cargo mutants` cannot build 23 of the builders' 24 mutants (the types have no `Default`). **Not
    run:** `cargo mutants --in-diff` on the server and client crates (a client run on A to E: 70 mutants,
    48 caught, 21 unviable, 1 missed and killed afterwards; the server run was stopped at 20 of 168; the
    G-10 conversion makes the diff ~330 files, so CI's shards are the way to run it,
    `.github/workflows/mutants.yml` has the command); `cargo-semver-checks` against v0.15.0 (not
    installed here; needed to confirm the breaking list); the official TCK; the toxiproxy partition run
    for group B; Kubernetes; the fuzz targets (compiled only); a live pass of the Squid, private-CA and
    model probes after the fixes; the sdk crate under `--no-default-features`. Live: the `deploy-agent`
    image was built from this tree and run (it found that the build fails behind a TLS-inspecting proxy;
    fixed with a BuildKit secret), and the retention sweep ran against the local PostgreSQL. Each
    intermediate commit was not built on its own; only the tips were.
  - **Not built (open), by the plan's group** (`docs/audits/2026-11-01-triage/PLAN.md`):
    **J (observability stack) is blocked**: its proof is `promtool test rules` and a live stack, and the
    auto-mode classifier refused to run a `promtool` I had downloaded (the 111 MB
    `prometheus-3.15.1.linux-amd64.tar.gz`, no checksum check; deleted, never run). Needs the maintainer to
    allow `promtool` (or the pinned `prom/prometheus:v3.15.1` image). No workflow runs
    `deploy/observability/prometheus/tests/a2a-alerts.test.yml` today. **G-10 option A remainder**: the
    other 26 wire structs (params, responses, security) are still exhaustive, about 600 literal sites, same
    codemod (`STABILITY.md` section 4 says so). G-12, G-16, G-09/G-14/G-15 (undecided), the rest of I
    (C-06, C-09, C-10, C-12, C-13, C-16, C-22, C-23, NEW-c-3/-4), K's NEW-d-3 (per-address bound),
    NEW-f-2 (fairness between callers: the sender is not told who registered a config), SEC-14's
    per-caller quota, per-destination rate and verification handshake, D-05's dead-webhook policy and
    outbox, L (fleet correctness), M (delegation hand-off on shutdown, F5), O and the HttpPool (F7,
    ADR 0032), the swarm designs beyond the four fixes, Q, most of G (A14, A15, A19, A21-A23, A25, A26,
    NEW-a-l-2, -4a, -5), B-10/B-11/D-20 (gRPC and idle defaults), D-03(d), E-F8 (F13), B-14 cert reload
    (F15), NEW-b-1/-5 and the proxy boundary for https pinning (F16). Nothing built forecloses any of them.
    The maintainer's brief: find every way the SDK fails a real deployment (local, Docker, Kubernetes,
  proxy or none, public/private/self-signed CA, mDNS, replicas, outages, disconnects), with live runs
  against llama.cpp and small Qwen and Gemma models, A2A v1.0.1 conformance, OpenTelemetry, the EU AI
  Act and GDPR; consider Agent Skills (agentskills.io); support sub-agents, fan-out and messaging a
  running agent; and ship 0.16.0 only when it is checked, never on assumption.
  - **The record is `docs/audits/2026-11-01-triage/`**: `README.md` (method, what was and was not
    checked, corrections), `PLAN.md` (191 items in 19 commit groups, each item in exactly one),
    `DECISIONS.md` (12 forks and 46 per-row option sets), ten `<slice>.json` with `file:line` evidence,
    `live-results.md` and `probes/`. `docs/audits/2026-10-29/README.md`'s `Status` column is updated
    for all 149 rows and has a new section N for what the triage found. Counted with the
    findings-triage skill's counter on 2026-11-01: 283 rows; confirmed (read) 125, fixed 66, needs
    decision 46, fixed (part) 25, verified live 7, not a defect 5, duplicate 5, confirmed (live) 2,
    refuted 1, cannot check 1; **none `open`**. Of the 149: 71 confirmed, 46 need a decision, 21 fixed
    in part, 7 already fixed, 3 not defects, 1 cannot be checked by reading. The previous figure here
    (149 `open`, "some likely stale") is withdrawn: 7 were already fixed, and of the 3 not-defects C-20 was
    a misreading while D-24 and D-25 record areas found sound.
  - **The three S0 rows are all `fixed (part)`.** H2 (private CA and mTLS) is S1: every binding can
    trust a private CA, except the token provider and gRPC's use of `SSL_CERT_FILE`; one pool per
    client remains. H3 (orphaned tasks) is medium: the reaper fixes the default SQL configuration, but a
    reaper that ends a live task whose owner merely failed to renew is a new risk (NEW-h-s0-1,
    NEW-e-1; a probe is written, not run). H4 (delegation) is high: resume, `reattach` and a
    journal exist, but nothing drives recovery, a graceful supervisor redeploy still cancels its
    children, and gRPC/WebSocket delegations are cancelled when idle.
  - **Found by running it (this session):** the CLI's `--header` guard (SEC-09, merged in #185) was
    bypassed when the card's first interface is on the typed host but cannot be built (binding
    `NOPE`, or `ftp://`) and a later one is on another host (NEW-f-1; fixed in A). A blanket
    `HTTP_PROXY` sends loopback traffic to the proxy (NEW-b-2; not fixed, F2). `live-results.md`.
    A blocking send over gRPC was cut at the request timeout by tonic's channel timeout
    (NEW-g-api-1): reproduced and fixed in D at a small scale (a 3 s turn, a 1 s request timeout), not
    with a 40 s executor. A blocking send to a task another replica had cancelled was answered
    `working` (found while fixing E-F9; fixed in C).
  - **Not release-ready.** `docs/readiness-bar.md` still records no measurement for R1 to R7, C1 to C5
    and F1 to F5. No Kubernetes, IPv6-on-the-wire, public-CA collector or official-TCK run has been made
    since #185.
  - **How #184 was verified, the bar for what follows.** Before a push:
    `scripts/preflight.sh --force`; every script gate (the list is in the
    workflow's Static checks job); `cargo hack clippy -p a2a-protocol-types
    -p a2a-protocol-client -p a2a-protocol-server -p a2a-protocol-sdk
    --all-targets --each-feature -- -D warnings`, exactly as CI runs it
    (one crate is not enough: 3dac520 failed the matrix that way);
    `scripts/run_postgres_suites.py` against PostgreSQL; and, for each
    crate a change touches, `cargo mutants --in-diff` on its diff from
    `main` (the command is in `.github/workflows/mutants.yml`). Hand-picked
    mutants are not that check: two CI mutation runs (on df86307 and
    8ec32f4) found 47 survivors that hand-picking had missed (10 client,
    10 discovery, 27 server). The
    server's full run is 20 shards of about 50 minutes; locally, apply its
    survivors by hand (scratchpad script pattern: replace, run the one
    test, restore) and let CI's run be the full one. macOS and Windows
    cannot be run here; CI is their check.
  - **Pull requests [#184](https://github.com/tomtom215/a2a-rust/pull/184) and
    [#185](https://github.com/tomtom215/a2a-rust/pull/185) are merged** (`a0e6673`, `4a8243c`). The
    workspace reads **0.16.0**, bumped at the maintainer's choice because `cargo-semver-checks` blocks
    the branch's deliberate breaks under 0.15.x. It is a version bump, not a release: no tag.
    **0.15.1 was never tagged or published** (checked 2026-11-01: crates.io, the GitHub latest release
    and the remote tags all end at 0.15.0), yet `CITATION.cff` still says 0.15.1, released 2026-10-26
    (NEW-t-1). The `0.15 → 0.16` upgrade guide is `book/src/reference/upgrading/0.15-to-0.16.md`. Before
    tagging 0.16.0: RELEASING.md's checklist, and STABILITY.md's cadence, since 0.15.0 was a breaking
    release in October 2026 (a second one that month needs a `**Cadence exception:**`). The SLIMRPC
    binding's requirements read `0.16`; its own version (0.7.0) moves at its own release (RELEASING.md,
    "releases separately").
  - **mDNS discovery is built, not released:** `crates/a2a-protocol-discovery`
    (ADR 0029, proposed) is `publish = false`. Publishing it needs its
    crates.io name set up for trusted publishing and a place in
    `.github/workflows/release.yml` (packaging steps, SBOM and attestation
    lists); then `a2a discover` can join the CLI. Its round-trip tests need
    multicast looped back on the CI runner. First run in CI on pull request
    #184: on macOS the browse heard the agent only over loopback, and
    `card_urls` left loopback out, so the card fetch had no URL; it now
    falls back to loopback when that is all there is.
  - **Run live 2026-10-30:** `live-model` 10 of 10 against Qwen3.5-4B and
    against Gemma 4 E2B (task list L13 to L15); with the drafts check, 11
    of 11 twice against Qwen3.5-2B (`examples/live-model/README.md`); every
    server PostgreSQL test through PgBouncer 1.26.1, 147 of 151, which found the halt store's
    `LISTEN` broken behind it (L16, fixed; L17).
    `deploy-agent` as two hardened containers on PostgreSQL (L18); its Dockerfile's build stages ran on 2026-11-01 (see above).
    `promtool test rules` (Prometheus 3.15.1, from its image) on
    `deploy/observability/prometheus/tests/`: passes. CI does not run it.
  - **Not yet run live:** the client dashboard with a
    long-lived client (L11: short-lived CLI processes never export); the
    (the `deploy-agent` Dockerfile's build stages ran on 2026-11-01.)
  - **First, next (2026-11-01).** The maintainer chose **one large pull request**, built as the
    19 commit groups of `docs/audits/2026-11-01-triage/PLAN.md`; open it only when the groups meant for
    0.16.0 pass the bar above (a pushed branch starts no CI; the workflows run on `pull_request`).
    1. Groups A to D of `PLAN.md`, in that order. Each regression test first, seen to fail on the
       unfixed code. A and the NEW-d-1 test of C are reproducible now: `probes/` holds the two live runs.
    2. Put the twelve forks of `docs/audits/2026-11-01-triage/DECISIONS.md` to the maintainer; none of
       groups A to D waits on one.
    3. Run the baseline suites on `4a8243c` in this environment (they were not run: only the 36 script
       gates, each exit 0, and a dev build of `deploy-agent`, `a2a`, `live-model` and `hello-agent`).
    4. `main`'s own `CI` run on `4a8243c` had not finished when it was last read (13 other push
       workflows were `success`); read it before building on `main`.
  - **Environment for live runs, as stood up 2026-11-01** (4 vCPU, 15 GB, about 22 GB of disk free after
    the build; the Docker daemon, PostgreSQL and the services below are *not* running at session start):
    `dockerd >/tmp/dockerd.log 2>&1 &` (the CLI is installed, the daemon is not); `pg_ctlcluster 16 main
    start` then `su postgres -c "psql -c \"ALTER USER postgres PASSWORD 'postgres'\""`, giving
    `postgres://postgres:postgres@localhost:5432/postgres` (PostgreSQL 16.15). llama.cpp: `docker pull
    ghcr.io/ggml-org/llama.cpp:server` works (build 12030, commit `a91eab67d`); models come from Hugging
    Face, which is reachable: `unsloth/Qwen3.5-2B-GGUF` `Qwen3.5-2B-Q4_K_M.gguf` (SHA-256 `aaf42c8b…9223`,
    the file of the 2026-10-28 run) and `unsloth/gemma-4-E2B-it-GGUF` `gemma-4-E2B-it-Q4_0.gguf`
    (`31d3a3c6…0132`, **not** the 2026-10-28 Gemma file `8e30dff3…6a52`, so Gemma results are not
    byte-comparable with that run). Serve with `docker run -d --name llama -p 127.0.0.1:8080:8080 -v
    /home/user/models:/models:ro ghcr.io/ggml-org/llama.cpp:server -m /models/<file> --host 0.0.0.0
    --port 8080 --jinja -c 16384 -np 2 -t 4`; a smoke prompt answered. `scripts/install_cargo_mutants.sh`
    (about 5 minutes) installs the patched `cargo-mutants` 27.1.0 and `cargo-nextest` 0.9.146. Not
    installed and not run: `cargo-hack`, `cargo-deny`, `cargo-semver-checks`, `mdbook`, `kind`,
    `kubectl`, `promtool`, the Collector, Squid, HAProxy, toxiproxy (pull images where a run needs
    them). Build with CI's flags (`RUSTFLAGS=-D warnings CARGO_PROFILE_DEV_DEBUG=0`) so `preflight.sh`
    reuses the artifacts. `pkill -f <text>` kills your own shell when the text is in its command line;
    use `pkill -x <name>`. Git here is configured as `Claude <noreply@anthropic.com>`, which `dco.yml`
    rejects; commit with `git -c user.name='Tom F.' -c user.email=tomf@tomtomtech.net commit -s`, as
    the merged commits are. The maintainer confirmed on 2026-11-01 that he signs off every commit as
    Tom F. with no other trailer, so that DCO passes.
  - **Toolchain: run `rustup update stable` first.** This image shipped Rust 1.97.0; CI's
    `toolchain: stable` was 1.99.1 (2026-10-15). Clippy 1.97 failed `doc_markdown` and
    `duration_suboptimal_units` on files nobody had touched, which looks like a defect in `main` and
    is not; under 1.99.1 clippy is clean on the crates checked. Clear `target/` after the update.
  - **Dependencies (maintainer, 2026-11-01):** the repository does not ban new dependencies. Avoid one
    where pure Rust can do the job to production grade; do not reinvent a vetted solution to avoid
    one, above all for anything security-sensitive. `CONTRIBUTING.md` (Dependency Policy) and ADR
    0002 (amendment) say so; the first use is `subtle` for constant-time comparison.
- **Branch `claude/intelligent-bohr-9qraxk` (2026-10-28), on `main` at
  `568fc6c1` (pull request #182, the 0.15.1 preparation, merged; no tag).**
  The maintainer judged 0.15.0 not ready in practice and asked for everything
  to be run live before another release. Done here, each item verified by the
  run named in `CHANGELOG.md` `[Unreleased]`, where every change is listed:
  - **Merged as pull request
    [#183](https://github.com/tomtom215/a2a-rust/pull/183)** (`8dc4331`).
    Run `scripts/preflight.sh` (CONTRIBUTING) before each push.
  - **The review's follow-ups (2026-10-29)**, each in `CHANGELOG.md` or a
    commit message with its evidence: retry metrics
    (`a2a.server.store_retries`); the queue's write wait tied to
    `store_write_retry` (a chatty turn during an outage was failed and left
    `Submitted`); startup under a short `statement_timeout`; the release of
    an idempotency key or lease pinged rather than retried; every retried
    write run twice on all six stores; round trips per send pinned
    (`tests/statement_count.rs`); a cancel forced between a continuation's
    read and save; the fence scoped to its task; every PostgreSQL suite run
    in CI, found rather than listed; PgBouncer in transaction mode run;
    `scripts/live/` committed.
  - **Replicas work without session affinity**: turn claims
    (`store::claim`, `handler::claims`), tested on SQLite and PostgreSQL in
    process (`tests/turn_claims/`) and as separate OS processes, one killed
    mid-turn (`tests/replica_processes.rs`), and live as two `deploy-agent`
    processes behind HAProxy on PostgreSQL 17 (6 of 6 `GetTask`s through the
    balancer answered, against every second one `TaskNotFound` before).
    Mutation-checked: claims off, all 10 claim tests fail; fence condition
    replaced by `TRUE`, exactly the 2 fence tests fail; the claim released
    at the old point, the terminal-event test fails 5 of 6 runs.
  - **An independent review of turn claims** (a read-only agent, given the
    commit and not this session's conclusions) reported 2 high, 4 medium and
    5 low findings. Six were confirmed and fixed (`fa2785eb`), each with a
    test that fails when the fix is reverted: a renewal racing a new claim
    cancelled it; renewal by owner kept forgotten claims alive; foreign
    claims over a fenced store refused every write; the PostgreSQL fence read
    a stale snapshot under READ COMMITTED (reproduced with two `psql`
    sessions; now `FOR KEY SHARE`); the quorum count was unfenced (two
    approvals on two replicas lost one, 3 of 3 runs); a waiting send held its
    idempotency key, so a retry elsewhere was told "accepted" for a send then
    refused. Also fixed: shutdown now releases claims, and (2026-10-29) the
    fence names the task written. Not changed, with the reason in *Open
    engineering*: a send to a task whose cancelled executor still winds down
    is refused at once.
  - **Claims under load and their cost**: `tests/soak_multi_replica.rs`, now
    with `A2A_SOAK_TURN_CLAIMS=off` and a check that no claim outlives a run,
    three 60 s runs each way, alternating, release build, PostgreSQL 17 on
    the same 4-vCPU host: 360 ± 17 pairs/s with claims, 423 ± 14 without
    (15% fewer), median 21.4 ms against 18.6 ms; 0 failures, 0 cross-replica
    misses, 0 claims left, in all six.
  - **OTLP over TLS and through proxies, live**: otelcol-contrib 0.163.2 with
    TLS from a private CA, mutual TLS, a self-signed certificate marked CA and
    one `CA:FALSE`, an expired one, a wrong host name and an IP SAN, over
    OTLP/HTTP and OTLP/gRPC; Squid 6.13 open, denying and requiring a login.
    Everything that should work delivered all three signals. Of the warnings
    for what should not, five had no hint and three hints never fired (they
    matched error names rustls does not print); all fixed, tested on the
    failures' own text. `NO_PROXY=[::1]` exempted nothing; fixed. IPv6 could
    not be dialled (no IPv6 in this kernel), so its URL and `NO_PROXY`
    handling is unit-tested only.
  - **Collector hardening**: `memory_limiter` and `health_check` in both
    Collector configurations, and `docker-compose.prod.yml` (Grafana login,
    only Grafana and OTLP published, a memory limit), each run live.
  - **Every example run against llama.cpp `llama-server` b11913**
    (`ghcr.io/ggml-org/llama.cpp:server`, commit `7aca0d2ea`) with Qwen3.5-2B
    Q4_K_M (SHA-256 `aaf42c8b…9223`): all exit 0, after fixing four that sent
    no token cap (a reasoning model ran past every timeout). `live-model` 10
    of 10 with Qwen3.5-2B and with Gemma 4 E2B Q4_0 (`8e30dff3…6a52`);
    `tool-loop-agent`, `genai-agent`, `rig-agent`, `mcp-agent` and
    `incident-response` also on Gemma 4 E2B, each exit 0 with its model leg
    exercised. The examples print the model name they ask for, not the one
    llama-server serves (it serves its one model whatever the name).
  - **Observability out of the box**: `deploy/observability/docker-compose.yml`
    (Collector 0.163.2, Prometheus 3.15.1, Tempo 2.9.0, Loki 3.7.9, Grafana
    13.2.4); `check_dashboards.py` ok through it (21 rules, 45 panel queries,
    only "Persistence errors" empty, as no store failed); the three
    dashboards, a Tempo trace and a Loki line's link to it looked at as
    screenshots, which found three panel defects the query check had passed
    (fixed). Export run live four ways: through the Collector over OTLP/HTTP
    and OTLP/gRPC, straight to Prometheus's OTLP receiver, and to the console.
  - **Docker**: `examples/deploy-agent/Dockerfile` built (140 MB) and run as
    two containers on the compose network, sharing a PostgreSQL on the host
    through `--add-host=host.docker.internal:host-gateway`; `docker stop`
    drained and exited 0. Built from the pinned digests pulled through
    `mirror.gcr.io` (Docker Hub answered 429) and with this sandbox's proxy CA
    added in a scratch copy of the Dockerfile (*Environment*); the repository's
    Dockerfile is unchanged.
  **Not run, and so not claimed:** Kubernetes (kind could not start a pod
  sandbox here: nested `runc` fails, "can't get final child's PID from
  pipe"; Kubernetes 1.35's kubelet also refuses this host's cgroup v1), IPv6
  on the wire, a public-CA collector, a network between replicas
  and database other than a local proxy adding latency, `cargo-semver-checks`,
  `cargo deny`, `promtool` (the
  rules were exercised live by Prometheus, not unit-tested), the official
  TCK. **For the maintainer:** (1) the release number, now 0.16.0 by the
  book's own definition (public API added, which sends are refused changed;
  `CHANGELOG.md` `[Unreleased]` holds it, and the `[0.15.1]` section above it
  was never tagged); (2) sign-off: the commits are authored as the session's
  identity and signed off by it alone, so `scripts/check_dco.sh` will want
  them re-authored and signed off by you, as with the 0.15.1 branch; (3) the
  release notes should say that a 0.15 replica's write to a task a newer
  replica runs is now refused (`A2A01`), which is what makes the rolling
  upgrade safe, and that SQL of an operator's own against the task tables is
  treated the same way unless it sets `a2a.claims`.
- **Rolling upgrades from 0.15, safe without affinity (PostgreSQL)**
  (`store::claim::upgrade`, pg migration 9): every write this release makes
  marks itself; triggers refuse an unmarked (0.15) write to a task a newer
  replica holds, let a 0.15 cancel through and forward it to the claim, and
  turn a 0.15 write into a claim newer replicas wait for. Verified against a
  real 0.15.0 replica built from the tag (`scripts/live/upgrade_live.py`,
  with `scripts/live/replica` built against both trees): 4 of 4
  scenarios, 1 of 4 with the triggers disabled. `tests/turn_claims/upgrade.rs`
  replays 0.15.0's own SQL; five mutants of the guard are each caught. All
  database-gated tests pass on PostgreSQL 16.15 and 17.11 (2,478 each).
- **Replicas over a network** (toxiproxy 2.12.0 between each replica and the
  database): the claim costs one or two round trips a turn, 8% to 12% of
  throughput at round trips from 1.0 to 11.2 ms; a 5 s cut, a 20 s
  cut (taken over at 16.1 s, the cut-off writer fenced and stopped 2.5 s after
  its link returned) and a database restart mid-turn all behaved as the book
  says (*What it costs*, *When the network fails*).
- **cargo-chef and nextest**: the `deploy-agent` image caches dependencies
  (rebuild after a source change 221 s against 428 s); CI's test matrix runs
  nextest with doctests in their own steps. Neither the image build in CI nor
  the edited `ci.yml` has run on GitHub yet.
  ADR 0028 (Proposed) and `docs/adr/0028-wasm-spike/` are unchanged from the
  previous session; its `reproduce.sh` was not re-run.
- **Released:** 0.15.0 (tag `v0.15.0` on merge commit `e4fc3bc1`, 2026-10-25) is
  the newest release; 0.14.1 (2026-09-30) is the one before. It is the first
  signed tag: SSH-signed by the key in `.github/release-signers/`, and
  `scripts/verify_tag_signature.sh` accepts it against the keys on `main`.
  Release run 40196985363: attempt 1 failed `test · Windows` on
  `lease_renewals_are_in_the_parents_trace` ("1 renewals"; it needs 2), a
  re-run of that job passed, and the run finished green at 14:25 UTC.
  crates.io lists all four crates at 0.15.0 (published 14:24 to 14:25 UTC;
  each checksum equals the release's `SHA256SUMS`), docs.rs built all four,
  and GitHub accepted the shortened release body (40,600 characters). The
  release has no CLI binaries: `PUBLISH_CLI_BINARIES` is unset
  (`packaging/README.md`). **The Windows failure's cause is not
  established.** The same tree passed Windows twice in the pull request and
  150 of 150 times in a probe on `windows-latest` (renewal 2 started at 679 to
  707 ms; the test's window closed at about 1,022 ms, so a stall of over
  ~315 ms would explain it, and nothing showed one). The test now waits for
  two recorded renewals, with a 10 s deadline and the span names in its
  failure message, instead of sleeping a fixed second.
- **Book audit (2026-10-25):** 13 readers checked every book page against
  the code, the specification and the changelog, and 13 editors applied what
  held up. Checked mechanically afterwards: the book's doctests (281 passed,
  0 failed, 5 ignored, the baseline), `rustdoc -D warnings` on the four
  crates, the CI gates that need no build, `promtool` on the alert rules, and
  the two generated benchmark pages (each regenerates byte-identical from its
  generator). The harness does not compile the bodies of `#[test]` and
  `#[tokio::test]` functions (rustdoc strips them), so `testing.md`'s seven
  were compiled and run by hand against a working executor: 7 of 7 pass; the
  page's hidden stub executor does nothing, so they fail against it, which is
  why the page now says `CalcExecutor` is not shown. The book's type face was
  11.9 px in a 560 px column (mdBook's 62.5 % root made `1rem` 10 px);
  `book/theme/custom.css` now sets 17 px in a 720 px column. The banner cards'
  subtitle and the code card's footer said v1.0.0; they say v1.0.1 now (only
  that text band was re-rendered; `book/static/brand/README.md`). Left open:
  `book/src/images/dashboards/a2a-server.png` predates the Agent executor row
  (the page says so; retaking it needs the live stack);
  `examples/deploy-agent/Dockerfile` was one Rust minor behind the MSRV and
  is now on 1.94, with no gate that keeps it there; `docs/official-tck-
  findings.md` §1 says 87 passing and 5 failing "cannot be reconciled" with
  114, and an editor's arithmetic suggests it can (87 + 5 + 1 + 21), which
  nobody has resolved.
- **Book restructure (2026-10-29):** split into landing pages and
  sub-pages (78 to 210 pages; paths kept, so outside links hold), with 38
  diagrams. Diagrams: edit `book/diagrams/<name>.mmd`, run
  `scripts/render_book_diagrams.sh <name>` (Node and a Chromium; pinned
  mermaid-cli 11.4.2), commit both SVGs; pages use the `figure.diagram`
  markup documented in `book/theme/custom.css`. A new page needs a SUMMARY
  entry, a `book-tests/src/lib.rs` registration and `scripts/gen_sitemap.py`;
  moving a cited sentence needs `scripts/check_spec_ledger.py --fix-lines`.
  `scripts/check_book_links.py` (CI) checks links, anchors and images in the
  built book. Checked mechanically: doctests 302 passed, 0 failed, 5 ignored
  (up from 281: new blocks, and `tool-loop` added to book-tests' features);
  `upgrading_guide`, `book_defaults` and the observability catalogue test
  pass; every book gate passes; 0 broken links over 215 built pages; every
  diagram looked at in light and dark at 1280 and 390 px. Left open: no
  shipped alert or panel reads `a2a_server_store_retries_total`
  (observability/dashboards.md says so and suggests a query); READMEs outside
  the book with stale claims (`tools/a2a-cli` exit codes and `--header` on
  `card`, `tck/README.md` on BIND-EQUIV-004, `packaging/README.md` on the
  signing key).
- **Working branch:** `claude/pensive-bardeen-c6ofm8`, from `main` at `4a8243c`. `git log origin/main..`
  lists what it holds that `main` lacks.
- **Merged in #179** (branch `claude/zealous-fermat-f4kg9z`, 2026-10-23 to
  2026-10-24; merged 2026-10-24 13:23 UTC as `2781bb69`). The PR head
  `6707f4d` had 104 check runs, 98 success and 6 skipped by their own
  workflows' conditions (`repos/tomtom215/a2a-rust/commits/6707f4d/
  check-runs`, `total_count`, counted 2026-10-24), the labelled mutation
  gate among them. `git log 4cadc2af..2781bb69` lists the commits; each
  message says what it changed and how it was verified. In sum:
  - the tool-calling study scored, audited by reading every answer, and
    published (`docs/tool-calling-study.md`, `34f579d`);
  - every dependency refreshed in all nine lockfiles, the Python
    requirements, Actions, Go, npm and Maven pins; MSRV 1.94, which sqlx
    0.9 requires (`7a73bdc`; `STABILITY.md` §5 allows the raise);
  - Prometheus recording and alerting rules (with `promtool` tests) and
    three Grafana dashboards (server, client, GenAI), built from a live
    Prometheus 3.15.1 fed by the repository's own binaries, directly over
    OTLP and through an OpenTelemetry Collector (`deploy/observability/`,
    `c372919`; GenAI and task-store panels, with screenshots in the README
    and the book, 2026-10-24);
  - a database `CLIENT` span and `db.client.operation.duration` for each
    operation of a store that names its database (`TaskStore::database`;
    the SQLite and PostgreSQL stores do), and `server.address` on a
    WebSocket client's spans (`Transport::server_url`), 2026-10-24;
  - OpenTelemetry: a `service.instance.id` per process (`755b121`), export
    wherever an endpoint is configured in every binary (`988fb69`), the
    tool loop's `GenAI` metrics and the rest of its `chat` span
    (`d118aaf`);
  - the spec ledger cites code by symbol at its definition line, and the
    checker verifies it (`6202a56`);
  - a re-audit of all 510 ledger entries against the v1.0.1 text found
    273 confirmed as written; its defects are being fixed in work
    packages. WP-A (server security: §13.3 schemes on the public card,
    §13.2 push credentials, push config `Debug`, the kid-optional signers
    removed, card `max-age` on WebSocket and gRPC, eleven security tests
    that now test what they are cited for) and WP-B (the client validates
    every object and card it receives against a2a.proto's REQUIRED
    fields; the gRPC conversion refuses absent ones; wire forms) are
    merged. So are WP-C (handler-wide conformance: the executor sees only
    card-declared extensions, `BadRequest` names the missing field, an
    empty push url is `InvalidParams`) and WP-D (SLIMRPC: every operation
    tested over the fabric, an equivalence suite against JSON-RPC, the
    push-config list fixed, retryable errors prefixed, the server's
    default telemetry, an `otlp` feature, on by default since the TLS
    conflicts below were fixed);
  - `examples/live-model` (2026-10-23): the SDK end to end against a real
    model through llama-server, ten checks (the tool loop streamed and
    signature-verified on all four bindings, provenance refusal,
    idempotency, budget exhaustion, delegation under a budget, halt,
    quorum). Its README records the runs, the model, the quantization and
    the llama.cpp build. The other model-backed examples were run the same
    day against the same llama-server (b11836, Qwen3-1.7B Q4_K_M, alias
    `qwen3:1.7b` on port 11434, release builds): `tool-loop-agent` (billing
    question: right, two rounds), `incident-response` (exit 0, Act 4 44 of
    44 cells, Act 5 16 of 16 with `A2A_TEST_POSTGRES_URL`), `genai-agent`
    (44 of 44, LLM leg exercised), `mcp-agent` (exit 0; 1 of 2 answers
    used the MCP tools, and the billing answer, which called none, is
    invented), `swarm` (`llm` scenario 32 of 32 jobs completed) and
    `rig-agent` (44 of 44 on its second run; its first lost SendMessage over
    WebSocket to the clients' 30 s default timeout, which `84d37e1` replaced
    with one 300 s `MODEL_CALL_TIMEOUT` per example on every binding).
    Logs were not committed.
  - §3.1.2 (2026-10-23): a `Message` an executor writes once its task
    stream has begun is sent as a status update carrying it, folded once
    at the queue's writer (`streaming/event_queue/message_fold.rs`),
    reviewed independently; a follow-up to an existing task folds its
    first `Message` too (the maintainer's decision, reviewed again).
- **`sign_agent_card` is deprecated, not removed** (2026-10-23): it shipped
  in 0.14.1, so `STABILITY.md` §3 keeps it for one minor release. It now
  refuses `key_id: None`, which wrote a header without the `kid` §8.4.2
  requires (a specification correction, §2), and goes in 0.16.0.
  `sign_agent_card_ed25519` never shipped and stays removed. No §3 waiver
  remains.
- **Built on this branch (2026-10-20):** ADR 0022's tool loop
  (`server::tool_loop`, feature `tool-loop`; `examples/tool-loop-agent`,
  whose README records the live run); the §3.5.2 holds, with the
  independent review's findings fixed (`store/holds`); a fix for the idle
  timeout cutting a blocking request still being answered
  (`serve::graceful::idle::Serving`); the dependency refresh (genai 0.7,
  tokio 1.54, no `deny.toml` waiver left at the root).
- **Encryption, verified three ways (2026-10-21):** the sealed formats
  read by Python's `cryptography` in both directions
  (`scripts/crypto_interop/check.py`, `tests/crypto_interop.rs`, a CI
  step); 37 NIST CAVP AES-256-GCM cases (the zip's SHA-256 and every case
  re-checked against the download, and every case re-decrypted
  independently); four fuzz targets on the unseal parsers, 600 s each, which
  found that a keyed audit digest accepted uppercase hex (fixed); and a
  mutation sweep of the crypto modules (210 mutants, the 2 missed rewritten
  so they are killed). `book/src/deployment/security.md`, "How this is
  verified".
- **Control map (2026-10-23):** 88 rows, every one citing a test or CI step
  that proves the row's `sdk` text; `check_compliance_map.py --check` fails
  on a row that cites nothing (`UNCITED` is empty: AIA-6 cites the book's
  "Is Your Agent High-Risk?" through `DOC_CLAIMS`, AIA-53 cites
  `check_no_model_artifacts.py`) and on a document that stops saying what
  `DOC_CLAIMS` quotes. All 187 cited tests were run 2026-10-23,
  PostgreSQL-ignored ones included, all passing. The checker verifies that
  a cited test exists, not that it passes: rerun them after editing a cited
  file. On 2026-10-23 the requirement text of every AI Act row claiming SDK
  support (12-1, 12-2, 12-3, 13-3f, 14-1-3, 14-4a, 14-4d, 14-4e, 14-5,
  15-4, 15-5, 19-26-6, and the unamended parts of 50-2 and 72-2) was
  compared with Regulation (EU) 2024/1689 as published (OJ PDF, SHA-256
  `bba63044…`, the same file as 2026-10-15, fetched with `curl -H 'Accept:
  application/pdf' -H 'Accept-Language: eng'
  http://publications.europa.eu/resource/celex/32024R1689`; EUR-Lex
  answers 202): all consistent, Art. 14(5)'s two-person verification and
  its law-enforcement exception included. What the rows say Regulation
  (EU) 2026/1744 changed was not re-checked: that text is not fetchable
  here.
- **Built on this branch (2026-10-22):** kill-switch conformance,
  `conformance::halt::{check_store, check_handlers}` (feature
  `conformance`): 8 store checks and 9 end-to-end replica checks, tested
  against deliberately broken stores; the in-memory, SQLite and PostgreSQL
  stores pass (`tests/kill_switch_conformance.rs`). This completes the
  2026-10-21 safety list except the upstream drafts, which wait on the
  maintainer.
- **Built on this branch (2026-10-22):** the prompt-injection containment
  harness (`crates/a2a-protocol-sdk/tests/injection_containment/`,
  `docs/injection-containment.md`): a model that obeys every instruction
  it reads, 8 scenarios a control contains and 3 limits asserted as limits;
  cited by the control map's ASI-MODEL row. Chosen by the maintainer over a
  live-model susceptibility study (2026-10-22).
- **Built on this branch (2026-10-22):** ADR 0027's provenance wiring:
  `with_content_signer` signs what the server serves (`src/provenance.rs`,
  the stream reader, push, the unary handlers), `with_content_keys` on the
  server (`RequestContext::content_check`) and the client
  (`src/content.rs`, `ClientError::ContentNotVerified`), and
  `Profile::Production` requiring a signer. An independent review found 12
  problems; the 9 fixed and the limits left are in ADR 0027 (including an
  unreleased flaw in `verify_content`). Verified at `69163dec`: all 169
  server test targets with all features (1,221 passed, 114 ignored), run 5
  at a time because 15 at a time filled the disk at link time. That
  commit's message says 22 hand mutants and 170 citations; recounted, they
  are 21 and 169. Found on the way, and fixed
  2026-10-22: metadata on an appended artifact chunk was dropped by the
  SQLite and PostgreSQL stores (`tests/artifact_append_metadata.rs`).
- **Built on this branch (2026-10-22):** ADR 0026's tool-call idempotency:
  `ToolInvocation::idempotency_key`, `Tool::read_only`, and a repeated call
  to a side-effecting tool answered from the first
  (`tool_loop/idempotency.rs`); `Delegation::start` keys the child's
  message when the agent's card honours keys.
- **Built on this branch (2026-10-21):** ADR 0025's production profile:
  `Profile::Production` (`profile.rs`) and `ServeConfig::production()`
  (`serve/graceful/production.rs`, `ServeReport::refused`). Not covered:
  the gRPC and WebSocket listeners. Since ADR 0027 the profile also
  requires a content signer.
- **Built on this branch (2026-10-21):** ADR 0024's pushed halts and
  cancels (`store/halt/postgres.rs`, `handler/halt/sync.rs`).
- **Built on this branch (2026-10-21):** ADR 0023's budget, G5 in
  `docs/swarm-orchestration.md`: `a2a_protocol_types::budget` (`Budget`,
  `BudgetStatus`, `BudgetHandle`), every server holding a turn to the
  caller's deadline (`handler/budget.rs`), `Delegation::start_budgeted`,
  and the tool loop charging the `tokens` meter. The 0.10/0.12
  `max_stored_tasks` deprecations are removed. `webpki-roots` requires
  1.0.10 (RUSTSEC-2026-0381).
- **Trailers on `main` (for the maintainer):** 36 of the 86 non-merge
  commits pull request #173 merged (`86c8fac..6c90d0b^2`) carry
  `Co-Authored-By` and `Claude-Session` trailers, against `CONTRIBUTING.md`'s
  "no other trailer" rule, and the `Co-Authored-By` line names a model.
  Counted 2026-10-20 with `git log -1 --format=%B` per commit. `main` is not
  rewritten; whether to leave it is the maintainer's call.
- **The spec ledger:** `python3 -I scripts/check_spec_ledger.py` is the
  authority on the counts (2026-10-24: TESTED 455, APPLICATION 32,
  NOT_APPLICABLE 19, PARTIAL 4; 510 entries). Re-audited 2026-10-24: the 20
  NOT_APPLICABLE entries were read again against the v1.0.1 text by an
  independent reader (18 agreed; §1.4's migration-guidance MUST at line 112
  moved to TESTED, with a drift test; Appendix A's note rewritten). The four PARTIAL entries, each short for
  a stated reason: §3.5.2, a stream lagging
  past the held-events ceiling (8 × `max_events_per_task`, 4,096 events by
  default) ends with `streamLagged`; §5.7, the specification contradicts
  itself on an empty `ListTasksResponse.tasks`; §12.4, agntcy-slim-rpc
  carries no error details (maintainer: stays PARTIAL until it does);
  §7.6.1,
  a message to a task in AUTH_REQUIRED is refused while its executor still
  runs (recommended: document that the executor returns, PARTIAL naming
  the conflict between the section's two SHOULDs). The APPLICATION and
  NOT_APPLICABLE entries were reviewed on 2026-10-20 (*Next*, item 1).
- **Ledger citations:** since `6202a56` every Rust citation is
  `path:line Symbol` at the symbol's definition line (non-Rust:
  `path:line "fragment"`), and the checker verifies that the symbol is
  defined at that line. After a code move, `check_spec_ledger.py
  --fix-lines` re-points the citations it can; where a symbol is defined
  twice in a file (`TryFrom` both ways) it cannot, and each must be
  re-pointed by hand to the same impl it named before (2026-10-23: 23
  after WP-B, each checked against the old file).
- **CI on `main` at `2781bb69`** (the fifteen push workflows, read with
  `gh api repos/tomtom215/a2a-rust/actions/runs?head_sha=…`, 2026-10-24
  13:40 UTC): DCO, Scorecard, ITK, OSV-Scanner, Verification, Deploy
  Documentation, Official TCK, Soak, ACTS, Fuzz, TCK and CodeQL passed; CI,
  Coverage and Benchmarks were still running. The scheduled OSV-Scanner
  runs failed on 10-21, 10-22 and 10-23 at head `6c90d0b`
  (RUSTSEC-2026-0381/0382/0383: rustls-webpki 0.103.15, webpki-roots and
  webpki-root-certs 1.0.9, in four lockfiles) and on 10-24 at head
  `4cadc2a` (RUSTSEC-2026-0387: crypto-bigint 0.7.5 in the SLIMRPC
  lockfile). Each cause was read in the job log; #179 fixed all of them
  (`8d7e878` the last) and OSV-Scanner passed on the merge.
- **Commits:** authored `Tom F. <tomf@tomtomtech.net>` with `Signed-off-by`,
  no other trailers (`CONTRIBUTING.md`, `PROVENANCE.md` §3.2).

## Next, in order

1. **The APPLICATION and NOT_APPLICABLE entries** were reviewed twice, the
   second time on 2026-10-24 by independent readers (32 and 20 entries
   against the v1.0.1 text and the code): no APPLICATION entry changed
   status (29 of 32 agreed; three notes were stale and are corrected), and
   NOT_APPLICABLE lost one (§1.4's migration-guidance MUST, now TESTED by
   `book-tests/tests/upgrading_guide.rs`). Built from the findings: the §5.2
   check is public (`RequestHandler::undeclared_binding`,
   `warn_if_undeclared`) and `SlimRpcServer` calls it; every listener's call
   is pinned by a test; the lint's IANA allowlist lost four tokens that are
   not schemes. Still not built: a plaintext warning on the WebSocket and
   gRPC listeners (only `Server` has one) and the ADR 0025 refusal for
   them, `CombinedDispatcher` and SLIMRPC. `spec_line` N is line N + 31 of
   the vendored file (the header ends at line 30, then one blank line).
2. **The four PARTIAL entries cannot all be closed from this repository.**
   §12.4 waits on agntcy-slim-rpc (3.4.6, the newest release, drops error
   details; draft `docs/upstream/agntcy-slim-rpc-error-details.md`); §5.7 is
   a contradiction inside the specification (a REQUIRED array that a
   no-match listing leaves empty; draft
   `docs/upstream/a2a-required-empty-array-report.md`); §3.5.2 is short by a
   design limit (a stream lagging past the held-events ceiling ends with
   `streamLagged`); §7.6.1 is short by a design conflict between the
   section's two SHOULDs (an executor still waiting for an out-of-band
   credential cannot also take the next message; closing it means an
   executor mailbox, an ADR and a decision). The TCK baseline was re-read
   against a2a-tck `d4d6fae` on 2026-10-24: all seven still fail on a defect
   of the suite (`official-tck-findings.md` §25), and upstream already
   tracks CORE-SEND-003 as issue #202. MAY entries relabelled SUPPORTED are
   not started.
3. **OpenTelemetry, still open:** `error.type` on a store span is the A2A
   code or `unavailable`, not the database's SQLSTATE, which the
   conventions prefer; carrying it would need the stores to keep it past
   the `A2aError` conversion. Done on this branch: store spans and
   `db.client.operation.duration`, the GenAI and task-store dashboard
   panels, `server.address`
   (from `Host`) and `network.peer.*` on server spans, a test that an
   exported log record carries its span's trace and span ids
   (`telemetry_export`), push `http.client.request.duration`, gRPC
   conversion failures recorded as failures, the tool loop's `GenAI` metrics,
   JSON-RPC client span attributes, the card-fetch and webhook-receiver
   spans, delegation spawns kept in the parent's trace, and default
   metrics that follow the global meter provider at each measurement; the
   task store a handler builds for itself reports to the handler's `Metrics`
   (`handler/default_store_metrics_tests.rs`), and a tenant refusal is
   counted as an answered error by all ten methods
   (`handler/tenant_refusal_metrics_tests.rs`; without it
   `A2ARequestsUnanswered` fired on refusals).
   **Open from the 2026-10-24 wiring audit** (a read-only static reading of
   the tree at `9a14939`; no binary was run, so each is a reading, not a
   measurement; the report is not in the repository). Fixed since: `live-model`
   starts `Telemetry` (H1), the SLIMRPC binding and `itk` ask for the
   client's `otel` (H4, M13; `bindings/a2a-protocol-slimrpc/tests/
   client_telemetry.rs`), the exemptions (`a2a-tck`, the MCP stdio server,
   benches, codegen) are recorded with reasons and enforced by
   `scripts/check_otel_wiring.py` (H2, M14, L2, L13), and the two stale
   README claims are corrected; `with_additional_metrics` keeps the
   OpenTelemetry default beside a custom `Metrics` and `agent-team` and
   `resilient-agent` use it (M1), and `incident-response` puts the global
   meter provider back after its two OpenTelemetry checks (M2); the client
   records a call as it ended for the caller: a dropped call and an
   early-dropped stream are `cancelled`, a refused answer is a failure (M9 a,
   b, d; `call_span/in_flight.rs`). Still open: the SLIMRPC multicast fan-out has
   no span or duration (H3); WebSocket upgrade refusals (`Origin`,
   `A2A-Version`, timeout) and oversize-message closes are logged but not
   metered (M6, the rest of it; M5 and the binary and busy answers are
   fixed); SLIMRPC refusals before the span (M7: they need `record_unrouted`
   and `RpcSystem` public, an API decision) and the refusals tonic itself
   makes (an oversize message, an unknown method, the connection limit; M8: a
   tower layer that can tell them from calls the service ran) record
   nothing; client
   retries are not visible (M10); refusals before anything is sent
   (undeclared capability, malformed message, a failing `before` hook such as
   a token provider) are not recorded by decision, not being RPCs (M9 (c);
   the token exchange itself is M12); push-config, audit, halt, lease and retention stores have no DB
   span (M11); card fetch, JWKS and token exchange have no duration metric
   (M12); the per-operation by binding matrix (M15) is now asserted for all
   eleven operations on JSON-RPC, HTTP+JSON, gRPC and WebSocket
   (`tests/operation_matrix`; it found the gRPC `with_custom_transport` gap,
   fixed), but not for the axum `A2aRouter` or SLIMRPC (SLIMRPC's own
   `tests/telemetry.rs` reads SendMessage and GetTask only); and the low items L1, L3 to L12 (an
   `OTEL_*_EXPORTER=prometheus` env that panics every binary, no flush on
   SIGTERM for four binaries, a stale `RpcCall::duration` doc line, and the
   rest).
4. **Mutation testing.** The full sweep deals `a2a-server` across 60 shards
   and the incremental gate across 20 (`mutants.yml`, measured from runs
   39193157308 and 39343090044). The three 2026-10-19 survivors were
   re-verified killed on this branch by hand mutants (2026-10-20). Read the
   next scheduled sweep's logs.
5. **Official TCK** after any wire change: `--min-graded 88` full, 66
   minimal. Last run 2026-10-20 at `1305674b`, after the auth-ordering,
   streaming-span and WebSocket trace-context changes, against a2a-tck
   `5daf014` (the workflow tracks `main`; the seven baselined failures were re-read against `d4d6fae` on 2026-10-24, `official-tck-findings.md` §25): full 88 graded, 81 passed, the
   9 baselined (requirement, transport) failures and no other; minimal 66
   graded, 8 of the 9 baselined failing (STREAM-SUB-003 not graded there);
   required-extension CORE-CAP-004 passed on JSON-RPC and HTTP+JSON. All
   three gates exit 0. Re-run after any further wire change.
6. **After 0.15.0:** `benches/sdk-comparison/harness/agent-a2a-rust` is pinned
   `=0.14.1` and the results under `benches/sdk-comparison/results/` were
   measured at 0.14.1, so moving the pin means re-measuring
   (`benches/sdk-comparison/README.md`); decide whether to publish the SLIMRPC
   binding (it is not on crates.io, and `book/src/bindings/slimrpc.md` shows a
   path dependency until it is). Then debt, starting with the changelog's size
   (*Open engineering* below).

## Waiting on the maintainer

- **ADR 0028: whether Tokio stays mandatory on `wasm32-unknown-unknown`.**
  Proposed, not accepted. It asks you to qualify ADR 0003 (Tokio is mandatory
  on native targets; on wasm32 the crates assume no Tokio runtime) so that a
  runtime seam can be landed as phase 1, behaviour-neutral on native and
  proven so by the lib suite, with a wasm32 `cargo check` and a lint on the
  trap APIs in CI. Phases 2 to 4 (a dispatch entry point that does not take
  `hyper::Request<Incoming>`, the client over `fetch`, Durable Object or D1
  stores) are unbuilt. Not measured: anything on Cloudflare's network, the
  1-second startup limit, durable state, a fan-out Worker. If you decline, the
  Worker-in-front-of-a-native-agent deployment needs no change here.
- **File GHSA-hr9h-6jvf-wvg6 with RustSec** (found 2026-10-21 by the CRA
  review). The project's own advisory (fixed in 0.14.1) is on GitHub only:
  the RustSec database and OSV have no entry (OSV queried 2026-10-21), so
  `cargo audit`, `cargo deny` and OSV-Scanner on an integrator's lockfile do
  not flag `a2a-protocol-server` or `a2a-protocol-sdk` below 0.14.1. CRA rows
  CRA-AI-I-2a, CRA-AI-II-2-4-8 and CRA-14 carry it as a gap. Filing is an
  upstream action, so it waits for the maintainer. The entry is drafted in
  `docs/upstream/rustsec-ghsa-hr9h-6jvf-wvg6.md` (file, front matter, the
  three decisions it leaves to you, and what was and was not checked).
- **Turn claims, built 2026-10-28 instead of compare-and-set writes.** The
  maintainer asked (2026-10-28) for replicas to work, superseding the
  2026-10-24 decision below. Built: a claim row per turn in the store's own
  database (`store::claim`, `handler::claims`), fencing every write of a turn
  by its token, cross-replica cancel through the claim, crash takeover after
  the time to live. It closes the lost update the compare-and-set design was
  for (the continuation waits for the claim and reads the task again) and the
  concurrent turns that design did not, without a `version` column or a
  change to any existing `TaskStore` method; the one addition is the provided
  `turn_claims` (default `None`). One decision for the maintainer: the
  release number (it adds public API and changes which sends are refused,
  so the book's definition of a patch does not fit it). The quorum gate's
  save, the other open question, now runs under the claim (reproduced as a
  lost approval first, then fixed).
- **A `v*` tag ruleset** is unset (a repository setting; `ROADMAP.md` tracks
  it). Without it, a tag on an unreviewed commit whose workflow has been
  edited is what `release.yml`'s validation cannot stop, because the workflow
  runs from the tagged tree (`.github/release-signers/README.md`).
- **Code-scanning alerts on `main` (2026-10-25: six open, read from a
  screenshot; the alerts API is not readable from a session).** Two are
  OSV-Scanner's `idna` (CVE-2026-45409, fixed in 3.15): the peers job scans
  `itk/agents/python/requirements.txt` as written, which had no `idna` floor
  and resolved to 3.9.0, while the image installs `requirements.lock`, which
  pins 3.20. The floor is added and Scorecard's Vulnerabilities alert should
  close with them once this reaches `main` and the scans rerun. Afterwards a
  recursive `osv-scanner scan source -r .` (nine `Cargo.lock` files, the Go,
  Java, JavaScript and Python peers, the CI requirement files) reports no
  issues, against the OSV data of 2026-10-25. The other three are Scorecard
  process findings that no code change closes. Branch-Protection: `main` has
  no active rule; the `protect-main` ruleset (created 2026-03-16) is
  `disabled` and holds only `deletion` and `non_fast_forward`. A rule that
  requires pull requests or checks would also need `github-actions` as a
  bypass actor, because the benchmark workflow pushes to `main` directly.
  Code-Review: Scorecard wants an approval from someone other than the
  author, and there is one maintainer. CII-Best-Practices: the badge is registered at the passing level (project
  15899); Scorecard's documentation scores passing 5 of 10, so the alert may
  stay open, and the next scan of `main` will show.
- **Upstream drafts to send**, all in `docs/upstream/` (its README lists
  each with its status), held on 2026-10-15 for re-verification. The a2a-go
  draft for R19 is deliberately not committed: it was never checked against
  a2a-go's source. Nothing is filed upstream without asking.
- **A private advisory to a2a-python** (GHSA, filed by the maintainer; see
  `handoff-history.md`, `claude/nifty-fermat-roj04f`). Do not file anything
  public about it while it is open.
- **§3.1.2, decided (2026-10-23):** the maintainer chose the spec's
  reading that a follow-up to an existing task has no direct reply (§3.1.2
  "no task tracking", §3.7 "Status Messages"): a turn that continues a task
  folds every `Message`, its first included (`create::continues`,
  `message_fold`), so the follow-up's stream and every subscriber's carry
  the same status update; one answered with messages alone and no state is
  re-parked where it waited (`TerminalTracking::repark_if_silent`; the
  review found the task otherwise left `Submitted`, which the SDK's client
  read as an incomplete stream and `Delegation` as `Lost`). Ledger 206 is
  TESTED. Left by design: a client
  that found a brand-new task's id and subscribed before its first-event
  `Message` (a direct reply, as §3.1.2 allows) receives it bare. Also
  decided in the fold and open to revisiting: a message written while the
  task is parked (`input-required`, `auth-required`) replaces the stored
  status message (the prompt, or §7.6.1's explanation) as a client applying
  the update would; the size limit applies to the status update, about 150
  bytes more than the bare message; the fold uses the state the executor
  last wrote, so after a status the processor refused (task stored
  `Failed`) a later message still streams in the refused state.
- **SLIMRPC `otlp` is on by default** (2026-10-23). The two conflicts that
  kept it opt-in are fixed: the server's OTLP/gRPC dials TLS through its own
  rustls connector, so tonic's TLS is not linked for it (with it linked,
  tonic refused SLIM's `https://` dial and `remote_node_tls` hung, re-measured
  with a 300 s bound); and the binding's builders install SLIM's rustls
  provider when the process has none (`install_crypto_provider`). The
  SLIMRPC error-details draft (`docs/upstream/agntcy-slim-rpc-error-details.md`,
  every citation re-read against agntcy-slim-rpc 3.4.6) is ready for the
  maintainer to file.
- **Publishing:** the SLIMRPC binding has never been published (0.7.0).
  `tck/sut` and `examples/mcp-bridge` are `publish = false`; CLI binaries
  wait on `vars.PUBLISH_CLI_BINARIES` (`packaging/README.md`).

## In flight outside this repository

- **adk-rust:** a patch on `claude/adk-rust-0.12-patch` (`6fbdd2f`), built
  against 0.12; it needs porting to 0.15 and a session with access to that
  repository. `adk-server` is the only external crate depending on ours.
- **rig #391:** a comment drafted, not posted. **AutoAgents #52:** a comment
  posted 2026-09-12, unanswered when last checked (2026-09-13).
- **`claude/a2a-rig-held`:** a rig integration crate, held unpublished
  (*Decisions* below); built against 0.12.

## Open engineering, not committed

None of this is in `ROADMAP.md`. Each line names where its evidence is.

- **Cross-replica, after turn claims (2026-10-28):** streaming on a replica
  not running the task is still fed by polling the event log every
  `subscribe_reattach_interval` (250 ms); `LISTEN`/`NOTIFY` could push it,
  unbuilt. A cancel reaches another replica's executor at the owner's next
  renewal (1 s), not at once, unless a halt store pushes it. Measured through
  a local proxy adding latency and cutting links (*Now*); not measured: a
  real network. Through PgBouncer 1.26.1 in transaction mode
  (`max_prepared_statements = 200`), 2026-10-29, the claim, replica and store
  suites pass (book, *Behind PgBouncer*), the upgrade guard's included; the
  one failure was `ALTER DATABASE … SET a2a.legacy_claim_ttl` not reaching
  backends the pooler opened before it, now documented, and the test sets it
  before anything connects. A send and a read
  now make 19 round trips (new task) or 25 (continuation), down from 45
  (CHANGELOG *Fixed*): 8 and 11 statements, the ping sqlx makes as each
  connection returns to the pool, and a ping before a connection idle a
  second or more (`store::pg_retry`, whose module docs say why each kind of
  statement may run twice). `tests/statement_count.rs` (CI, through
  `run_postgres_suites.py`) fails when the 8 or the 11 changes, counting
  round trips that run a statement on the wire. `scripts/check_pg_retry.py` (a CI step) fails
  on a statement on those pools outside `pg_retry::retry` or `checked`, and
  checks every PostgreSQL module by default: one that runs on a pool of its
  own with sqlx's ping is listed in its `EXCLUDED` with the reason. It reads
  the source, not types: a statement reached through a helper that takes the
  pool as a plain argument is caught only if the helper's own file is in
  scope, which every file under `store/` with `postgres` or `pg_` in its
  path is. A turn's event writes are
  retried while the store is unavailable, up to `store_write_retry`; during
  an outage sqlx itself waits in `acquire` (30 s by default) before any
  statement fails, so that retry is what covers statement-level failures (a
  timeout, a lock, a deadlock) and outages past the acquire timeout. An
  executor that fills its queue meanwhile waits as long as that retry
  (`HandlerLimits::store_write_retry` says how long one write can take, about
  a minute with the defaults). Not solved: an outage longer than the retry
  loses the event being written and then spends the whole retry again on
  each event behind it, so a queue of N events drains in N × 30 s while the
  store is down, and the executor's `Failed` status, if its own write gave
  up, can be dropped, leaving the task unfinished in the store with nothing
  to finish it (`write_failure_event` logs and moves on). A breaker that
  stops retrying once one write has used the whole budget, and a direct
  store write of the failure, are the candidates; neither is built. The
  live scripts behind these figures (round trips, outages, partitions, the
  upgrade, the soak matrix) are in `scripts/live/`, with a README saying what
  each needs; `tests/transient_store_errors` holds the CI-sized outage
  cases.
  From the review, left as
  they are: a send to a task whose executor was cancelled but
  is still winding down is refused at once rather than waited for (the
  cancel paths store a terminal state, after which it is refused anyway; not
  tested); a continuation of a task parked in this process waits about one
  20 ms retry for the claim's release, which runs on a spawned task.
- **AES-GCM nonce budget is per process** (encryption verification,
  2026-10-21, medium, design). The 2^32-seal limit per key is counted per
  process and resets on restart, so R replicas can seal R × 2^32 under one
  key: a repeat probability R² times the NIST target (R = 10: 1.2e-8). The
  proposed fix seals each value under an HKDF-derived key with a random
  salt, marked by a leading `0x00` byte no current value can start with;
  it is a format change that needs a two-step rollout (readers first,
  then writers behind a flag), so it waits. The book tells deployers to
  give each replica its share of the limit meanwhile.
- **Tool loop:** spans record no content, because the handler's
  content-capture setting does not reach an executor (the caller-facing
  `Failed` message no longer carries the provider's error body, 2026-10-24).
  `CONTRIBUTING.md` says twenty public traits with every feature and fifteen
  by default (2026-10-24, from `cargo doc`'s `trait.*.html`; a grep of `pub
  trait` gives 21, one of them unreachable).
- **Privacy:** audit protection (`AuditLog::with_privacy`) leaves what its
  module docs list: an erased person's records stay linkable, keys survive
  in backups and logs, erasure is not serialised across replicas.
  `erase_caller` (2026-10-18) removes a caller's partitions but not what a
  caller left in a tenant's shared partition. `EncryptedTaskStore` leaves
  ids, states, timestamps, `metadata` and order unprotected, and its seal
  limit is per process while the true budget is per key across processes.
- **Measurement:** `docs/readiness-bar.md` records no measurement of its rows
  R1 to R7, C1 to C5 and F1 to F5 (WS3, WS4, WS5: each unmet, not assumed);
  the 0.9–1.8 µs `ListTasks` gap at 10K tasks is unmeasured. The send
  slowdown is measured (2026-10-24, pinned core, interleaved rounds, five
  runs; the section "the CPU cost of a blocking send"): not slower than
  0.14.1 (102.0 against 106.2 µs), 11.5–16.0% slower than `6ce5dc9a`, which
  was 13.9% faster than 0.14.1; this branch's share is inside the noise
  (+1.4%, 12 rounds); default telemetry is 4.6–6.6% of a send. Cause of the
  rise since `6ce5dc9a`: not isolated.
- **Interop:** `itk/interop/card_signature_interop.py` checks Python only.
  `scripts/go_sdk_interop.sh` last passed both directions 2026-10-20 at
  `4101bb55`, after the auth-ordering change: leg 1 52 checks, leg 1b
  38, leg 2 33 cells exercised (11 WebSocket cells not applicable), 0
  failures.
- **Control map:** 15 supported, 4 documented, 29 partial, 0 gap, 40
  integrator's duty, of 88 rows (the table at the top of the generated
  `docs/compliance/control-map.md`, counted from `controls.toml` 2026-10-24;
  `check_compliance_map.py` prints no counts). This line said 13, 2, 11 and
  22 until then, which matched no version of the file we could find. The
  partial rows' remaining gaps are in each row's `gap`. Narrowed on
  2026-10-24 (no row changed status): task deletions, erasures and legal
  holds are audit records (GDPR-5-2) and the SQLite stores zero deleted
  content in the file (CRA-AI-I-2m). Not built, and the reason each: a
  per-caller export (GDPR-15-20); a bounded cancel deadline (AIA-14-4e);
  sealing `metadata` in `EncryptedTaskStore` (CRA-AI-I-2e); erased audit
  records unlinkable from each other, which needs a per-record handle, a
  format change; erasure serialised across replicas, which needs the
  compare-and-set writes of 0.16 (decided); attribution of a tenant's shared
  partition, which has no caller to attribute it to. A reading of each of
  the 29 partial rows' `gap` (2026-10-24, a judgement, not a check) found
  none that SDK code alone moves to supported: the gaps are an integrator's
  or a model provider's (AIA-15-4, AIA-15-5, AIA-50-2, CRA-13-16-18,
  CRA-AI-I-2b, -2d, -2f, -2h-2i, -2j, -2k, -2l, GDPR-5-1f-32-1b, ASI03,
  ASI07, ASI08, ASI10), a default settled as off (GDPR-25-2), the
  protocol's (no method to correct or delete a task: GDPR-5-1d-16,
  GDPR-5-1e), something only the maintainer can supply (the RustSec entry
  CRA-AI-I-2a and CRA-AI-II-2-4-8, a release key CRA-AI-II-7), or a feature
  that would narrow the row without closing it (GDPR-15-20, GDPR-18-21,
  CRA-AI-I-2e, CRA-AI-I-2m, ASI04, AIA-14-4e, GDPR-5-2).
- **Release bar, measured 2026-10-24 on the final tree** (after the `InFlight`
  restructure, the last code change; each line is a run, not a reading):
  `cargo semver-checks` in CI's form (`--all-features`, four crates) exits 0;
  with `--release-type patch` (the default skips a 0.14 to 0.15 bump as
  already major) against the published 0.14.1 it reports 7 major-level lints
  and 5 minor-level ones, the SDK crate none. The 7 are the six entries of
  `### Breaking Changes` in `CHANGELOG.md` (`ClientRequest.query_params`,
  `SseParseError` non-exhaustive, `Server` no longer `UnwindSafe`,
  `TenantLimits.max_stored_tasks` removed, which is three lints with its two
  builder methods, `TaskState` discriminants, `ConvertError`
  non-exhaustive); the 5 are deprecations. This line said 6 major-level
  differences until the lints were listed rather than counted. Also run:
  clippy (workspace all features and default, `--each-feature` on four
  crates), tests (workspace all features: 6089 passed, 0 failed; the client
  and SDK crates with no, default and all features), docs (both forms), the
  SLIMRPC binding (fmt, clippy in two forms, 133 tests, the in-process
  example), `itk` clippy, the four offline examples, `cargo deny` as CI runs it
  (the workspace and the binding), `cargo package` as CI runs it, `cargo check`
  of the four crates on Rust 1.94 (all targets, all features),
  `prove_workflow_gates_fail.py` (30 proven, 0 unproven, 23 exempt of 53),
  and `scripts/go_sdk_interop.sh` (both directions, 33 cells exercised, 11
  not applicable, 0 failures). The incremental mutation gate's invocation
  (`mutants.yml`, `--in-diff` against `origin/main`, with PostgreSQL) was run
  locally on the whole diff: the client has 39 mutants (12 caught, 27
  unviable, 0 missed), the server 82 (12 caught, 70 unviable, 0 missed, 0
  timeouts), the types crate none. Not run locally: the official TCK, ACTS,
  the Windows and macOS legs, the full mutation sweep, and
  `prove_gates_fail.sh` in full (the one gate added this cycle was proven when
  it was added). The CI run of the pull request is the check of those.
- **Gates:** a mutation shard that starts after its PR merges grades
  nothing (low); the Windows stress-test connect failure, cause unknown; the
  benchmark comparison harness pins `=0.14.1`, the newest release on
  crates.io (checked 2026-10-20), and moves when 0.15.0 is published;
  `prove_workflow_gates_fail.py` needs `ssh-keygen`: a fresh cloud container
  has none, and `apt-get install openssh-client` finds no candidate until
  `apt-get update` has run (it installed 1:9.6p1 after one, 2026-10-24).

## Waivers and their removal tests

No waivers anywhere since 2026-10-23. The SLIMRPC binding's `deny.toml`
ignored the yanked `mls-rs-crypto-awslc` 0.25.1 until agntcy-slim-mls 0.5.1
accepted 0.26; a plain `cargo update` in the binding then took
agntcy-slim-rpc 3.4.6 and kept rustls at 0.23.46 (its comment has the
history). The root `osv-scanner.toml` dropped RUSTSEC-2023-0071 when sqlx
0.9 removed `rsa` from every lockfile. Before updating the binding, still
read `cargo deny check advisories` after the update, not before.

## Decisions the maintainer made

Kept under this heading because `release.yml` and `packaging/README.md` cite
it.

- **2026-11-01, answers to `docs/audits/2026-11-01-triage/DECISIONS.md`** (all built or recorded as
  below; the forks are described there):
  - **F1, 0.16.0 breaks: all five.** `#[non_exhaustive]` on the generated wire structs (G-10),
    `tls-rustls` as a default feature of the server crate (G-06), `RequestContext.context_id` becomes
    `ContextId` (G-18), `AgentCard::new` defaults `streaming` to true (G-07), and `sign_agent_card` is
    removed (NEW-g-api-4). Each under Breaking Changes with a migration; a `**Cadence exception:**` line
    (0.15.0 was also breaking, in October).
  - **F2, loopback and proxies: exempt loopback by default**, `A2A_PROXY_LOOPBACK=1` to opt back in, and
    Go's `NO_PROXY` dialect (B-23).
  - **F3, bad `OTEL_*` values: warn and fall back**; only an unusable endpoint URL still fails.
  - **F4, Production profile: refuse** a process-local halt, lease, push-config or audit store when the
    task store hands out claims, with an explicit opt-out (amends ADR 0025).
  - **F5, delegation on shutdown: detach and resume.** With a journal attached, shutdown detaches; the
    resume streak resets on any event; stream keep-alive for gRPC and WebSocket; a journaled
    delegation can be leased and budgeted.
  - **F7, one HTTP pool: build ADR 0032's opt-in `HttpPool` now** (not the "document the `Arc` pattern"
    position the triage recommended).
  - **F8, container image: publish a signed image to GHCR from `release.yml`** with provenance (written
    here, cannot be exercised here), beside fixing the README and manifest.
  - **F9, Agent Skills: an ADR now, a feature-gated module in 0.17.0**, no script execution.
  - **F10, steering a running agent: an ADR and the extension URI now, the build after the audit PR.**
  - **F12, swarm designs: only the four fixes** (H16, NEW-h-designs-1, -4, -7) in this PR; each design is
    its own ADR and later PR.
  - **F11, changelog: a highlights block (at most 15 lines) checked by `check_release_tree.py`, and
    `### Internal` moved out to `docs/changelog-internal/`**, as its own commit at the end.
  - **F6, patch behaviour policy: no work.** The maintainer's answer: "we are planning to release
    v0.16.0, v0.15.1 was never released." Read as: the `[0.15.1]` changelog section (dated 2026-10-26,
    never tagged) is not a release and its entries belong to 0.16.0's notes; no patch gate is built.
    That folding is not done yet (group P); say so to the maintainer if it was not what they meant.
  - **F13 to F15 were not asked;** the positions in `DECISIONS.md` stand until told otherwise (F13
    accept and document the window, F14 a bounded wait in the stream reader, F15 certificate reload
    after 0.16.0 and deprecate `tls_config_with_extra_roots` now).

- **2026-10-28:** replicas must work without session affinity, and the
  release is not ready until they do and the observability stack works out of
  the box; every check is to be run live (llama.cpp with current small Qwen
  and Gemma models, a real Collector, Prometheus, Grafana, trace and log
  backends). Supersedes the next entry.
- **2026-10-24 (superseded 2026-10-28):** multi-replica high availability is
  not a requirement for 0.15.0. It shipped as a documented limitation
  (CHANGELOG "Known Limitations", the book's horizontal-scaling page).
- **2026-10-08:** commits are authored as Tom F. with sign-off; release tags
  are signed with the maintainer's own SSH or GPG key and CI verifies them;
  new distribution channels (the CLI, Homebrew, winget) are drafted in-repo
  only, and the maintainer publishes. The Langfuse preset and every page
  default to a self-hosted Langfuse and lead with self-hosting; Cloud is an
  alternative only.
- **2026-10-14:** every audit-register recommendation approved as written;
  R49, the types generated from the proto (ADR 0016); R12 as a one-shot
  migration per store (`migrate_caller_keys`), not reading old keys for a
  release; R67 option (b) with 0.15.0 as the next release; OpenTelemetry as
  in Step 4; R68, follow the text.
- **2026-10-15:** R69 option (a), the store reports the log position a saved
  task reflects. Later the same day, for the next session's plan:
  `subscribe_max_idle` off by default, kept as an opt-in (§3.1.6); lag
  catch-up paging, so `streamLagged` remains only where the log no longer
  holds the events (§3.5.2); `application/a2a+json` for a request that states
  no preference, gated on `scripts/go_sdk_interop.sh` (§11.1); the card
  resolver honours `max-age` with an always-revalidate option and an opt-in
  default lifetime (§8.6, §8.6.2); the §5.7 and SLIMRPC error-mapping drafts
  re-verified and held; for 0.15.0, UCUM units, `a2a.server.latency` renamed
  `a2a.server.request.duration` with the old name removed, and
  `init_otlp_pipeline` taking an optional service name — not the single
  outcome counter; commits carry `Signed-off-by` only, and
  `CONTRIBUTING.md` follows. After the a2a-go check failed (its streaming
  calls name no JSON type and it reads only `application/json` errors), the
  narrowed §11.1 rule: a request asking only for an event stream keeps
  `application/json` errors. A part-level at-rest cipher hook (AES-256-GCM
  on message and artifact part payloads, structure left plaintext) for
  GDPR-25-1 and GDPR-32-1a.
- **2026-10-16:** the audit trail protects who and what its records name,
  with per-person keys that can be destroyed (crypto-shredding), amending
  ADR 0015 — chosen over keyed digests alone and over deferring it.
- **2026-10-18:** the OTLP exporter stays in `a2a-protocol-sdk` only; the
  WebSocket listener gets an opt-in `Origin` allowlist (built,
  `with_allowed_origins`); an unidentified caller is rate-limited by its
  peer address (built); a total-task cap across tenant partitions is an
  option (built, `TenantStoreConfig::with_max_total_tasks`), and the
  approval-gate refusal under caller isolation stays.
- **2026-10-20:** §3.5.2: build a per-task watermark so the in-memory event
  log never trims what an attached stream still owes (bounded by a hard
  ceiling), with an independent review. §12.4: SLIMRPC stays PARTIAL until
  agntcy-slim-rpc carries error details; no details in the message text.
  Tool calling moves into the SDK as a feature-gated module (ADR 0022),
  superseding the earlier examples-only decision below.
- **2026-10-21:** production-grade safety work for delegation at scale,
  each built with an ADR, tests that fail without it, and an independent
  review where it touches the store/stream contract. Provenance: receivers
  verify present signatures by default and refuse altered content, signing
  turns on when a key is configured, unsigned peers still interoperate
  (marked unverified), and the production profile requires a key.
  Deadlines and budgets: an extension carrying an absolute deadline and
  named meters (tokens, tool calls, cost in a named currency), decremented
  per hop, with the remainder visible before expensive work. Production
  profile: builder presets (`RequestHandlerBuilder::production()`,
  `ServeConfig::production()`) that refuse to start on plaintext off
  loopback, an undeclared binding, no authenticating interceptor, or
  delegation without leases and a halt store; defaults unchanged. Instant
  halt: the Postgres halt store pushes halts and cross-replica cancels
  with LISTEN/NOTIFY; SQLite and in-memory keep polling. Also: narrower
  credentials per hop through RFC 8693/8707, idempotency keys for tool
  side effects by default, a multi-agent prompt-injection harness, the
  tool-calling study published, kill-switch conformance tests, upstream
  drafts for deadline propagation and provenance (not filed without the
  maintainer), the three encryption verifications, and the removal of the
  0.10/0.12 deprecations in 0.15.0. Built differently from this record in
  two respects: ADR 0023's deadline is relative (`remainingMs`, as gRPC's
  `grpc-timeout`), not absolute, so clock skew between agents does not
  move it; each hop's remainder is what its parent reserved. And the
  handler preset is `with_profile(Profile::Production)`, not
  `RequestHandlerBuilder::production()`, because profiles are how the
  builder already applies presets (ADR 0025).
- **Earlier, still standing:** ~~tool calling stays in the examples, not the
  crates (`examples/rig-agent`)~~ (superseded 2026-10-20, above); the rig integration crate is held
  unpublished; the cargo-mutants patch is kept in-repo, not proposed
  upstream (`scripts/cargo-mutants/README.md`); a store that cannot isolate
  refuses a request naming a tenant (G3 option 2); caller isolation is on by
  default with an authenticating interceptor (2026-10-12); the store cap and
  the terminal gate were kept as they are (2026-10-03, G4 in
  `docs/swarm-orchestration.md`); `deploy-agent` runs on jemalloc.
- **Rejected designs worth not re-proposing:** the halt writing `Canceled`
  itself, and re-checking the halt after the row is written (both
  2026-10-09); for §13.1, a composite tenant string, a nested future per
  method and a general `TaskAuthorizer` hook. The reasons are in
  `handoff-history.md` under each date.

## Release gotchas

`RELEASING.md` is the procedure; these are what it cost to learn.

- Regenerate the provenance manifest last, at the release commit:
  `release.yml` checks it against the tag.
- After the content merge, wait for the benchmark bot's commit before
  preparing the release; `benchmarks.yml`'s guard covers only the window
  after preparation.
- The eight inter-crate version pins are not what `release.yml` checks; it
  reads the first `version` line. `RELEASING.md` §1 has the loop over every
  lockfile and `check_lockfiles.sh`.
- The maintainer has allowed release preparation inside the content pull
  request, most recently for 0.15.0 (2026-10-25); the manifest-last rule
  holds either way.

## Branches

Only two kinds matter: the working branch (*Now*, above) and the storage branches (`claude/adk-rust-0.12-patch`,
`claude/a2a-rig-held`), both built against 0.12. Every other `claude/*` branch is merged or a pre-0.12 session branch;
deleting them is the maintainer's call. To check one:

```sh
git fetch origin && git merge-base --is-ancestor origin/<branch> origin/main && echo merged
```

This clone is shallow, so a "no" for an old branch is not proof, and `git
blame` stops at the shallow boundary. Never write this branch's own head SHA
into this file: the commit that records it changes it.

## Environment

- **The live stack of 2026-10-28, rebuilt in minutes.** `dockerd &` starts a
  Docker daemon (the binary is installed; nothing starts it). Then:
  `docker compose -f deploy/observability/docker-compose.yml up -d`;
  PostgreSQL 17 with `docker run -d --name pg --network host -e
  POSTGRES_PASSWORD=a2a -e POSTGRES_USER=a2a -e POSTGRES_DB=a2a postgres:17`;
  llama.cpp with `docker run -d --name llama --network host -v
  /opt/models:/models:ro ghcr.io/ggml-org/llama.cpp:server -m
  /models/<gguf> --port 11434 --host 127.0.0.1 --jinja -c 16384 -np 2 -t 4`
  (its image healthcheck probes 8080 and reports unhealthy; `curl
  localhost:11434/health` is the real one). GGUFs from Hugging Face by
  `curl -L https://huggingface.co/<repo>/resolve/main/<file>` and checked
  against the API's `lfs.oid` (`unsloth/Qwen3.5-2B-GGUF`,
  `ggml-org/gemma-4-E2B-it-GGUF`). Docker Hub answers 429 when busy:
  pull the same digest from `mirror.gcr.io/library/...`. A `docker build`
  that fetches crates needs this sandbox's proxy: `--network host`,
  `--build-arg HTTPS_PROXY=$HTTPS_PROXY`, and the CA from `/root/.ccr`
  through `--build-context` (a scratch Dockerfile; never the repository's).
  GitHub release downloads are refused; `go install` through the module
  proxy works (kind was installed that way). Kubernetes does not run here
  (kind: nested `runc` cannot start a pod sandbox).
- **Live-testing pitfalls met in this sandbox** (2026-10-28): the kernel has
  no IPv6 (no `/proc/net/if_inet6`; an AF_INET6 socket fails with errno 97),
  so nothing IPv6 can be dialled. The sandbox's `NO_PROXY` covers 127.0.0.1
  and curl applies it even to `-x`, so check a local proxy with
  `--noproxy ''`, and start test processes under `env -i` with only the
  variables meant. Squid (`mirror.gcr.io/ubuntu/squid`) cannot write
  `/dev/stdout` as its `proxy` user: log to `/var/log/squid/`. A second
  Collector on host networking collides on its own telemetry port 8888;
  publish ports instead. `docker compose up` does not recreate a container
  whose bind-mounted config changed: `--force-recreate`. A Collector on
  `localhost:4318` makes `tests/otel_pipeline_default_endpoint.rs` fail
  (it needs that port free). A debug build of the whole server suite needs
  about 14 GB of disk; `CARGO_PROFILE_DEV_DEBUG=0` brings it under 4 GB.
- **A model's thinking costs minutes on four CPUs**: Qwen3.5 reasons by
  default, and an uncapped call generated over 5,000 tokens at about 6.5
  tokens/s. Every model-backed example now caps output and turns thinking
  off unless told otherwise; anything new must too.

- **Disk** is a fixed allowance: `CARGO_INCREMENTAL=0`, and delete
  `target/semver-checks` and the incremental directories when space runs
  low. A linker "bus error" (signal 7), or `linking with cc failed`,
  means the disk is full. `bindings/a2a-protocol-slimrpc/target` reaches
  12 GB for its full test suite: run it with `CARGO_PROFILE_DEV_DEBUG=0`
  and delete it afterwards. `cargo test --workspace` fills the allowance
  while linking (2026-10-24: three gates failed with `No space left on
  device`, which a failed test run reports as a compile error): run one
  package at a time and delete the executables in `target/debug/deps` between
  (`find target/debug/deps -maxdepth 1 -type f -perm -u+x ! -name '*.so' !
  -name '*.d' -delete`; 14.7 GB of 20 GB were test executables). When the
  harness prints "Disk full ... 0 MB free", its output is lost: free space
  first, then re-run (a command that prints nothing, not even a trailing
  `echo`, is that symptom). `target/debug/deps` also keeps every build of the
  workspace crates, one per feature set and profile: `cargo clean -p
  a2a-protocol-types -p a2a-protocol-client -p a2a-protocol-server -p
  a2a-protocol-sdk` removed 10.6 GiB of 172 stale builds of the server crate
  alone (2026-10-24) and costs only those crates' recompilation. Do not run
  a script that prunes `target/debug/incremental` while another `cargo` is
  building in the same tree: the build fails with `failed to move dependency
  graph`.
- **`check_file_lengths.sh` counts tracked files only:** run it after
  `git add`, or a new file over 500 lines passes unseen (it did:
  `tests/provenance_signing.rs`, 533 lines, at `69163dec`).
- **PostgreSQL 16** is installed: `pg_ctlcluster 16 main start`, then
  `A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres`.
  The `postgres` role's password is not always `postgres` in a fresh
  container (scram auth refused it on 2026-10-20): set it with
  `su postgres -c "psql -c \"ALTER USER postgres PASSWORD 'postgres'\""`.
- **llama.cpp, in a fresh container (2026-10-24, none was installed):**
  `git clone --depth 1 https://github.com/ggml-org/llama.cpp`, `cmake -S . -B
  build -G Ninja -DCMAKE_BUILD_TYPE=Release -DLLAMA_CURL=OFF -DGGML_NATIVE=ON
  -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_TESTS=OFF`, `cmake --build build
  --target llama-server -j2` (about 6 minutes on 4 vCPUs; built at commit
  `3f03b17`). github.com release pages answer 403 through the proxy;
  huggingface.co is reachable, e.g. `unsloth/Qwen3-1.7B-GGUF`
  `Qwen3-1.7B-Q4_K_M.gguf` (1.1 GB, 7 s). The examples default to
  `localhost:11434` and the name `qwen3:1.7b`: start the server with `--port
  11434 --alias qwen3:1.7b --jinja -c 8192`. Earlier: b11836 (`f5027fb`) at `/opt/llama/src/build/bin`, and
  the study's b11751 (`8466395`) at `/opt/llama/old`; GGUFs in
  `/opt/models` (hashes in the study). Use `-c 8192` (the default context
  sized the KV cache to ~4.6 GB and the OOM killer took the server), and
  send one request at a time to a server whose results must replay. A
  replay is host-dependent: the same binary and seed gave different
  answers on a 2.10 GHz and a 2.80 GHz host (`GGML_NATIVE`), which is
  what the study's uptime gap turned out to be.
- **A local OTLP collector breaks one test:**
  `otel_pipeline_default_endpoint` expects its flush to the default
  endpoint to fail, and fails whenever something listens on 4317. Stop the
  `deploy/observability/` stack before running the server suite.
- **No Docker daemon** in the cloud container, so `itk/docker-compose.yml`
  cannot be run here.
- `pkill -f` and `pgrep -f` match their own shell's command line, which
  contains the pattern: kill by PID, or put the command in a script file
  (cost a killed shell twice on 2026-10-20 and twice more on 2026-10-24).
- Agents in worktrees that share `target/` race on a crate's `.rlib`
  (`linking with cc failed` while the other build replaced it): re-run the
  step before reading anything into it.
- `cargo test --workspace` stops at the first failing test binary; use
  `--no-fail-fast`, or one package at a time when the disk allowance cannot
  hold every test binary at once.
- `cmd | tail` reports `tail`'s exit status; use `PIPESTATUS` or a file.
- Run `rustup update stable` and nightly clippy before trusting local clippy;
  CI's stable moves.
- Measure performance on one pinned core, medians of six interleaved runs.
- Run `scripts/preflight.sh --full` on the merged tree before pushing a
  release-relevant change, and `scripts/go_sdk_interop.sh` before changing
  HTTP+JSON headers.
- Mutation testing: read the logs, not the counts; reproduce a CI shard with
  the "How to Reproduce Locally" block `mutants.yml` prints.

## Lessons

- **A sandbox run that fails oddly may be a full disk.** `cancel_keeps_concurrent_writes sqlite`
  failed once in a full run while `df` read 100%, and passed 3 of 3 alone and in the next full run. The
  target directory is about 8 GB and each side manifest (`fuzz/`, `itk/`, `bindings/`) adds 300-700 MB;
  `cargo clean -p a2a-protocol-server -p a2a-protocol-sdk`, `rm -rf target/nextest` and removing the side
  `target/` directories gets it back. Check `df` before blaming a test.
- **A codemod over hundreds of sites needs a fixpoint, a path allowlist and a comment rule.**
  Nested literals are not reached by one pass; `pb::Message` (prost) and a different SDK's harness matched
  the type names and had to be kept out; comments inside a literal vanish unless carried to the new call.
  Run it to zero conversions, then let the compiler list what is left. See the G-10 commit message.
- **A liveness check that compares a clock with the interval is decided by
  timer jitter** (found 2026-11-01 by a test that failed 22 of 40 runs on an
  idle machine). The WebSocket client's first keepalive asked "has the idle
  time reached the interval?" at each tick; right after a pong the idle time
  was 99 ms against a 100 ms interval, so that tick sent no ping and the next
  saw 200 ms and closed a live connection. At the 15 s default the same thing
  would have dropped healthy idle connections about every other timer run. It
  now counts quiet ticks (`Keepalive`, `transport/websocket.rs`): a ping on the
  first, close on the second, any frame resets. Count events, do not compare a
  duration with the period that produces the sample.
- **Restoring a mutated file by `shutil.move` of a backup leaves cargo with a
  stale binary.** The backup was copied before the mutant was written, so the
  restored file's modification time is older than the mutant's build output, and
  cargo does not rebuild: the next test run still contains the last mutant
  (`a2a_extensions_is_not_reported_as_a_dropped_header` failed with left 1,
  right 0 on correct source). Restore by writing the content back, or `touch`
  the file, and treat any failure that follows a mutant run as stale until a
  rebuild shows it.
- **A scoped tracing subscriber is not enough to capture a log line in a
  parallel test binary** (root cause of the 1-in-20 `log_redaction` flake,
  found 2026-10-24). `tracing-core` 0.1.36 keeps `has_just_one` true while
  one dispatcher is registered and then computes a callsite's interest from
  the first-reaching thread's own default, so another test's thread, with no
  subscriber, reaching the same `warn!` first makes it "never interested" for
  every thread. Two threads and two barriers reproduce it every time; 160
  runs of the real test did not, so repetition cannot find it. Capture
  through `crate::handler::warnings_during` or `warning_lines_during` (one
  global subscriber), never `with_default`, in a test that shares a binary.


- A mutation run's "caught" is only as good as the test that failed. Read
  the per-mutant logs in the shard artifacts (`mutation-report-*`,
  `outcomes.json` and `log/`): on 2026-10-22 an assertion-free `#[ignore]`
  measurement timing out at nextest's 45s kill had scored 245 of 610 server
  catches, so each labelled run surfaced survivors the last had scored
  caught (`640773fc`). A slow test that asserts nothing belongs in the
  mutation filter beside the soak binaries.
- A test passing and `cargo semver-checks` passing do not show a serde
  change preserves behaviour; parse fixed inputs with both builds.
- After code moves, re-point ledger citations with `--fix-lines` and
  check by hand every one it cannot place: the checker knows where a
  symbol is defined, not which of two same-named impls a citation meant.
- Count before writing a number into a document, and say what was counted.
- A test that reads spans through a thread-local subscriber needs a test
  binary of its own when other tests in its binary reach the same callsite
  with none: `tracing` caches interest globally. Rebuilding the interest
  cache was not enough (`tests/card_span_over_tls.rs`, 2026-10-23: 9 of 20
  runs failed beside the other TLS tests, 20 of 20 passed alone).
- Look at a dashboard before showing it to anyone. A short-lived process
  exports each series once or twice, so `rate()` over it tapers to nothing,
  and a process started without its `OTEL_*` environment exports nothing at
  all; both read as an empty or dying panel. Feed a screenshot from a
  long-lived process, confirm its series in Prometheus first
  (`count by (job, instance)`), and wait a full window (2026-10-24: the
  first capture had both faults, and a client legend that exposed the
  WebSocket transport's missing `server.address`).
- A `compile_fail` doctest needs a control that compiles.
- A re-run after a fix must cover the binary that failed, not only the files
  edited.
- An independent review before committing found two defects in R69 that the
  change's own tests and 23 hand mutants did not; budget for one on any
  change to the stream or store contract. It did again on 2026-10-23: a
  per-reader §3.1.2 fold passed its tests and broke §3.5.2 on resume; the
  writer fold that replaced it was reviewed the same way, and the review's
  findings (a failed write still setting the fold's state, the agent span
  ending at a mid-task message, the audit record losing the message id, a
  revert leaving `history: []`) were fixed with a test each.
- The workspace suite and the all-features server suite in one target
  directory exceed this container's disk allowance; delete `target/debug`
  between them (2026-10-23). Building with `CARGO_INCREMENTAL=0
  CARGO_PROFILE_DEV_DEBUG=line-tables-only
  CARGO_PROFILE_TEST_DEBUG=line-tables-only` keeps the server's ~180 test
  binaries well under it; full debuginfo filled it at 14 GB of `deps`.
- Run every clippy invocation CI runs, not only `--all-features`: a type
  used by an always-compiled store from a module gated on `sqlite` built
  with all features and nowhere else (2026-10-18).
  The 13 named invocations are not all of it: `cargo hack clippy
  --each-feature` (57 runs) and `cargo doc -p <crate>` per crate, outside
  the workspace's feature unification, caught a dead function and a link to
  a `signing`-only method on pull request #178 that every named run passed
  (2026-10-22).
- An end-to-end test of a new client helper found a server bug the
  server's own tests missed: after a blocking send answered at
  `AUTH_REQUIRED`, nothing stored the rest of the run. Test the path a
  client takes, not only the one a handler test takes.
- A cargo-mutants run spends almost all its time building: link with `lld`
  (`RUSTFLAGS="-C link-arg=-fuse-ld=lld"`), pass `--test` filters through
  `--`, run it detached from a pinned worktree, and resume with
  `--iterate`.
- An `--in-place` mutation run, or a hand mutant, interrupted by a
  container restart leaves the mutant in the source (2026-10-19): diff
  against `HEAD` before anything else, and never commit while one runs.
- A change to a crate's default features changes what `itk/` and `fuzz/`
  resolve, though neither manifest changes: run `scripts/check_lockfiles.sh`
  before a pull request. On 2026-10-19 `otel` by default left both stale,
  and the ITK harness's `cargo build --locked` failed every scenario.

- tokio 1.54.0 starts an accepted socket readable (#8435), so a mock that
  read only the request head and answered left the body unread; five push
  sender mocks failed until `tests/support/raw_http.rs` read the whole
  request. Its signal driver's panic text also changed ("IO is disabled").
- One example dependency turning on `serde_json/preserve_order`
  (genai 0.7) changes every crate built with it: map iteration order (the
  card ETag, a lint's output) and type sizes (`A2aError` past clippy's
  `result_large_err`). Sort explicitly where order is a contract.
- An idle timeout that counts bytes cuts a request the server is still
  answering: a blocking `SendMessage` is silent while its executor works.
  Found only by driving a slow live model through the SDK (2026-10-20).
- A test that passes with and without the code it is meant to guard is
  not coverage: prove each new test fails without its fix, and drop one
  that cannot.

- A line remap of ledger or control-map citations is applied once, to
  files restored from `HEAD`: run on an already-remapped tree it shifts
  every moved citation again, and the checkers, which check names not line
  text, pass it (2026-10-22).
- Long work in this container dies with it: results kept in scratch space
  are lost on a restart. Commit raw results as they arrive, and make any
  long driver resumable and check that its server is ready before each
  chunk (a restart script that did not filled two cells with failed runs
  in seconds, 2026-10-22).

- A subagent's "fails at HEAD too" is a claim about the environment as
  much as the code: two agents reported `otel_pipeline_default_endpoint`
  as pre-existing; it failed because a collector of mine held port 4317
  (2026-10-23).
- An agent's report written as an untracked file in its worktree is
  deleted with the worktree: copy what is not yet applied before
  `git worktree remove --force`.
- A defect-injection script that restores a file from a backup restores
  its old modification time too, older than the build, so Cargo reuses the
  binary built with the defect: the next "green" or "red" run is about the
  defect, not the code. Touch every restored file (`os.utime`), or edit the
  crate, before trusting a run after one (2026-10-23).
- A stricter client breaks the repository's own fixtures outside the
  crate it changed: after WP-B, the CLI's and the MCP bridge's tests
  served cards without skills. Run the whole workspace, not the crates a
  package touched, before merging it.

## History

[`handoff-history.md`](handoff-history.md) holds the file as it stood on
2026-10-15, before this rewrite, verbatim: every branch section from
`claude/relaxed-planck-c4hsn0` to `claude/funny-heisenberg-e6tl83`, the
observability review, the future ideas (Part B, "What not to chase"), and
each branch's verification of record. Each branch's merged content is also in
`CHANGELOG.md` under its release, in its pull request, and in its commits.
