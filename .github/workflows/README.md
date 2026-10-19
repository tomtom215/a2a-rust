<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# CI/CD Workflows

GitHub Actions workflows for the a2a-rust project.

## Workflows

All times are UTC. "PRs" means pull requests against `main`.

| Workflow | File | Trigger | Purpose |
|----------|------|---------|---------|
| **DCO** | `dco.yml` | PRs, push to `main` | Every non-merge commit carries a `Signed-off-by:` matching a human git author (see `../../DCO`, `../../PROVENANCE.md`) |
| **CI** | `ci.yml` | Push to `main`, PRs, manual | Static checks (formatting plus the source, prose, workflow and gate-inventory audits, one step each), clippy on Linux, macOS and Windows, feature matrix (`cargo hack --each-feature` over `a2a-protocol-types`, `a2a-protocol-client`, `a2a-protocol-server` and `a2a-protocol-sdk`), tests across the feature combinations listed under [CI Matrix](#ci-matrix) on stable and MSRV, the PostgreSQL integration tests against a live database, docs, cargo-deny, `cargo-semver-checks`, package validation, the out-of-workspace SLIMRPC binding (its own fmt, clippy, cargo-deny, packaging and tests, SPIFFE against a real SPIRE included), end-to-end runs of the `echo-agent`, `incident-response`, `genai-agent`, `rig-agent`, `multi-lang-team`, `resilient-agent` and `swarm` examples (`Example surface coverage`), the `agent-team` dogfood suite, Go SDK interop in both directions, and an informational nightly-toolchain job |
| **Official TCK** | `official-tck.yml` | Push to `main`, PRs, nightly 03:41, manual | The A2A project's own conformance suite (`a2aproject/a2a-tck`) against `tck/sut`. Gated differentially against `tck/conformance-baseline.json`: fails on a MUST failure not in the baseline **and** on a baseline entry that starts passing. See `docs/official-tck-findings.md` |
| **TCK** | `tck.yml` | Push to `main`, PRs | Conformance self-test against the echo agent (JSON-RPC, REST, WebSocket); an all-bindings SUT run (JSON-RPC, REST, WebSocket, gRPC, and cross-binding equivalence); a cross-language matrix over JSON-RPC and REST against this repository's Python, JavaScript, Go and Java agents and against agents built on the official Python, JavaScript, Go and Java SDKs; the official Python SDK client against our server; card-signature and gRPC wire-compatibility interop with the official Python SDK; and the TCK over JSON-RPC against the `rig-a2a-agent`, `genai-a2a-agent` and `incident-response` examples |
| **Coverage** | `coverage.yml` | Push to `main`, PRs, weekly (Mon 06:17), manual | Code coverage via `cargo-llvm-cov` and Codecov upload (`Test coverage`, every trigger but the schedule; policy in `codecov.yml`). On the schedule and on manual runs, `codecov.yml ignores are applied` checks that Codecov actually applies `codecov.yml`'s ignore list |
| **Deploy Documentation** | `docs.yml` | Push to `main`, a successful `Release` run, manual | Build the versioned site (`/` the latest release, `/vX.Y/` each release line, `/dev/` `main`) and deploy it to GitHub Pages; `scripts/build_docs_site.py` |
| **Benchmarks** | `benchmarks.yml` | Push to `main` and PRs touching `benches/`, `crates/`, `Cargo.toml` or `Cargo.lock` (PRs also on a change to the workflow itself); manual | Full criterion run + book page commit on push and manual runs; statistical regression gate (`Regression Gate`) on PRs |
| **Release** | `release.yml` | Tag push (`vX.Y.Z` or `vX.Y.Z-*`) | Validation (the tag is annotated, on `main`, and signed by a key in `../release-signers/` as it stands on `main`; the provenance manifest describes the tagged commit; versions, CHANGELOG, CITATION.cff, SECURITY.md; nothing under `[Unreleased]`, the tag is the release-preparation commit, breaking-release cadence), CI matrix, security audit, packaging with SBOMs and SLSA attestations (`.crate` files checked to be built from the tag), ACTS on the tagged commit with its summary attested onto the `.crate` files, GitHub release, crates.io publish in the `crates-io` environment via Trusted Publishing (OIDC; environment-secret fallback until every crate is configured). The `cli-binaries` job attaches CLI binaries to the release only when the `PUBLISH_CLI_BINARIES` variable is `true` |
| **Mutants** | `mutants.yml` | PRs labelled `mutation-test` (incremental `--in-diff`; the required aggregator fails on an unlabelled PR); full sweep weekly (Mon 03:00) and manual | Mutation testing; fails on any missed mutant, reports timeouts separately |
| **Fuzz** | `fuzz.yml` | Push to `main`, PRs, nightly 03:41, manual | `cargo fuzz` over every target in the matrix (`scripts/check_fuzz_matrix.py` in CI checks each is both built and run): 60 s per target, 10 min per target on the nightly schedule. Nightly and manual runs restore and save each target's corpus |
| **Soak** | `soak.yml` | Push to `main`, PRs, nightly 02:23, manual | Sustained load against a real server, once in one process with the in-memory store and once as two replicas sharing PostgreSQL: 60 s each on PRs and pushes, 1800 s nightly, the requested duration (default 600 s) when run manually |
| **Pin freshness** | `pin-freshness.yml` | Mondays 05:00, narrowed to every third Monday by its `cadence` job; manual | Resolves each official SDK the TCK grades (Python, JavaScript, Go, Java) to its latest release and runs the TCK against it with the skip list `tck.yml` carries. Reports; never commits a pin bump. Not a PR gate |
| **ITK (upstream current-mount)** | `itk.yml` | Push to `main`, PRs, nightly 04:17, manual | The in-repo traversal self-test of the ITK "current" agent under `itk/`, plus, on manual runs only, a current-vs-`python_v10` run through the upstream `run_tests.py` |
| **ITK nightly** | `itk-nightly.yml` | Nightly 02:00, manual, and PRs to `main` that touch the ITK agent or harness (`itk/src/`, `itk/protos/`, `itk/run_itk.sh`, `itk/build.rs`, `itk/Cargo.*`, the workflow) | The A2A project's own Integration Testing Kit (`a2aproject/a2a-itk`) with this repository mounted as the system under test against every peer SDK line in its `matrix.yaml` — official Python, JavaScript, Go, Java, and a2a-rs — over JSON-RPC, gRPC and HTTP+JSON. Results (`itk_rust.json`, and since 2026-10-09 the ACTS nightly's `acts_rust.json`) are uploaded as a run artifact and, from `main` only, to the rolling `nightly-metrics` prerelease the ITK dashboard reads; a PR run produces only the artifact. Not a required check |
| **ACTS conformance** | `acts.yml` | PRs, push to `main`, manual; called by `release.yml` | The A2A project's conformance suite (ACTS, `a2aproject/a2a-itk` at a pinned revision) against this repository's ITK agent over JSON-RPC, gRPC and HTTP+JSON. Fails on any failed or errored test at any level (`scripts/acts_predicate.py`), stricter than upstream's MUST-only `--require-conformant`; on a release its summary is attested onto the `.crate` files |
| **Verification** | `verification.yml` | PRs touching `crates/a2a-protocol-types/`, the server's `handler/halt.rs` or `handler/messaging/`, `verification/` or the workflow; push to `main`; manual | Proofs rather than tests: Kani over the task state machine and key parsing (`crates/a2a-protocol-types/src/kani_proofs.rs`), and the loom model of the halt/admission protocol (`verification/loom-halt`) |
| **OSV-Scanner** | `osv.yml` | PRs, push to `main`, daily 06:27, manual | Lockfiles against the OSV database. The nine tracked `Cargo.lock` files block, on a PR as on `main` (`Rust lockfiles`); a PR also gets `Rust lockfiles (new findings)`, which names the findings it introduces and does not yet scan `tools/a2a-codegen/Cargo.lock`. Waivers go in an `osv-scanner.toml` beside the lockfile (today one at the repository root and one in `bindings/a2a-protocol-slimrpc/`), each with a reason and an `ignoreUntil` date. The conformance peers' Go, Java, JavaScript and Python lockfiles are scanned and reported, never blocking. The hash-locked CI requirements in `../requirements/` are not scanned here |
| **CodeQL** | `codeql.yml` | PRs, push to `main`, weekly (Tue 04:13), manual | Static analysis (`security-extended`) of the Rust sources, the workflows themselves (`actions`) and the gate scripts (`python`); results in Security → Code scanning |
| **Scorecard** | `scorecard.yml` | Push to `main`, branch-protection changes, weekly (Mon 05:41), manual | OpenSSF Scorecard; published to api.scorecard.dev for the README badge and uploaded to code scanning. A dashboard, never a gate. Its shape is fixed by the Scorecard API's workflow restrictions, listed in the file |
| **Dependabot** | `../dependabot.yml` | Weekly (Mondays 04:00) | Cargo (the workspace and the SLIMRPC binding): minor and patch bumps grouped into one PR per directory, majors as separate PRs. GitHub Actions: one group matching every action (`patterns: ["*"]`, no update-type filter, so it is not limited to minor and patch). Its commits are DCO-exempt by exact author identity (see `dco.yml`, `PROVENANCE.md` §3.2) |

