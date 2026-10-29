<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Pitfalls for contributors

Part of [Pitfalls & Lessons Learned](../pitfalls.md). These concern working in this repository rather than using the SDK.

## Workspace / Cargo Pitfalls

### `cargo-fuzz` needs its own workspace

The `fuzz/` directory contains its own `Cargo.toml` with `[workspace]` to prevent cargo-fuzz from conflicting with the main workspace. The fuzz crate references `a2a-protocol-types` via a relative path dependency.

### Feature unification across workspace

Enabling a feature in one crate (e.g., `signing` in `a2a-protocol-types`) enables it for all crates in the workspace during `cargo test --workspace`. Use `--no-default-features` or per-crate test commands when testing feature gates.

## Testing Pitfalls

### Collection iteration order and determinism

The `InMemoryTaskStore` uses a `HashMap<TaskId, TaskEntry>` for O(1) get/save, plus a `BTreeMap` keyed by a monotonic per-write sequence and a `HashMap<String, BTreeMap<…>>` context index. `list` walks them in reverse, so tasks come back most-recently-updated first (spec §3.1.4) in O(log n + page\_size) with a collision-free cursor, without the O(n log n) per-call sort that previously caused 20-70× regressions at scale. Other in-memory stores (e.g., `InMemoryPushConfigStore`) also use a `HashMap`, whose iteration order is arbitrary. Always sort results before applying pagination or returning them to clients.

### Percent-encoded path traversal

Checking `path.contains("..")` is insufficient. Attackers can use `%2E%2E` (or mixed-case `%2e%2E`) to bypass the check.

**Solution:** The REST dispatcher percent-decodes the path, repeatedly (up to eight passes), *before* checking for a dot segment — `.` or `..` as a whole path segment, with `\` read as a separator — so `%2E%2E`, `%252E%252E` and `..\` are refused while an id such as `v1..prod` is served. A path still changing after eight passes is refused.

### A cargo-mutants baseline failure looks exactly like a clean file

`cargo mutants` writes `caught.txt`, `missed.txt` and friends whether or not it
tested anything. When the *baseline* build or test run fails — the unmutated
tree — it tests no mutants at all and every one of those files is empty. A
summary that reports counts alone prints `caught=0 missed=0`, which is
indistinguishable from a file with nothing left to kill.

Only the exit code separates them: `0` all caught, `2` surviving mutants, `3`
timeout, `4` baseline failed. Two baseline failures in one afternoon on this
repository were a full disk and a stopped Postgres — neither had anything to do
with the code under test, and both would have been recorded as a perfect score
by a reader who trusted the numbers.

**Solution:** always report the exit code next to the counts, and treat an
empty denominator as "did not run" rather than "passed". `mutants.yml` does
this at the aggregation step for the same reason.

### A mutation TIMEOUT is a third outcome, and the score hides it

`caught` and `missed` are not the whole result. A mutant that makes a test
*hang* rather than fail is reported `TIMEOUT`, and the usual score —
`caught / (caught + missed)`, which is what `mutants.yml` computes — puts it in
neither term. It is not a mutant the score counts against; it is a mutant the
score does not describe. `0 missed` and `0 timeouts` are different statements
and this repository has published the first as if it were the second.

Hangs are also the failure mode that reads worst in CI: the job wedges with no
assertion named, and under nextest's run-wide fail-fast a suite cannot exit
while a hung test is still running, so tests that *do* catch the mutant fail in
milliseconds and never get to report.

**Solution:** bound it at the harness, not test by test. `.config/nextest.toml`
kills any single test at 45 seconds, so the hang becomes a named failure and the
mutant is scored. Read the timeout column on every sweep, and treat a non-zero
one as unfinished work.

### The config file a tool ignores is worse than no config file

cargo-mutants 27.1.0 discovers `.cargo/mutants.toml`. This repository's
`mutants.toml` is at the workspace root, so none of it — file globs, exclusions,
timeout multiplier, cap — has ever applied to a run. It was believed to for
months, and three of its keys had individually been written off as "silently
ignored" without anyone asking why three unrelated keys would all misbehave.

Two checks tell you which situation you are in, and neither takes a full run:
compare `--list` output with and without `--no-config` (identical output means
the config is not being read, because disabling something inert changes
nothing), and pass the file explicitly with `--config <file>` (a config that is
never discovered can carry a fatal parse error indefinitely — this one does).

**Solution:** treat "is this configuration loaded?" as a question with an
answer, separate from "is this configuration right?" — the same distinction as
"can this gate fail?" versus "is this gate pointed at what it claims to cover?"
Assert it if the configuration matters.

### Matching CI's commands is not the same as matching CI

`scripts/preflight.sh` reads its gate list from `.github/workflows/ci.yml` so
the two cannot drift. Copying the commands alone still is not parity, because
this repository's CI also sets an `env:` block those commands run under:
`RUSTFLAGS: "-D warnings"` (a warning CI denies would pass locally without it)
and `CARGO_PROFILE_DEV_DEBUG: 0` (without which the all-features link can die
with `ld terminated with signal 7 [Bus error]` on a constrained machine — a
failure that reads as a broken test but is not one).

**Solution:** `preflight.sh` exports CI's top-level `env:` block, parsed from
the same file as the gates. Be aware that `RUSTFLAGS` and the profile setting
are part of cargo's fingerprint, so adopting them rebuilds a workspace that was
previously built without them.
