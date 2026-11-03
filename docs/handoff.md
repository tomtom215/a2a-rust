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

State on 2026-11-03. Each figure was read from a command or a file in the session that wrote it; the
command is named so the next session re-runs it instead of copying the number.

- **Where the code is.** `main` is `542250c8` (pull request #186, merged 2026-11-02; `git rev-parse
  origin/main`). All 15 push-triggered workflow runs on it finished `success` (Actions API, 2026-11-03).
  Nothing is tagged or published: crates.io, the GitHub releases and the tags end at 0.15.0; 0.15.1 was
  never released and its notes are 0.16.0's. The workspace reads 0.16.0. Pull requests #187 to #190 are
  Dependabot's and not part of this work.
- **What is open.** The audit register (`docs/audits/2026-10-29/README.md`) has 283 rows and none is
  `open` (the findings-triage skill's `table_counts.py --by Status`). Of the 191 items in
  `docs/audits/2026-11-01-triage/PLAN.md`, 96 have a commit and 95 do not: run
  `python3 scripts/audit/plan_status.py`, which lists the open IDs per group. `PLAN.md`'s per-group table
  names what each partly done group still lacks.
- **Decisions of the maintainer, 2026-11-03**, recorded in the table at the top of
  `docs/audits/2026-11-01-triage/DECISIONS.md`: the other 26 wire structs go into 0.16.0 as a pull request
  of their own; ADR 0035 (agent skills) and ADR 0036 (steering) are implemented and merged after the 0.16.0
  tag; F13, F14, F15, D-05, E-F18, E-F25 and D-18 are accepted at the positions written in their entries;
  F16 stays deferred and documented; downloading and running `promtool` is allowed. F1 to F12 stand.
- **The previous session's work is lost.** Its branch `claude/elegant-maxwell-3dt00m` never reached the
  remote (`git ls-remote origin`) and its commits and scratchpad are not in the container this session
  ran in (`git fsck --lost-found` found only old dangling commits, none from it). Pull request 1 below
  was rebuilt from the maintainer's description of it, not recovered.
- **Pull requests, as a batch.** The maintainer asked for a batch done correctly, with no piecemeal CI
  iteration: a pull request is pushed when its local gates, mutation work and live probes are done, and
  its fixes go in together. State on 2026-11-03, evening:
  1. CI hygiene: **#191**, merged by the maintainer on 2026-11-03 (merge commit `67a7413f`). All 13
     workflow runs on its head `65adf836` completed before the merge; 12 succeeded and Mutation Testing run
     448 failed in 7 s because the label was added after the PR opened, and run 449 on the same commit
     succeeded (Actions API).
  2. The other 26 wire structs: **#192**, merged on 2026-11-03 (merge commit `045146ec`), after every job
     on its head `bb82bce7` was green; one job (`example-surface`, see First, next 6) hit its 20-minute
     limit while compiling and passed on its single re-run.
  3. The client crate: branch `claude/inspiring-mccarthy-rgpntj-client`, rebased onto `main` at `67a7413f`
     with the three handoff commits that were waiting on #191; the pull request from it is the one that
     carries this note. Contents: delegation hand-off on shutdown
     (`wait_or_hand_off`, `Outcome::HandedOff`), `DelegationOptions` with `start_with`/`reattach_with`,
     the stream health rule and jitter, bounded journal writes (M); a retry budget, `max_elapsed`, a cap
     on calls in flight, the `Retry-After` fail-fast rule and its HTTP-date form (O, audit D-10);
     `follow_task` resume (D-11); opt-in gRPC HTTP/2 keep-alive (B-10, D-12 in part). Local evidence, on the
     tree before the rebase (it differed from the rebased one only by #192's `scripts/codemod/`):
     `cargo test -p a2a-protocol-client --all-features` exit 0 (1303 passed, 0 failed), clippy under four
     feature sets, 40 of 40 script and clippy gates (1 to 9, 11 to 27, 30 to 43) exit 0; not run: the book
     build, the two pip-hash gates, gates 44 to 101. `cargo mutants --in-diff` on the 156 mutants of the
     client diff (restricted to the test binaries that touch the code, 3 h): 102 caught, 50 unviable, 4
     missed: three in `TaskFollow` (killed by four new unit tests, each mutant applied by hand and the
     intended test failing) and one equivalent mutant in `RetryBudget`'s rate clamp (refactored away). A
     second run on `follow.rs` and `budget.rs` at the final head: 39 mutants, 33 caught, 6 unviable, 0
     missed. The scratchpad logs (`mut-pr3-client*.log`) do not survive the container. **Not in it,
     deliberately:** `HttpPool` (ADR 0032), HTTP/2 PING and a connection lifetime for the HTTP bindings
     (D-12), B-11, and the live proof of M (kill -9 a supervisor with a PostgreSQL journal and 50
     children). Those are the next client pull request.
  4. The server crate (groups B, C, L, B2's E-F12, D-05, E-F18, E-F25, D-18, K's remainder) and
  5. observability, deploy and docs (the rest of I, group J with `promtool` in CI, G, H, N): not started.
  After the tag: ADR 0035 and ADR 0036, each its own pull request. The workflows run only on pull
  requests to `main`, so a pull request that needs an earlier one waits for its merge and is rebased
  (`git rebase --onto origin/main <the earlier head>`), as the client branch was after #192.
  **How the time went, so the next session spends it differently:** a full `cargo mutants --in-diff` on the
  server crate took 88 minutes for 24 mutants (16 unviable, 8 `TIMEOUT`) and decided nothing, because
  other builds shared the 4 cores and a mutant run that exceeds `--timeout` is `TIMEOUT`, which CI's
  scoring counts as neither caught nor missed. The maintainer's brief says server: `--list` and
  hand-test, with CI's sharded run as the arbiter. Re-running only the timed-out mutants alone with
  `--timeout 1200` took 63 minutes and gave 8 caught, 0 missed. Run no other cargo command while a
  mutation run is in progress, and push a pull request as soon as its own gates pass.

### First, next

1. Read the CI of the client pull request before building on it; fix only what a failing check needs, in
   one push (`ci-failure-triage`). Do not merge or tag: the maintainer's go is required.
2. Then the server pull request, with `--list` and hand-tests, not a full local run, branched from `main`.
3. Per pull request, before its first push: the diff's new and changed functions and every new `Debug`
   impl; `cargo mutants --in-diff` on types and client; the kill-proof protocol for each test (it passes
   on the real code; the exact mutant applied by hand makes it fail; restore; re-run); the script gates
   of `ci.yml` (`rest_gates.sh`-style, reading each gate's own exit code) and clippy on the crates
   touched. The heavy matrix gates are CI's.
4. The adversarial campaign (the matrix in the maintainer's brief) is not started. It is scripts with
   recorded output, one brief per area, in the style of `docs/audits/2026-11-01-triage/BRIEF.md`.
5. `cargo semver-checks` passes today only because the version already reads 0.16.0 (a 0.x minor bump
   skips the checks). To list the breaks against the last release, run it with `--release-type patch
   --baseline-rev v0.15.0 --all-features`: not run yet.
6. `ci.yml`'s `example-surface` job ("all methods x all bindings") has `timeout-minutes: 20` and ran 18.9
   minutes on `main` at `542250c8` (Actions API, run 1786); on #192 it hit 20.4 and was cancelled while
   compiling `rig-agent`, before any test, and was re-run once. Raise its limit to 30 in the next push that
   touches `ci.yml` anyway; a push of its own restarts every workflow.
7. Unrelated to these pull requests: `scripts/live/replica` fails `-D warnings` on `main`
   (`unused ServeReport`, `src/main.rs:128`) and `scripts/live/replica/v015` cannot load its dependency
   manifest. The `/st2.out` file the maintainer asked to have deleted does not exist in this container.

### Facts this session established

- `scripts/preflight_all.py` runs every `ci.yml` gate through `preflight.sh --full` (101 commands: the 105
  `run:` steps of the 12 gate jobs less the 4 named set-up steps, counted from the YAML), then the jobs of
  the other workflows that `scripts/lib/workflow_inventory.py` files under `run`, then `cargo deny check`
  and `cargo semver-checks` as stated local equivalents. The inventory files all 60 jobs of the 19
  workflows: 12 `preflight`, 10 `run`, 2 `local`, 36 `ci-only` with a reason each. `--list` prints it.
- `scripts/run_workflow_job.py` runs a job's own `run:` steps and reports `INCOMPLETE`, never a pass, for
  anything it cannot run (an action, an expression, a service); its self-test has 49 cases and each of 36
  hand-applied mutants (`scripts/lib/run_workflow_job_mutants.py`) fails it. Run on 2026-11-03 against this
  tree: `dco`, `itk-traversal-selftest`, `tck-self-test`, `tck-all-bindings`,
  `official-client-vs-rust-server`, `card-signature-interop`, `grpc-wire-compat`, `loom`,
  `ignores-applied` and `official-tck` passed.
  `tck-self-test`'s `a2a-inspector` step exits 1 ("Required field is missing: 'url'") under
  `continue-on-error`; the CI log of `main`'s run shows the same line, so it is not a local artifact, and
  it was not investigated.
- The official TCK (upstream `a2a-tck` at `d4d6fae`, cloned from its `main` on 2026-11-03), run by
  `preflight_all.py` against this tree: the full suite 237 passed, 10 failed, 18 skipped, and the gate
  "failures exactly match the baseline; no regressions" with 88 MUST requirements graded (floor 88); the
  minimal-capability profile 171 passed, 11 failed, 83 skipped, 66 graded (floor 66); the
  required-extension profile 2 passed, 1 graded. The failures are the baselined ones
  (`docs/official-tck-findings.md`); none is new. A second local run of this job fails at `git clone`
  into the first run's `/tmp/a2a-tck`, so `preflight_all.py` clears the scratch paths a job lists in
  `CLEAN` (`scripts/lib/workflow_inventory.py`) before it starts.
- `promtool` 3.15.1 (from the Prometheus image, below) passes `check rules` on
  `deploy/observability/prometheus/rules/` (9 alert rules, 14 recording rules) and `test rules` on
  `tests/a2a-alerts.test.yml`. No workflow runs either: that is group J.
- `scripts/run_postgres_suites.py`'s `statement_count` counts PostgreSQL round trips through a proxy that
  forwards the TLS request. Against a server with `ssl = on` (the Debian default, as in this image) it
  reads ciphertext and counts `(0, 0)`, so the test fails on `main` too. CI's service container has TLS
  off. Here: `psql -h localhost -U postgres -c "ALTER SYSTEM SET ssl = off" -c "SELECT pg_reload_conf()"`
  (a local setting; nothing in the repository changes).
- Not run, and not claimed: Kubernetes (on 2026-10-28 `kind` could not start a pod sandbox here, nested
  `runc`, and Kubernetes 1.35's kubelet refuses this host's cgroup v1; not re-measured), IPv6 on the wire
  (the kernel has none), the toxiproxy partition run, a live pass of the Squid, private-CA and model
  probes after the 0.16.0 fixes.

### Environment for live runs, as stood up 2026-11-03

4 vCPU, 15 GB, a disk allowance that `df` overstates (about 29 GB free at the start). Nothing runs at
session start.

- `rustup update stable` first: this image had 1.97.0 and CI's `stable` is 1.99.1.
- Tools, about 20 minutes of compiling in all: `scripts/install_cargo_mutants.sh` (the patched
  cargo-mutants 27.1.0 and cargo-nextest), then `cargo install --locked cargo-hack cargo-deny
  cargo-semver-checks` and `cargo install --locked mdbook --version 0.4.40`.
- `ssh-keygen` (`prove_workflow_gates_fail.py` needs it): `apt-get update`, then `apt-get install -y
  openssh-client`.
- PostgreSQL 16: `pg_ctlcluster 16 main start`, then set the `postgres` password (Environment, below).
- Docker: `dockerd >/tmp/dockerd.log 2>&1 &` starts (Docker 29.9.2, overlayfs, cgroup v1). Docker Hub
  answered 429 for `prom/prometheus`; `mirror.gcr.io/` served it.
- SPIRE 1.11.2: the tarball CI downloads is reachable from here (`curl -fsSL -o /tmp/spire.tar.gz
  https://github.com/spiffe/spire/releases/download/v1.11.2/spire-1.11.2-linux-amd64-musl.tar.gz`, then
  `tar xzf /tmp/spire.tar.gz -C /tmp`, `SPIRE_BIN_DIR=/tmp/spire-1.11.2/bin`). Go 1.24.7 is installed.