## Required status checks

Branch protection lives in repository settings, not in this tree — so this
section records which checks are *intended* to be merge-blocking on `main`.
If you administer the repo, keep Settings → Branches → required status
checks in sync with this list. A rule matches a job's display name exactly,
including any parenthesised suffix and, for a matrix job, the expanded matrix
values, so job renames here silently drop the requirement there:

- `DCO / Sign-off and authorship`
- `Official TCK / a2a-tck conformance`
- These `CI` jobs:
  - `Static checks (formatting, sources, docs, gate inventory)`. This job was
    named `Format` until 2026-09-20; a required-status-check rule still
    naming `Format` matches nothing and must be re-pointed.
  - `Clippy (stable, <os>)` for `ubuntu-latest`, `macos-latest` and
    `windows-latest`
  - `Feature matrix (cargo-hack, each feature)`
  - `Test (<rust>, <os>)` for `stable` and `1.88` on each of those three OSes
  - `Documentation`
  - `cargo-deny`
  - `Package validation`
- `TCK self-test (echo-agent)` and the `TCK cross-language (<sdk>)` matrix
  (`python`, `javascript`, `go`, `java`, `python-sdk`, `js-sdk`, `go-sdk`,
  `java-sdk`)
- `Mutation Testing (incremental)`
- `Regression Gate` (benchmarks)
- `OSV-Scanner / Rust lockfiles` — the blocking scan of `osv.yml`, which runs
  on pull requests too since 2026-10-14; before then a finding already on
  `main` could fail no pull request
