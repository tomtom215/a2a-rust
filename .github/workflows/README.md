<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# CI/CD Workflows

GitHub Actions workflows for the a2a-rust project.

## Workflows

| Workflow | File | Trigger | Purpose |
|----------|------|---------|---------|
| **DCO** | `dco.yml` | PRs | Every non-merge commit carries a `Signed-off-by:` matching a human git author (see `../../DCO`, `../../PROVENANCE.md`) |
| **CI** | `ci.yml` | Push to `main`/`claude/**`, PRs | Format, clippy, feature matrix (`cargo hack --each-feature` over every published crate), tests across nine feature combinations, docs, cargo-deny, MSRV, package validation |
| **Official TCK** | `official-tck.yml` | Push to `main`, PRs, nightly | The A2A project's own conformance suite (`a2aproject/a2a-tck`) against `tck/sut`. Gated differentially against `tck/conformance-baseline.json`: fails on a MUST failure not in the baseline **and** on a baseline entry that starts passing. See `docs/official-tck-findings.md` |
| **TCK** | `tck.yml` | Push to `main`, PRs | Conformance self-test (echo-agent) plus cross-language agents (Python, JS, Go, Java) over the JSON-RPC and REST bindings |
| **Coverage** | `coverage.yml` | Push to `main`, PRs | Code coverage via `cargo-llvm-cov`, Codecov upload (policy in `codecov.yml`) |
| **Documentation** | `docs.yml` | Push to `main` | Build mdbook, deploy to GitHub Pages |
| **Benchmarks** | `benchmarks.yml` | Push to `main`, manual; PRs run the regression gate | Full criterion run + book publish on `main`; statistical regression gate on PRs |
| **Release** | `release.yml` | Tag push (`v*`) | Validation (versions, CHANGELOG, CITATION.cff, SECURITY.md), CI matrix, security audit, SLSA-attested packaging, GitHub release, crates.io publish via Trusted Publishing (OIDC; environment-secret fallback until every crate is configured) |
| **Mutants** | `mutants.yml` | PRs (incremental `--in-diff`), manual full sweep | Mutation testing; fails on any missed mutant, reports timeouts separately |
| **ITK (upstream current-mount)** | `itk.yml` | Push to `main`, PRs, nightly | The in-repo traversal self-test of the ITK "current" agent under `itk/`, plus a manual current-vs-`python_v10` run through the upstream `run_tests.py` |
| **ITK nightly** | `itk-nightly.yml` | Nightly 02:00 UTC, manual | The A2A project's own Integration Testing Kit (`a2aproject/a2a-itk`) with this repository mounted as the system under test against every peer SDK line in its `matrix.yaml` — official Python, JavaScript, Go, Java, and a2a-rs — over JSON-RPC, gRPC and HTTP+JSON. Results (`itk_rust.json`) are uploaded as a run artifact and, from `main`, to the rolling `nightly-metrics` prerelease the ITK dashboard reads. Not a PR gate |
| **Dependabot** | `../dependabot.yml` | Weekly (Mondays 04:00 UTC) | Grouped minor/patch bumps for Cargo (workspace and the SLIMRPC binding) and GitHub Actions; majors arrive as separate PRs. Its commits are DCO-exempt by exact author identity (see `dco.yml`, `PROVENANCE.md` §3.2) |

## Required status checks

Branch protection lives in repository settings, not in this tree — so this
section records which checks are *intended* to be merge-blocking on `main`.
If you administer the repo, keep Settings → Branches → required status
checks in sync with this list (job renames here silently drop the
requirement there):

- `DCO / Sign-off and authorship`
- `Official TCK / a2a-tck conformance`
- All `CI` jobs (Format, Clippy, Feature matrix, Test, Documentation, cargo-deny, Package validation)
- `TCK self-test (echo-agent)` and the `TCK cross-language` matrix
- `Mutation Testing (incremental)`
- `Regression Gate` (benchmarks)
- `Test coverage` (upload job; the Codecov project/patch statuses themselves
  are dashboards guarded by the thresholds in `codecov.yml`)

`Nightly (informational)` and the full mutation sweep are deliberately not
required: the former floats with the nightly toolchain as an early-warning
canary, the latter runs on demand.

## CI Matrix

The CI workflow tests across multiple configurations:

- **Rust versions**: stable + MSRV (1.88) for tests; clippy runs on stable only, since lint verdicts change between clippy versions
- **Platforms**: Linux, macOS, Windows
- **Feature combinations**: default, `signing`, `tracing`, `tls-rustls`,
  `sqlite`, `postgres`, `axum`, `--all-features`, `--no-default-features`
- **Feature matrix**: `cargo hack clippy --each-feature` over the four
  published crates — every feature alone, no-default-features and
  all-features — on Linux, so a `#[cfg(feature)]` gap cannot hide behind
  the hand-picked list above
- **Checks**: `cargo fmt`, `cargo clippy`, `cargo test`, `cargo doc`,
  `cargo deny`, `cargo package`

## Running Locally

```bash
# Reproduce CI checks locally
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo doc --workspace --no-deps
```

## Benchmark Automation

The benchmarks workflow runs every criterion suite, generates a Markdown
results page and an interactive dashboard, and commits them to
`book/src/reference/benchmarks.md` and
`book/src/reference/benchmark-dashboard.html`. This triggers the docs
workflow to redeploy GitHub Pages with fresh numbers.

## License

Apache-2.0