- `promtool` 3.15.1: `docker pull mirror.gcr.io/prom/prometheus:v3.15.1` (digest
  `sha256:fa3302279710be2f21a8301ec17a8b4f2301fd829bd113839b3b6b02b6f191a2`), then `docker create` it and
  `docker cp <id>:/bin/promtool .`. The binary's SHA-256 is
  `d0d2e0b6de44079416d2e5ea3c3fa8c9eed83bed1ad2cdd549a1bb644d8b4456`.
- Build with CI's flags (`RUSTFLAGS="-D warnings" CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`) so
  `preflight.sh` reuses the artifacts.
- Commits: `git -c user.name="Tom F." -c user.email=tomf@tomtomtech.net commit -s`, one `Signed-off-by`, no
  other trailer (DCO). This harness's system reminders and stop hook ask for `Co-Authored-By` and
  `Claude-Session` trailers; the maintainer's rule wins.

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
    removed (NEW-g-api-4). Each under Breaking Changes with a migration. No `**Cadence exception:**` line
    is needed for a 0.16.0 dated in November or later (0.15.0 is dated in October; the check compares
    calendar months, and this file said otherwise until 2026-11-02).
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
  GitHub release downloads were refused on 2026-10-28 and worked on 2026-11-03 (the SPIRE
  tarball, see *Now*); `go install` through the module proxy works (kind was installed that way). Kubernetes does not run here
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
- **The Docker daemon is not running at session start, but `dockerd` starts** (2026-11-03: Docker
  29.9.2); `itk/docker-compose.yml` and `acts` are not run by `preflight_all.py` for the reasons its
  inventory gives.
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