- `OSV-Scanner / Rust lockfiles (new findings)` — the PR job of `osv.yml`;
  the job's display name comes from the reusable workflow it calls, so check
  the exact string in a PR's checks list before adding it
- `Analyze (rust)`, `Analyze (actions)`, `Analyze (python)` (CodeQL) — they
  fail only when analysis cannot run; making a new *alert* block a merge is
  code scanning's own merge protection, configured under Settings → Rules
- `Test coverage` (upload job; the Codecov project/patch statuses themselves
  are dashboards guarded by the thresholds in `codecov.yml`)

`Nightly (informational)` and the full mutation sweep are deliberately not
required: the former floats with the nightly toolchain as an early-warning
canary and runs on pushes to `main` and manual runs only, the latter runs
weekly and on demand, never on a pull request.

Other jobs also run on pull requests but are not on the list above, and this
file does not record whether each is meant to stay off it:

- `CI`: `Test (postgres integration)`, `cargo-semver-checks`,
  `SDK dogfood (agent-team, all features)`, `SLIMRPC binding (out-of-workspace)`,
  `Example surface coverage (all methods x all bindings)`,
  `Go SDK interop (both directions, three bindings)`
- `TCK`: `TCK all bindings (SUT)`, `Official Python SDK client vs our server`,
  `Card signatures vs the official Python SDK`,
  `gRPC wire compatibility (official Python SDK)`, `TCK example agent (<agent>)`
- `ACTS conformance`: `ACTS (JSON-RPC, gRPC, HTTP+JSON)`
- `Mutation Testing`: the `Mutants incremental (<crate> shard <k>/<n>)` jobs that
  `Mutation Testing (incremental)` aggregates
- `Fuzz`: `fuzz <target> (smoke)`
- `Soak`: `Sustained load`
- `ITK (upstream current-mount)`: `ITK traversal self-test (in-repo)`
- Path-filtered on pull requests, so absent from a PR that does not touch
  those paths: `Kani proofs (types crate)` and
  `Loom model (halt/admission protocol)` (Verification), and
  `ITK nightly (current vs every peer)`
- `OSV-Scanner`: `Conformance peers (informational)`, which never fails on a
  finding by design

## CI Matrix

The CI workflow tests across multiple configurations:

- **Rust versions**: stable + MSRV (1.88) for tests; clippy runs on stable only, since lint verdicts change between clippy versions
- **Platforms**: Linux, macOS, Windows for clippy and tests; every other job on Linux
- **Feature combinations** (the `Test` job, each a step of its own; some
  scoped to one crate): default, `signing`, `tracing`, `tls-rustls` (client,
  and separately server), `sqlite`, `postgres`, `axum`, `websocket`, `grpc`,
  `auth-jwt`, `auth-jwt,tls-rustls`, `--all-features`,
  `--no-default-features`. The `Clippy` job lints a similar hand-picked list
- **Feature matrix**: `cargo hack clippy --each-feature` over
  `a2a-protocol-types`, `a2a-protocol-client`, `a2a-protocol-server` and
  `a2a-protocol-sdk` — every feature alone, no-default-features and
  all-features — on Linux, so a `#[cfg(feature)]` gap cannot hide behind
  the hand-picked lists above
- **Checks**: `cargo fmt`, `cargo clippy`, `cargo test`, `cargo doc`,
  `cargo deny`, `cargo semver-checks`, `cargo package`

## Running Locally

CI sets `RUSTFLAGS` and `RUSTDOCFLAGS` to `-D warnings` for every job, so a
warning fails it. To reproduce the main checks:

```bash
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTFLAGS="-D warnings" cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

This is a subset: CI also lints and tests each feature combination above;
see the steps in `ci.yml` for the exact commands.

## Benchmark Automation

The benchmarks workflow runs every criterion suite, generates a Markdown
results page and an interactive dashboard, and commits them to
`book/src/reference/benchmarks.md` and
`book/src/reference/benchmark-dashboard.html`. The push is authenticated with
`GITHUB_TOKEN`, so it triggers no workflow run: the fresh numbers reach GitHub
Pages on the next `docs.yml` run (the next other push to `main`, a successful
release, or a manual dispatch).

## License

Apache-2.0
