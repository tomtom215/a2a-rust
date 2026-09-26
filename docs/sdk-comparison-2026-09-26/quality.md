<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Static and quality measurements

> Appendix to [`README.md`](README.md) §3.8. Raw outputs are in
> [`results/quality/`](results/quality/). The measuring agent ran every
> command with the same flags for both projects. The test totals, the
> MSRV refusal and the RustSec IDs were then re-derived from those raw logs
> by a second pass. The remaining figures are the agent's own measurements
> and were not re-derived.

**Subjects:**
- **a2a-rs** at `365d056`, the commit its six latest crates.io releases were
  cut from.
- **a2a-rust** at `10f3435`, the 0.14.0 release commit.

**Toolchain:** Rust 1.94.1, cargo-audit 0.22.2 (RustSec database of
2026-09-26), cargo-deny 0.20.2, tokei 15.0.0, cargo-geiger 0.13.0.

## Headline table

| # | Metric (how obtained) | a2a-rs | a2a-rust |
|---|---|---|---|
| 1 | Published `.crate` vs git tree, byte compare | identical, 6/6 crates | identical, 4/4 crates |
| 2a | `cargo test --workspace`, default features | 661 passed / 0 failed / 0 ignored ¹ | 4,132 / 0 / 112 ² |
| 2b | the same, `--all-features` | 661 / 0 / 0 ¹ | 4,142 / 0 / 112 ² |
| 3a | `cargo clippy --workspace --all-targets -- -D warnings` | pass | pass ³ |
| 3b | `-W clippy::pedantic`, unique warnings in hand-written library `src/` | 271 | 0 ³ |
| 4 | `cargo fmt --all --check` | pass | pass |
| 5a | `cargo audit`, committed lockfile | RUSTSEC-2026-0285 (rustls 0.23.43, medium), plus warnings: RUSTSEC-2025-0134 (unmaintained), one yanked crate | RUSTSEC-2023-0071 (rsa, medium), plus a RUSTSEC-2024-0436 unmaintained warning |
| 5b | Reachable from the library crates (minimal consumer + `cargo tree -i`) | RUSTSEC-2026-0285, only through `a2a-slimrpc` ⁴ | none ⁵ |
| 6 | cargo-deny: licenses, advisories, bans, sources; same `deny.toml` for both | client+server+types, pb, grpc pass; `a2a-slimrpc` fails (licenses MIT-0 / CC0 / none declared; advisory above) | all pass, including all features |
| 7 | Unique crates, client+server+types, fresh resolution (`cargo tree -e normal`) | 153 default / 161 all features | 69 default / 207 all features |
| 8 | `unsafe` in published `src/` and `build.rs` | 0 in `src/`; 1 in `a2a-pb/build.rs`; no `#![forbid(unsafe_code)]` | 0 in `src/`; 3 (`build.rs` `set_var("PROTOC")`); `#![forbid(unsafe_code)]` in all 4 crates |
| 9a | `cargo rustdoc -p <crate> -- -W missing_docs`, unique warnings | 498 across 6 crates | 0 (`#![deny(missing_docs)]`) |
| 9b | `cargo doc --no-deps` warnings | 4 | 0 |
| 9c | docs.rs build status of the latest versions | 6/6 built | 4/4 built |
| 10 | Library `src/` Rust code lines (tokei) | 17,510 hand-written + 1,736 generated | 76,501 |
| 11 | Fuzz targets (listed, not built) | 1 | 13 |
| 12 | Declared MSRV, `cargo +MSRV check --locked` with the committed lockfile | 1.85: only `a2a-lf` passes; the other 5 are refused by dependency MSRVs (icu 2.2.0 needs 1.86, tonic 0.14.6 needs 1.88) | 1.88: all 4 pass |
| 12b | Same, MSRV-aware fresh lockfile | `a2a-lf` and `a2a-pb` pass. client/server/grpc fail on `yoke-derive` 0.8.3, which uses a 1.87 API and declares no `rust-version`. | all 4 pass |
| 13 | `cargo-semver-checks` in CI | no workflow job; release-plz's default semver check was not verified against a run | blocking job, 4 crates, all features |

Footnotes:

1. a2a-rs's non-default `itk` member needs a system `protoc`, so the agent
   excluded it with `--exclude itk-rust-current-agent`.
2. The 112 ignored tests break down as:
   - 81 need PostgreSQL (none was provided);
   - 17 are load or soak tests;
   - 14 are doctests marked `ignore`.
3. The clippy comparison is not like-for-like. a2a-rust's library crates turn
   on `clippy::pedantic` and `clippy::nursery` and deny `missing_docs` in
   source, so those counts reflect a lint policy the other project does not
   have.
4. The a2a-rs repository documents this pin, which is caused by a SLIM
   dependency holding `aws-lc-rs =1.16.2`.
5. The rsa advisory arrives through `sqlx-mysql`, which no feature enables.

## Reading these numbers fairly

- **Size is not quality.** a2a-rust has about 4.4× more library code and 6.3×
  more tests. That is a cost as well as a capability: every one of those
  lines is code someone has to review. The claims audit (report §3.7) found
  four defects in exactly that surface, despite the test count.
- **Default footprint follows the defaults.** a2a-rs's client and server
  turn on reqwest, axum and rustls by default. a2a-rust's server defaults to
  raw hyper with only `tracing`. With every feature turned on, the
  dependency graph reverses: 207 crates for a2a-rust against 161 for a2a-rs.
- **a2a-rs misses its declared MSRV partly because of other crates.**
  `yoke-derive` 0.8.3 declares no `rust-version`, so an MSRV-aware resolver
  cannot avoid it. a2a-rust's resolver-3 workspace and MSRV CI job make its
  claim hold, but the same upstream hazard applies to it too.
- **Not measured:** the fuzz targets were not built (no nightly toolchain);
  the PostgreSQL tests were not run; and code generated into `OUT_DIR` is not
  counted.