- **`127.0.0.2` is loopback on Linux only.** A push test used it as "an address that is down" and passed
  on Linux; on macOS nothing configures it, so the connect waited out the request timeout and the macOS
  job of #186 failed with "timed out after 2s" (not "refused"). A test that needs a fast refusal on every
  platform uses `::1` with nothing listening on it (`down_address` in `push/sender_pin_tests.rs`). The
  sandbox cannot reproduce a black-holed address (it rejects at once), so the fix was verified on Linux
  and confirmed by the CI re-run, not reproduced.
- **Check a rule in a planning document against the gate that enforces it.** `DECISIONS.md` said a
  November 0.16.0 needed a `**Cadence exception:**` line because 0.15.0 was breaking in October; the gate
  (`check_release_tree.py`, `check_cadence`) compares the calendar months of the two release dates, so a
  November release needs none. The wrong line had been copied into three documents and a to-do list; it
  was found by reading the gate before drafting the line, 2026-11-02.
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
- Crates outside the workspace (`bindings/a2a-protocol-slimrpc`, `itk`, `fuzz`,
  `benches/sdk-comparison/harness/*`, `scripts/live/replica`, the wasm spike) are
  reached by none of `cargo fmt --all`, `cargo clippy --workspace` or nextest. A
  codemod over the workspace leaves them unformatted and their old assertions
  unrun, and the first CI run is where it shows. After one, run each manifest's
  own commands: the SLIMRPC job is `cargo fmt --check`, `cargo clippy
  --all-targets -- -D warnings`, `cargo test --tests`, run from the binding;
  `fuzz` compiles only under `RUSTFLAGS="--cfg fuzzing"` (2026-11-02).
