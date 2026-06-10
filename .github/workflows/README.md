<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# CI/CD Workflows

GitHub Actions workflows for the a2a-rust project.

## Workflows

| Workflow | File | Trigger | Purpose |
|----------|------|---------|---------|
| **CI** | `ci.yml` | Push to `main`/`claude/**`, PRs | Format, clippy, tests across nine feature combinations, docs, cargo-deny, MSRV, package validation |
| **TCK** | `tck.yml` | Push to `main`, PRs | Conformance self-test (echo-agent) plus cross-language agents (Python, JS, Go, Java) over the JSON-RPC and REST bindings |
| **Coverage** | `coverage.yml` | Push to `main`, PRs | Code coverage via `cargo-llvm-cov`, Codecov upload (policy in `codecov.yml`) |
| **Documentation** | `docs.yml` | Push to `main` | Build mdbook, deploy to GitHub Pages |
| **Benchmarks** | `benchmarks.yml` | Push to `main`, manual; PRs run the regression gate | Full criterion run + book publish on `main`; statistical regression gate on PRs |
| **Release** | `release.yml` | Tag push (`v*`) | Validation (versions, CHANGELOG, CITATION.cff, SECURITY.md), CI matrix, security audit, SLSA-attested packaging, GitHub release, crates.io publish |
| **Mutants** | `mutants.yml` | PRs (incremental `--in-diff`), manual full sweep | Mutation testing; fails on any missed mutant, reports timeouts separately |

## Required status checks

Branch protection lives in repository settings, not in this tree — so this
section records which checks are *intended* to be merge-blocking on `main`.
If you administer the repo, keep Settings → Branches → required status
checks in sync with this list (job renames here silently drop the
requirement there):

- All `CI` jobs (Format, Clippy, Test, Documentation, cargo-deny, Package validation)
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

- **Rust versions**: stable + MSRV (1.93)
- **Platforms**: Linux, macOS, Windows
- **Feature combinations**: default, `signing`, `tracing`, `tls-rustls`,
  `sqlite`, `postgres`, `axum`, `--all-features`, `--no-default-features`
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
