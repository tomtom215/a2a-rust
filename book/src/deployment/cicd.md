# GitHub Pages & CI/CD

a2a-rust uses GitHub Actions for continuous integration, crate publishing, and documentation deployment.

## CI Pipeline

The CI workflow (`.github/workflows/ci.yml`) runs on pushes to `main` and `claude/**` and on PRs to `main`:

| Job | Description |
|-----|-------------|
| **Static checks** | `cargo fmt --all -- --check`, vendored-proto agreement, the 500-line file-length ratchet, no process-global panic hooks, the mutants-config check, and gate-inventory completeness |
| **Clippy** | `cargo clippy` per feature combination (default, signing, tracing, tls-rustls, sqlite, postgres, axum, websocket, grpc, auth-jwt without default features, all-features) across 3 OSes (ubuntu, macOS, Windows) on stable (lint verdicts are a property of the clippy version; the MSRV leg of the Test job is what proves 1.88 compatibility) |
| **Feature matrix** | `cargo hack clippy --each-feature` over the four published crates — every feature on its own, plus no-default-features and all-features — so a `#[cfg(feature)]` gap is caught here rather than by a downstream build enabling an unusual subset |
| **Test** | `cargo test --workspace` per feature combination (default, signing, tracing, tls-rustls, sqlite, postgres, axum, websocket, grpc, auth-jwt, auth-jwt + tls-rustls, all-features, no-default-features) across 3 OSes and 2 Rust versions |
| **Test (postgres integration)** | Runs the `#[ignore]`-gated live-database suite (`postgres_store_tests.rs`) against a `postgres:16` service container |
| **Nightly** | Tests and clippy on the nightly Rust toolchain for early compatibility checks (`continue-on-error: true` — non-blocking) |
| **Deny** | `cargo deny check` — audits dependencies for vulnerabilities |
| **Doc** | `cargo doc --workspace --no-deps`, then each published crate on its own in its own default features — verifies documentation builds |
| **Package** | `cargo package --workspace` (excluding example and tool crates) — validates crate packaging for publish |
| **Semver** | `cargo-semver-checks` over the four published crates, all features |
| **SDK dogfood** | `cargo run -p agent-team --release --all-features` |
| **SLIMRPC binding** | builds the out-of-workspace binding and runs `cargo-deny` over its own dependency tree |
| **Example surface coverage** | runs the examples (echo-agent, incident-response, genai, rig, multi-lang-team), each driving every method over every binding it supports |
| **Go SDK interop** | `scripts/go_sdk_interop.sh` — both directions, three bindings |