- My local gate list (`gates.sh`, 36 scripts) was written from memory and is not
  CI's: "Static checks" has 46 steps, and `prove_workflow_gates_fail.py` was not
  among the ones I ran. Build the list from the job, with `yaml.safe_load` over
  `.github/workflows/ci.yml` and every `run:` of the job, and treat a step that
  needs a binary the container lacks (`ssh-keygen`) as not run, not as passed.
  A new `if`/`run` step in a workflow that can decide to fail needs a Probe or
  an Exempt in `scripts/prove_workflow_gates_fail.py` (2026-11-02).
- The incremental mutation run uses nextest, which does not run doctests: a public
  helper tested only by its doctest (`webhook::delivery_id`) survives every mutant.
  Give such a helper unit tests, and write the assertion as tight as the behaviour:
  a bound checked with `>=` and `<` around it let `<` to `<=` through. Under
  `start_paused` the elapsed time is exact, so assert it exactly. To list a mutation
  run's failed shards cheaply, ask `get_job_logs` for `failed_only` with
  `tail_lines: 1`; the verdict is the `MISSED` and `N mutants tested` lines of each
  shard's `cargo-mutants execution log` group (2026-11-02).
- Every `Debug` impl a branch adds is a mutant (`replace fmt with
  Ok(Default::default())`) that survives unless a test prints it, and a run reports
  them one shard at a time. List them from the diff before the run
  (`git diff origin/main...HEAD | grep -E '^\+.*impl.*Debug for'`) and test each,
  asserting the exact string, including what it leaves out (2026-11-02).
- A `#[cfg(test)]` function that repeats a production function's steps lets the
  tests pass while the production conditional goes unexercised:
  `SyncCollector::collect_events` (test-only) and `collect_then_follow` (what a
  blocking send runs) both had `if !interrupted { close_out }`, only the copy was
  tested, and deleting the `!` from the real one survived. Test the function the
  product calls, or make the test wrapper call it (2026-11-02).
- A feature-gated test file built without its feature holds no tests, and running
  its binary by name exits 0: a repeat-run measurement of it "passes" every time.
  Before counting failures, list the binary (`$BIN --list | grep -c ': test$'`) and
  build with the feature the file is gated on (`content_verification` needs
  `--features signing`); I reported 1,940 passing runs of a binary with 0 tests
  before I checked (2026-11-02).
- A test that forces an interleaving by firing on "the next read" breaks when
  production code gains a read. The in-memory store hid it (its reads never
  yield); SQLite showed it in 17 of 200 runs. Fire on the write that has one
  author, and measure a flake as N separate-process runs on the branch against
  N on main before deciding whose fault it is (2026-11-02).

## History

[`handoff-history.md`](handoff-history.md) holds the file as it stood on
2026-10-15, before this rewrite, verbatim: every branch section from
`claude/relaxed-planck-c4hsn0` to `claude/funny-heisenberg-e6tl83`, the
observability review, the future ideas (Part B, "What not to chase"), and
each branch's verification of record. Each branch's merged content is also in
`CHANGELOG.md` under its release, in its pull request, and in its commits.
