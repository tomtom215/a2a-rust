<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Draft: list a2a-rust on the ITK dashboard, and let the driver find a non-`a2aproject` repo

**Status: draft, not sent.** Target: `a2aproject/a2a-itk`, an issue followed
by a pull request. The maintainer decides whether and when to send it.

Read against `a2aproject/a2a-itk` at
`82458ceac60208cc46ccbe52b68dbb8e1ca2614a` (2026-10-08).

## What is already in place on our side

- **Nightly traversal.** `tomtom215/a2a-rust` runs the shared nightly
  traversal set through `scripts/run_itk_shared.sh`
  (`.github/workflows/itk-nightly.yml`). It publishes `itk_rust.json` to a
  rolling `nightly-metrics` prerelease, exactly as
  `docs/interoperability/04_ci.md` describes.
- **Nightly ACTS.** As of 2026-10-09 the same workflow runs ACTS nightly on
  JSON-RPC, gRPC and HTTP+JSON, and publishes `acts_rust.json` beside it.
- **ACTS as a merge gate.** ACTS also runs as a merge-blocking job at a pinned
  revision (`.github/workflows/acts.yml`). The 2026-10-08 reports are
  committed under `acts/reports/2026-10-08`: 101/101 on JSON-RPC, 88/88 on
  gRPC and 94/94 on HTTP+JSON, at every level.

## Three things stop it from appearing

1. **The driver assumes the `a2aproject` org.**
   `scripts/run_itk_shared.sh` builds the history URL as
   `https://github.com/a2aproject/${ITK_SDK_REPO}/releases/download/nightly-metrics/…`
   for both `itk_` and `acts_` (lines 271–272 at the revision above, and the
   `process_results.py` call). For a repository outside the org that URL
   404s. Each night then starts from an empty history, so the rolling window
   never fills.
   - **Proposal:** an `ITK_SDK_OWNER` variable, defaulting to `a2aproject`,
     used in both URLs. The shim sets `ITK_SDK_OWNER=tomtom215`.
2. **The dashboard's fetch has one base URL.**
   `dashboard/scripts/fetch-metrics.sh` sets
   `base=https://github.com/a2aproject`, and `dashboard/src/shared/sdks.ts`'s
   `commitUrl` does the same.
   - **Proposal:** an optional `owner` on `SdkTarget`, defaulting to
     `a2aproject`, and an owner argument to `fetch`.
3. **The id `rust` is a2a-rs.**
   - **Proposal:** a separate entry with id `rust-a2a-rust`, label
     `Rust (a2a-rust)`, repo `a2a-rust`, owner `tomtom215`, and files
     `itk_rust-a2a-rust.json` and `acts_rust-a2a-rust.json`, fetched from our
     `itk_rust.json` and `acts_rust.json`.

## Proposed patch, in outline

```sh
# scripts/run_itk_shared.sh
ITK_SDK_OWNER="${ITK_SDK_OWNER:-a2aproject}"
#   …/github.com/${ITK_SDK_OWNER}/${ITK_SDK_REPO}/releases/download/nightly-metrics/…

# dashboard/scripts/fetch-metrics.sh
fetch() {  # <local name> <owner/repo> <remote asset>
  … "https://github.com/$2/releases/download/nightly-metrics/$3"
}
fetch itk_rust-a2a-rust.json  tomtom215/a2a-rust itk_rust.json
fetch acts_rust-a2a-rust.json tomtom215/a2a-rust acts_rust.json
```

The tests that pin the current behaviour are
`tests/test_matrix.py`, `dashboard/src/acts/lib.test.ts` and
`tests/test_scenarios_diff.py`; the patch updates them alongside.

## Not asked for

- A place in `matrix.yaml` as a peer. Whether other SDKs should run against
  this one nightly is a separate question for the ITK maintainers.
- Any change to scenarios or scoring.