The **Coverage** workflow (`.github/workflows/coverage.yml`) runs on pushes to `main` and `claude/**`, on PRs, on demand, and weekly (for the `codecov.yml` ignores-applied check):
- Uses `cargo-llvm-cov` for source-based coverage instrumentation
- Generates LCOV reports and uploads to [Codecov](https://codecov.io/gh/tomtom215/a2a-rust)

<a id="mutation-testing-workflow"></a>
The **Mutation Testing** workflow (`.github/workflows/mutants.yml`) runs separately:

| Mode | Trigger | Scope |
|------|---------|-------|
| **Full sweep** | Weekly (Mondays 03:00 UTC) + on-demand (`workflow_dispatch`) | All library crates, sharded across parallel runners (12-way for `a2a-server`, 4-way for `a2a-types` and `a2a-client`) |

Every pull request additionally runs an **incremental** mutation gate:
`cargo-mutants --in-diff` mutates only the source lines changed in the PR
and fails on any missed mutant — this one *is* a blocking PR check, enforced
on every commit. The full sweep is the one that doesn't run on every
commit: a full sweep can take 100+ minutes per crate and a2a-server alone
generates hundreds of mutants, so it runs on its own weekly schedule (plus
`workflow_dispatch` for an on-demand run against `main`) rather than
blocking PRs.

The full sweep produces a mutation report artifact with caught/missed/unviable
counts and a mutation score. The workflow is configured to fail on surviving
mutants, so a clean weekly run confirms that every caught mutant across the
whole codebase — not just the surface area of recent PRs — is covered by at
least one test. The report artifact itself only survives 90 days; see
[Mutation Testing History](../reference/mutation-history.md) for the
dated, durable record each sweep should be copied into.

The **Benchmarks** workflow (`.github/workflows/benchmarks.yml`) runs on-demand (`workflow_dispatch`) and on pushes to `main` that affect benchmark or SDK code. It:

1. Builds and runs 13 of the 15 benchmark suites individually via Criterion.rs (`coordinator_chain_under_fault` and `send_latency_breakdown` are run by hand)
2. Auto-generates the [benchmark results page](../reference/benchmarks.md) via `benches/scripts/generate_book_page.sh`
3. Auto-generates the [interactive benchmark dashboard](../reference/dashboard.md) via `benches/scripts/generate_dashboard.sh`
4. Commits the updated results page and dashboard to `main` via `github-actions[bot]` (skipped while the in-tree version has no release tag)
5. Archives the full criterion HTML reports (violin plots, comparison overlays) as workflow artifacts with 30-day retention

On PRs that touch benchmark or SDK code, a separate **Regression Gate** job compares `transport_throughput` and `protocol_overhead` against the base branch.

The 14 benchmark suites cover: transport throughput (payload scaling to 1MB), protocol overhead (including `protocol/payload_scaling` isolation benchmarks for serde regression detection), task lifecycle, concurrent agents, cross-language comparison, realistic workloads, error paths, streaming and backpressure, data volume scaling (with cache-busting), memory overhead, enterprise scenarios, production scenarios, advanced scenarios, and — new in this release — **agent-level latency under fault** via an in-process 5-hop coordinator chain with fault injection at every link. The last suite is the first benchmark on this page that does not measure SDK-layer overhead; see the [Agent-Level Latency Under Fault](../reference/benchmarks.md#agent-level-latency-under-fault) section for the honest caveats.

The **TCK** workflow (`.github/workflows/tck.yml`) runs the Technology Compatibility Kit on pushes to `main` and PRs. It tests the echo-agent (self-test) and runs cross-language conformance tests against Python, JavaScript, Go, and Java agent implementations with both JSON-RPC and REST bindings.

All actions are **SHA-pinned** for supply chain security:

```yaml
- uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6.0.2
```

## Release Pipeline

The release workflow (`.github/workflows/release.yml`) triggers on version tags:

```text
vX.Y.Z tag → validate → ci + security → package + publish-dry-run → github-release → publish
```

The `grpc` feature compiles the canonical schema with `prost-build`/`tonic-prost-build` using the vendored `protoc` (`protoc-bin-vendored`); no workflow installs a system `protoc`. Set `PROTOC` to use your own.

Crates are published in dependency order:
1. `a2a-protocol-types` (no internal deps)
2. `a2a-protocol-client` + `a2a-protocol-server` (depend on types)
3. `a2a-protocol-sdk` (depends on all three)

## Documentation Deployment

The docs workflow builds the mdBook and deploys to GitHub Pages. Abridged — the
toolchain, cache and API-documentation steps are omitted; see the workflow file
for the whole of it:

```yaml
# .github/workflows/docs.yml (abridged)
name: Deploy Documentation

on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: "pages"
  cancel-in-progress: false

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # v6.0.2
      - name: Install mdBook
        run: |
          mkdir -p "$HOME/.local/bin"
          curl -sSL https://github.com/rust-lang/mdBook/releases/download/v0.4.40/mdbook-v0.4.40-x86_64-unknown-linux-gnu.tar.gz \
            | tar -xz -C "$HOME/.local/bin"
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - name: Build book
        run: mdbook build book
      - name: Copy static assets into build output
        run: |
          set -euo pipefail
          cp -r --no-preserve=mode book/static/. book/book/
          test -f book/book/robots.txt
          test -f book/book/sitemap.xml
      - uses: actions/configure-pages@983d7736d9b0ae728b81ab479565c72886d7745b # v5.0.0
      - uses: actions/upload-pages-artifact@7b1f4a764d45c48632c6b24a0339c27f5614fb0b # v4.0.0
        with:
          path: book/book

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - id: deployment
        uses: actions/deploy-pages@d6db90164ac5ed86f2b6aed7e0febac5b3c0c03e # v4.0.5
```

### Setting Up GitHub Pages

1. Go to **Settings → Pages** in your GitHub repo
2. Set **Source** to "GitHub Actions"
3. Push to `main` to trigger the first deployment
4. Your docs will be live at `https://tomtom215.github.io/a2a-rust/`

### Building Locally

```bash
# Install mdBook
cargo install mdbook

# Build
mdbook build book

# Serve with hot reload
mdbook serve book --open
```

## Cargo Documentation

The docs workflow also builds rustdoc for the four published crates
(`--all-features`, `--cfg docsrs`) and publishes it under `/api/` beside the
book. To build it locally:

```bash
# Build API docs for all crates
cargo doc --workspace --no-deps --open
```

## Next Steps

- **[Configuration Reference](../reference/configuration.md)** — All configuration options
- **[Changelog](../reference/changelog.md)** — Version history
