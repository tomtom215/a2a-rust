<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# CI/CD Workflows

GitHub Actions workflows for the a2a-rust project.

## Workflows

| Workflow | File | Trigger | Purpose |
|----------|------|---------|---------|
| **CI** | `ci.yml` | Push to `main`, PRs | Format, clippy, tests (5 feature combos), docs, cargo-deny, MSRV |
| **TCK** | `tck.yml` | Push to `main`, PRs | Conformance tests via TCK; cross-language ITK tests (Docker) |
| **Coverage** | `coverage.yml` | Push to `main`, PRs | Code coverage via `cargo-llvm-cov`, Codecov upload |
| **Documentation** | `docs.yml` | Push to `main` | Build mdbook + rustdoc, deploy to GitHub Pages |
| **Benchmarks** | `benchmarks.yml` | Push to `main`, manual | Run criterion benchmarks, generate book page, auto-commit |
| **Release** | `release.yml` | Tag push (`v*`) | CI matrix, security audit, crates.io publish, GitHub release |
| **Mutants** | `mutants.yml` | Manual (nightly schedule) | Per-crate mutation testing sweeps |

## CI Matrix

The CI workflow tests across multiple configurations:

- **Rust versions**: stable + MSRV (1.93)
- **Feature combinations**: default, `signing`, `tls-rustls`, `tracing`, all features
- **Checks**: `cargo fmt`, `cargo clippy`, `cargo test`, `cargo doc`, `cargo deny`

## Running Locally

```bash
# Reproduce CI checks locally
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo doc --workspace --no-deps
```

## Benchmark Automation

The benchmarks workflow runs all 12 benchmark modules, generates a Markdown results page, and commits it to `book/src/reference/benchmarks.md`. This triggers the docs workflow to redeploy GitHub Pages with fresh numbers.

## License

Apache-2.0
