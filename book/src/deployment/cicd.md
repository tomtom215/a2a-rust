# GitHub Pages & CI/CD

a2a-rust uses GitHub Actions for continuous integration, crate publishing, and documentation deployment.

## CI Pipeline

The CI workflow (`.github/workflows/ci.yml`) runs on every push and PR:

| Job | Description |
|-----|-------------|
| **Format** | `cargo fmt --check` — enforces consistent formatting |
| **Clippy** | `cargo clippy -- -D warnings` — catches common mistakes |
| **Test** | `cargo test --workspace` — runs all tests |
| **Deny** | `cargo deny check` — audits dependencies for vulnerabilities |
| **Doc** | `cargo doc --no-deps` — verifies documentation builds |

The **Mutation Testing** workflow (`.github/workflows/mutants.yml`) runs separately:

| Mode | Trigger | Scope |
|------|---------|-------|
| **Full sweep** | On-demand (`workflow_dispatch`) | All library crates |

Nightly schedule and PR-gate triggers are currently disabled to save CI time.

The full sweep produces a mutation report artifact with caught/missed/unviable
counts and a mutation score. Zero missed mutants is required — any surviving
mutant fails the build.

All actions are **SHA-pinned** for supply chain security:

```yaml
- uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2
```

## Release Pipeline

The release workflow (`.github/workflows/release.yml`) triggers on version tags:

```
v0.2.0 tag → validate → ci + security → package → publish → github-release
```

Crates are published in dependency order:
1. `a2a-protocol-types` (no internal deps)
2. `a2a-protocol-client` + `a2a-protocol-server` (depend on types)
3. `a2a-protocol-sdk` (depends on all three)

## Documentation Deployment

The docs workflow builds the mdBook and deploys to GitHub Pages:

```yaml
# .github/workflows/docs.yml
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
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683
      - name: Install mdBook
        run: |
          mkdir -p $HOME/.local/bin
          curl -sSL https://github.com/rust-lang/mdBook/releases/download/v0.4.40/mdbook-v0.4.40-x86_64-unknown-linux-gnu.tar.gz | tar -xz -C $HOME/.local/bin
          echo "$HOME/.local/bin" >> $GITHUB_PATH
      - name: Build book
        run: mdbook build book
      - uses: actions/configure-pages@v5
      - uses: actions/upload-pages-artifact@v3
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
        uses: actions/deploy-pages@v4
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

Rust API docs are generated separately:

```bash
# Build API docs for all crates
cargo doc --workspace --no-deps --open
```

Consider deploying these alongside the book, or linking to docs.rs once published.

## Next Steps

- **[Configuration Reference](../reference/configuration.md)** — All configuration options
- **[Changelog](../reference/changelog.md)** — Version history
