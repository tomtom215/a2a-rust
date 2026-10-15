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

- **Released:** 0.14.1 (tag `v0.14.1`, 2026-09-30) is the newest release. The
  four crates are at 0.15.0 and the SLIMRPC binding at 0.7.0, all unreleased;
  `CHANGELOG.md` is `[Unreleased]`.
- **Open:** pull request #171, branch `claude/funny-heisenberg-e6tl83`, based on
  #170's merge `2a888a4c`. `main` has since gained only the benchmark bot's
  `179fd50d`.
- **The spec ledger:** `python3 -I scripts/check_spec_ledger.py` is the
  authority on the counts. The PARTIAL entries at MUST level are §3.1.6 (the
  idle bound, approved as recorded), §3.5.2 (explicit `streamLagged` cases
  only, since R69), §5.7 (the spec contradicts itself; upstream draft below)
  and §12.4 (agntcy-slim-rpc carries no error details).
- **Commits:** authored `Tom F. <tomf@tomtomtech.net>` with `Signed-off-by`,
  no other trailers (see *Waiting on the maintainer* for the conflict with
  `CONTRIBUTING.md`).

## Next, in order

The session plan the maintainer set on 2026-10-14 numbers its steps 0 to 6.
Steps 0 to 2 are done (red workflows, the v1.0.1 re-anchor and the approved
audit-register items, the EU AI Act control map), as are the PARTIAL MUSTs
that code could close and R69.

1. **Step 3 — the unfinished items.**
   - **R13, SLIMRPC on agntcy-slim-rpc 3.x** (register: approved). It
     records the verified peer, carries the second half of R41 (the
     rate-limit peer key) and removes the binding's RUSTSEC-2024-0449 waiver
     (`bindings/a2a-protocol-slimrpc/deny.toml` names the condition).
   - **Split the incremental mutation job by crate.** Its slowest shard had
     about ten minutes of headroom when measured on 2026-10-14
     (`handoff-history.md`, tender-cray item 107).
   - **Freshness:** record the first 24-shard weekly mutation sweep in
     `book/src/reference/mutation-history.md` after it runs (Monday
     2026-10-19); after the next release, check the versioned docs deploy
     (`docs.yml` runs on `workflow_run: [Release]` and has not yet fired).
   - The plan also named "rustdoc links"; nothing open matches it, so it is
     dropped unless the maintainer says what was meant.
2. **Step 4 — OpenTelemetry on by default** (decided 2026-10-14):
   instrumentation on by default in every crate; the OTLP exporter default-on
   in `a2a-protocol-sdk` only. Not started: the SDK's `default` is
   `["tls-rustls", "tracing"]`.
3. **Steps 5 and 6 — a release-readiness audit for 0.15.0, then debt.** The
   release itself follows `RELEASING.md` and needs the signing key below.
   Debt is *Open engineering* below, by the maintainer's priority.
4. **After #171 merges:** add `tools/a2a-codegen/Cargo.lock` to the
   `rust-pr` job in `osv.yml` (the push and scheduled `rust` job already
   scans it; the PR scan also scans the base, which lacks it until then).

## Waiting on the maintainer

- **The lag catch-up paging change**: reading a replay longer than
  `subscribe_replay_limit` in pages instead of ending with `streamLagged`. It
  changes an approved behaviour, so it is the maintainer's call.
- **A release signing key.** `.github/release-signers/allowed_signers` and
  `gpg-fingerprints.txt` hold comments only, so `release.yml` refuses every
  tag, by design. Also a `v*` tag ruleset. `ROADMAP.md` tracks both.
- **Upstream drafts to send**, all in `docs/upstream/` (its README lists
  each with its status): among them the SLIMRPC error-mapping disagreement
  (§12 / §5.4) and §5.7's non-empty required arrays against an empty
  `ListTasksResponse.tasks`. The a2a-go draft for R19 is deliberately not
  committed: it was never checked against a2a-go's source.
- **A private advisory to a2a-python** (GHSA, filed by the maintainer; see
  `handoff-history.md`, `claude/nifty-fermat-roj04f`). Do not file a public
  issue while it is open.
- **`CONTRIBUTING.md` asks contributors to add `Co-Authored-By: Claude
  <noreply@anthropic.com>`**, while the maintainer's commits carry only
  `Signed-off-by`. One of the two should change.
- **The OpenSSF Best Practices badge** is not registered;
  `docs/openssf-best-practices.md` has the answers.
