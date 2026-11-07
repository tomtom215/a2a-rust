<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# 2. README and book, typed as written

Scripts: `02-readme-quickstart.sh`, `02-book-getting-started.sh`, `mkproject.py`
(builds a stranger's project from a page's own `toml` and `rust` blocks, dropping
the lines rustdoc and mdBook hide), `03-book-sweep.sh` with `mksweep.py` and
`sweep_classify.py`, `02-commands-static.py`, `book_blocks.py` (the inventory),
`07-doc-gates.sh`. Raw output: `evidence/02-*.log`, `evidence/03-book-sweep-*.log`.

Every project here was built **outside the repository, with no `RUSTFLAGS`, no lockfile
from the repository and the dependencies the page names**, against the unpacked tarballs
of `01-packaging.md` (`[patch.crates-io]`), by stable rustc 1.99.1. The inventory
(`book_blocks.py`): README plus the 217 pages `SUMMARY.md` links, 631 fenced blocks: 340
Rust (336 compiled by rustdoc, 4 `ignore`), 107 `bash`, 94 `text`, 32 `json`, 27 `toml`.

## Programs typed and run

| Source | Result | Notes |
|---|---|---|
| README Quick Start (agent plus client in one file; `[dependencies]` verbatim from the README) | **pass**: builds in 1 m 15 s with no warning; `cargo run` prints `Hello, Tom!`, then `artifact: <uuid>` and `status: Completed`, and exits 0 | The README says it "prints `Hello, Tom!` and then the streamed events": it does. Only `rt-multi-thread` and `macros` of tokio are enabled, as the README says |
| Installation, "Verifying the Installation" (dependencies verbatim) | **pass**: prints `Task state: Submitted`, a `Part` and the capabilities | The page promises no output, only "If this compiles and runs, you're ready" |
| Quick Start, "The Smallest Agent" (the page gives no dependency lines, so the Installation page's `a2a-protocol-sdk = "0.16"` and `tokio = { features = ["full"] }`) | **pass**: the page's `curl` returns the JSON the page shows (a `task` with an artifact `Hello, Tom!` and `TASK_STATE_COMPLETED`); without `A2A-Version: 1.0` the server answers `-32009` `VERSION_NOT_SUPPORTED` as the page says; `SIGTERM` exits 0 | The page's JSON is abridged and says so. The refusal text names version `0.3`, the spec's assumption for an absent header |
| Quick Start, "Running the Echo Agent" (`cargo run -p echo-agent`) | **pass**: endpoints, "Discovered 'Echo Agent' v1.0.0 — 4 interface(s)", one sweep per binding, five counter-tests refused, 44 of 44 cells, exit 0, in 2 s | Matches the page's five-part description line by line. `--features echo-agent/tracing` was not run (the name is a real package feature: `02-commands-static.py`) |
| Your First Agent, Steps 1 to 4 (`cargo new`, `[dependencies]` verbatim, Steps 1 to 3 as `src/main.rs`, Step 4 as `src/bin/client.rs`) | **pass with a defect**: `Result: Completed` / `Answer: 100`, as the page prints. Typed as written it prints two `unused import` warnings (finding D-4) | Fixed in this branch: `evidence/02-book-getting-started-after-fix.log` |

Three more builds of the README program, for the claims the README makes about features and the toolchain:

| Variant | Result |
|---|---|
| `a2a-protocol-sdk = { version = "0.16", default-features = false }` (the README says that opts the SDK out of rustls, logging, OpenTelemetry and the webhook receiver), `02b-readme-no-defaults.sh` | **pass**: builds in 1 m 21 s (81 packages locked, against 168 with the defaults), prints `Hello, Tom!` and `status: Completed` |
| `a2a-protocol-types` and `serde_json` only (the README sells the types crate as "no I/O"), a message round trip | **pass**: 12 packages locked, prints `{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"hello"}]}` and the assertion holds |
| rustc 1.94.0 (the MSRV), no lockfile, `02c-msrv.sh` | **pass**: `Locking 168 packages to latest Rust 1.94.0 compatible versions`, builds in 3 m 15 s, runs; and a `cargo check` with seven more SDK features (`grpc`, `websocket`, `sqlite`, `tool-loop`, `axum`, `signing`, `auth-jwt`) passes too |

## Compiled, not run: every other Rust block

`03-book-sweep.sh default` puts the README and every page into a library crate's doc
comments, so each Rust block is a doctest, and builds it outside the repository with
`a2a-protocol-{types,client,server,sdk} = "0.16"` at their **default features** (what the
pages' dependency lines give a reader who ignores the feature notes) plus the third-party
crates the blocks name. It is the check `book-tests/` cannot make, because
`book-tests/Cargo.toml` turns every feature on.

* 340 doctests: **270 compile and pass, 66 fail, 4 are `ignore`d**. Every one of the 66 is a
  missing Cargo feature (an `unresolved import`, or "no method named `build_grpc`"), on 35 pages
  (`evidence/03-sweep-default-classified.txt`).
* 8 of them are on migration guides (historical, they show the old code), 2 on the page for
  `a2a-protocol-discovery` (an unpublished crate that page says is "not published yet"), 15 on
  nine pages that **never name the feature the block needs**, and the rest on pages that name
  it (`stores/postgres.md` and `stores/sqlite.md` carry the dependency line, for example).
* **D-5**: the nine pages are the three tool-loop sub-pages, the WebSocket dispatcher page, the
  authorization page, the custom-store page and three horizontal-scaling pages. Fixed in this
  branch: each now says which feature it needs, and `scripts/check_feature_tables.py` fails when a
  page uses a feature-gated item without naming the feature.
* What this does **not** show: for the pages that do name a feature, whether the dependency line
  they show is complete. The CI check added in this branch only requires the page to mention the
  feature, in backticks or quotes. `book/src/deployment/oversight.md:151` (`approval::action_digest`)
  and `production.md:50` (`provenance::ContentSigner`) fail under the defaults and name
  `signing` on the page; I did not read each page's dependency line.
* The sweep with every feature on was not run as a doctest sweep: the disk quota could not hold
  that target directory. The tarballs were built with every feature on instead
  (`01-packaging.md`), and `book-tests/` already compiles every block with every feature in CI.

## Commands

`02-commands-static.py` reads the 107 `bash`, 6 `sh` and 6 `console` blocks of the README, the
book, the example READMEs, the CLI README and the observability README and checks that each
`cargo ... -p NAME`, `--bin`, `--example`, `--test`, `--bench`, `--features`, `--manifest-path`,
`docker/kubectl -f FILE` and `./scripts/...` names something that exists: **195 commands and
paths, no finding** (two reported lines are a shell variable, `$b`, in a loop in
`regression-gate.md`). It runs nothing. Commands actually run are in this file's table above, in
`03-examples.md`, `04-cli.md` and `06-deployment.md`.

## Prose claims checked

| Claim | Where | Result |
|---|---|---|
| "`A2A-Version: 1.0` is required ... refuses ... with `VERSION_NOT_SUPPORTED`" | quick-start.md | true |
| `Result: Completed`, `Answer: 100` | first-agent.md | true |
| About 50 lines of code above hello-agent's tests | README | 48 non-comment, non-blank lines above `#[cfg(test)]` |
| 20 alerts (14 on agents, 6 on the stack) | observability pages | 14 + 6 in the two rule files |
| 30 s header-read and 75 s idle timeout | first-agent.md, deploy-agent README | `DEFAULT_HEADER_READ_TIMEOUT` 30 s, `DEFAULT_IDLE_TIMEOUT` 75 s |
| `tls-rustls` is on by default for the client and the SDK | installation.md, README | **stale**: the server's default also holds it since 0.16.0 (D-3, fixed) |
| The README's conformance badge | README.md:23 | **stale**: 83/114, 5 failing against the paragraph's 85 of 115, 2 failing (D-1, fixed) |
| `serve()` as the startup helper | introduction.md, README | **stale**: deprecated since 0.16.0 (D-2, fixed) |
| `deploy-agent` `/health`, `/ready`, drain | README, deploy-agent README | true (`03-examples.md`) |

## Documentation gates

All twelve documentation gates `preflight.sh --list` names, and `mdbook build` with
`scripts/check_book_links.py`, exit 0 on the tree as found (`evidence/07-doc-gates.log`); they
did not see D-1 to D-5, which is the point of running the pages.
