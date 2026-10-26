<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Evidence for ADR 0028

[ADR 0028](../0028-webassembly-and-cloudflare-workers.md) proposes how the
server could run on Cloudflare Workers. This directory holds the experiment
behind it. **None of it is built by CI, and none of it is part of any crate.**
It is here because the container the experiment ran in is gone after the
session, and a measurement nobody can repeat is an opinion.

| File | What it is |
|---|---|
| `server-wasm32.patch` | The experiment: `git diff` against `6a82edcde4d5b2c84fe7e273e03738a6bec0f678` (147 files, `Cargo.lock` included). Applies to that commit; it will not apply to a later one without a rebase. |
| `handler-worker/` | A Worker that calls the real `RequestHandler`. Builds only with the patch applied. |
| `probe-worker/` | A Worker with one route per primitive (`/probe/<name>`), on `a2a-protocol-types` alone. Needs no patch; run it unpatched and patched to see the types crate's clock fail and then work. |
| `battery.py` | Eleven assertions against `handler-worker`; exit status is the number that failed. |
| `hold_probe.py` | One request on a fresh isolate, for the `ctx.wait_until` A/B. |
| `clippy-wasm-traps.toml` | The `disallowed-methods` list ADR 0028 proposes as a gate. |
| `count_sites.sh` | Counts, per crate, the lines that touch a native-only runtime API. Upper bounds: inline test modules are not excluded. |
| `reproduce.sh` | Everything above, from a clean worktree, in one command. |
| `results.txt` | What the commands printed when they were run, with the toolchain versions. Every figure the ADR quotes is in it. |

## Reproduce

```sh
# once: rustup target add wasm32-unknown-unknown; cargo install worker-build@0.8.7
docs/adr/0028-wasm-spike/reproduce.sh /some/empty/dir
```

It needs `git`, `clang` (for `ring`), `python3`, `node`, and network access
for crates.io and `npx wrangler@4.152.0` (set `WRANGLER=` to use an installed
one). It builds about 1.5 GB under the directory you give it and prints
`PASS` or `FAIL` per check. `SKIP_NATIVE_TESTS=1` skips the 1,545-test native run.

The Workers are not deployed anywhere: `wrangler dev --local` runs the
`workerd` binary on loopback.

## Caveats

- The two Worker manifests carry no `Cargo.lock`; the versions in
  `results.txt` are what resolved on 2026-10-26.
- `handler-worker` calls `RequestHandler` directly through a JSON-RPC front it
  writes for itself. The dispatchers cannot be called from a Worker (ADR 0028,
  finding 6), so the dispatch layer is untested here.
- Two edits in the patch exist only for the experiment: `pub mod rt` with a
  public `rt::sleep` (so the test agent can wait on a timer), and
  `RequestHandler::wait_idle`. ADR 0028 argues for the second; the first is
  not a design.