- **Publishing:** the SLIMRPC binding has never been published (0.7.0;
  `RELEASING.md`'s binding step has never run). `tck/sut` and
  `examples/mcp-bridge` are `publish = false`; CLI binaries wait on
  `vars.PUBLISH_CLI_BINARIES` (`packaging/README.md`).
- **Decisions never asked**, each documented as current behaviour:
  the default tenant store caps tenants but not the total across partitions
  (`book/src/deployment/multi-tenancy.md`); the approval-gate refusal under
  caller isolation; `RateLimitInterceptor` keys an unidentified caller as
  `"anonymous"`, not by peer address; a WebSocket `Origin` allowlist (the
  ledger's §13.4 note).

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

- **Cross-replica:** single-writer admission (pinned by a test in
  `tests/multi_replica.rs` that is ignored by default); a halt across replicas and G1-B
  (`ROADMAP.md`); a stale unscoped write from another replica keeps a stored
  position whose effect it overwrote (`store/applied.rs`).
- **Client:** the gRPC transport drops `RetryInfo` on `RESOURCE_EXHAUSTED`
  (`transport/grpc.rs`, `retry_after: None`); no check for an empty
  `messageId` or empty parts before sending; no behavioural test of
  `connection_timeout`.
- **Server:** empty push credentials are sent as a bare `Bearer` prefix with nothing after it (`push/sender.rs`);
  no `412` for a non-GET request with a matching `If-None-Match`
  (`agent_card/caching.rs`); TLS on `serve`, `serve_with_addr` and the
  WebSocket listener (only `Server::with_tls` has it); JSON-RPC and REST on
  one socket without a hand-written accept loop; `find_task_by_context`
  clones full history and `RequestContext::stored_task` is `pub` (an API
  decision, `handler/helpers.rs`).
- **Observability:** the metric names' breaking changes — UCUM units, an
  outcome counter, `init_otlp_pipeline`'s `service_name` — undecided;
  removing the deprecated `a2a.server.latency` is allowed from 0.15.0 and
  belongs in `ROADMAP.md`; ADR 0013 still says "Proposed" though its
  decision (`otel::Telemetry`) has shipped; mTLS to the collector and
  `http/json` OTLP are unsupported.
- **Measurement:** `docs/readiness-bar.md` records no measurements yet (WS4);
  WS3 and WS5; a send 6–21% slower at `2b0a3abd` than at `6ce5dc9a`,
  suggested by a two-round run and not yet measured with the pinned-core
  method (`handoff-history.md`, funny-heisenberg item 3); the
  unexplained 0.9–1.8 µs `ListTasks` gap at 10K tasks.
- **Interop:** `itk/interop/card_signature_interop.py` checks Python only;
  a2a-go and @a2a-js are not covered. A Python agent in one cross-language
  trace has not been run.
- **Gates:** a mutation shard that starts after its PR merges grades
  nothing (low); the Windows stress-test connect failure, cause unknown
  (`tests/stress_tests.rs`'s accept loop breaks on the first error); the
  benchmark comparison harness still pins `=0.14.1`.
- **Housekeeping:** the root `deny.toml` ignores RUSTSEC-2024-0436 (`paste`)
  until genai drops it. Found by the 2026-10-15 README review and not fixed:
  `benches/sdk-comparison/scripts/run_itk.sh`'s usage header names
  `run_acts.sh`, and both advertise a third argument neither reads;
  `benches/scripts/run_benchmarks.sh` writes `"version": "0.5.0"` into its
  summary; `benches/benches/cross_language.rs`'s doc comment describes ten
  stream events and a cross-language payload the code does not have;
  `alloc_count.rs` names an `ALLOC_REPORT_MS` that does not exist;
  `.github/dependabot.yml` cites PROVENANCE §3.3 for the exemption in §3.2;
  the SDK's `Cargo.toml` says the client verifies nothing under `signing`,
  though card discovery can verify signatures.

## Waivers and their removal tests

`bindings/a2a-protocol-slimrpc/deny.toml` holds the binding's: RUSTSEC-2026-0285,
-0346, -0348 and -0349 (rustls below 0.23.46 in the binding's lock, held by
`mls-rs-crypto-awslc`'s exact `aws-lc-rs` pin) and RUSTSEC-2024-0449 (goes
with R13). The probe is `cargo update -p rustls --precise 0.23.46` in the
binding; the `osv-scanner.toml` beside it waives the GHSA aliases until
2027-01-08. deny.toml's own comments carry the full chain and are the
authority.

## Decisions the maintainer made

Kept under this heading because `release.yml` and `packaging/README.md` cite
it.

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
  task reflects.
- **Earlier, still standing:** tool calling stays in the examples, not the
  crates (`examples/rig-agent`); the rig integration crate is held
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
  request twice; the manifest-last rule holds either way.

## Branches

Only two kinds matter: the open branch (#171, above) and the storage branches
(`claude/adk-rust-0.12-patch`, `claude/a2a-rig-held`), both built against
0.12. Every other `claude/*` branch is merged or a pre-0.12 session branch;
deleting them is the maintainer's call. To check one:

```sh
git fetch origin && git merge-base --is-ancestor origin/<branch> origin/main && echo merged
```

This clone is shallow, so a "no" for an old branch is not proof, and `git
blame` stops at the shallow boundary. Never write this branch's own head SHA
into this file: the commit that records it changes it.

## Environment

- **Disk** is a fixed allowance: `CARGO_INCREMENTAL=0`, and delete
  `target/semver-checks` and the incremental directories when space runs
  low. A linker "bus error" (signal 7) means the disk is full.
- **PostgreSQL 16** is installed: `pg_ctlcluster 16 main start`, then
  `A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres`
  (password `postgres`).
- **No Docker daemon** in the cloud container, so `itk/docker-compose.yml`
  cannot be run here.
- `pkill -f` matches its own shell: kill by PID.
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

- A test passing and `cargo semver-checks` passing do not show a serde
  change preserves behaviour; parse fixed inputs with both builds.
- The ledger checker verifies test names, not line text: after code moves,
  re-point `impl_evidence` citations from a diff and check each one's text.
- Count before writing a number into a document, and say what was counted.
- A `compile_fail` doctest needs a control that compiles.
- A re-run after a fix must cover the binary that failed, not only the files
  edited.
- An independent review before committing found two defects in R69 that the
  change's own tests and 23 hand mutants did not; budget for one on any
  change to the stream or store contract.

## History

[`handoff-history.md`](handoff-history.md) holds the file as it stood on
2026-10-15, before this rewrite, verbatim: every branch section from
`claude/relaxed-planck-c4hsn0` to `claude/funny-heisenberg-e6tl83`, the
observability review, the future ideas (Part B, "What not to chase"), and
each branch's verification of record. Each branch's merged content is also in
`CHANGELOG.md` under its release, in its pull request, and in its commits.
