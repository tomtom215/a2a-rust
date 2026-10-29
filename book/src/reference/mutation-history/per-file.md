<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Per-file measurements

Part of [Mutation Testing History](../mutation-history.md).

A full workspace sweep needed 21 CI runners when this was measured (69 since
2026-10-20: 60 for `a2a-server`, 4 each for `a2a-types` and `a2a-client`, 1 for
`a2a-sdk`); a single 4-core machine projects to well over a day for `a2a-server`
alone (measured: 37 of 2113 mutants in ~40 minutes). Per-file sweeps are the
practical unit for burning down a cluster, and they answer the question a stale
survivor list cannot: *is this still true of `HEAD`?*

All rows below were run on 2026-08-09 (the `a2a-protocol-client` row on
2026-08-10) against this branch, with a live Postgres and `--run-ignored all`.
Exit codes are quoted because they are the only reliable signal — `0` all
caught, `2` survivors, `4` baseline failed.

| File | Mutants | Caught | Missed | Unviable | Exit | Was |
|---|---:|---:|---:|---:|---:|---:|
| `dispatch/axum_adapter.rs` | 49 | 49 | **0** | 0 | **0** | 7 |
| `dispatch/grpc/service.rs` | 44 | 44 | **0** | 0 | **0** | 9 |
| `streaming/event_queue/in_memory.rs` | 50 | 19 | **0** | 31 | **0** | 7 |
| `agent_card/caching.rs` | 112 | 110 | **0** | 2 | **0** | — |
| `store/task_store/in_memory/eviction.rs` | 17 | 17 | **0** | 0 | **0** | 2 (excluded) |
| `dispatch/websocket.rs` | 33 | 33 | **0** | 0 | **0** | 7 |
| `push/sender.rs` | 99 | 64 | **0** | 35 | **0** | 7 |
| `rate_limit.rs` | 56 | 32 | **0** | 24 | **0** | 5 |
| `handler/event_processing/sync_collector.rs` | 27 | 14 | **0** | 13 | **0** | 9 |
| `a2a-protocol-types` (whole crate) | 674 | 605 | 8 | 61 | 2 | 8 |
| `a2a-protocol-client` (whole crate) | 804 | 357 | 10 | 437 | 2 | 10 |

Across these `a2a-server` files: **53 survivors addressed, none remaining** —
the sum of the `Was` column, or 60 counting the 7 that the 2026-08-03 artifacts
show for `agent_card/caching.rs`, which has no `Was` figure.
Every one of the nine reports exit 0, and nothing was excluded anywhere in the
project at the time — no `--exclude-re`, no `#[mutants::skip]`, no baselined
exception. (Since 2026-09-10 there are `#[mutants::skip]` attributes — three
then, nine in `a2a-protocol-server` as of 2026-10-24 — none in these nine
files; see ADR 0006.)

**What that sentence does not cover.** Nine files is not the crate.
`a2a-server` has 2113 mutants; these nine account for 487 of them. Treat "zero
survivors" as a claim about the nine rows, not the crate.

**The weekly sweep ran on 2026-08-10** (scheduled 03:33 UTC, `041c3666` on
`main`) and was, when this was written, the current whole-repo figure (the
newest is the top row of the [ledger](./sweeps.md#the-ledger)):

| | Caught | Missed | Timeout | Unviable | Score |
|---|---:|---:|---:|---:|---:|
| All crates, 2026-08-10 | 2187 | 125 | 2 | 1277 | **94%** |

It exited 1, which is the workflow working: 125 survivors, reported rather
than rounded away. Ten of the twelve `a2a-server` shards carried survivors;
shards 3 and 8 came back clean.

**The per-crate split, recovered 2026-08-10** by re-counting run
[31352927429](https://github.com/tomtom215/a2a-rust/actions/runs/31352927429)'s
21 shard artifacts with the workflow's own counting rule (non-empty lines).
This closes the gap the paragraph here previously described — the ledger now
states a whole-crate `a2a-server` score:

| Crate | Caught | Missed | Timeout | Unviable | Score |
|---|---:|---:|---:|---:|---:|
| `a2a-server` | 1225 | 107 | 2 | 779 | **91%** |
| `a2a-client` | 357 | 10 | 0 | 437 | 97% |
| `a2a-types` | 605 | 8 | 0 | 61 | 98% |
| `a2a-sdk` | 0 | 0 | 0 | 0 | n/a — pure re-export facade |
| **combined** | **2187** | **125** | **2** | **1277** | **94%** |

The combined row reproduces the CI figure exactly, which is what makes the
split trustworthy: the same arithmetic that yields 94% yields these four rows.
`a2a-server`'s 2113 total (1225 + 107 + 2 + 779) also matches the count quoted
in "What that sentence does not cover" above.

## The 125 is measured on a commit that predates PR #103

Read the headline with this attached. `041c3666` is **44 commits behind**
`af7a1f8`; the scheduled sweep ran at 03:33 UTC and PR #103 merged later the
same day. So the 125 describes a tree that no longer exists.

Quantified rather than asserted: **61 of the 125 survivors (49%) sit in files
that changed between `041c3666` and `af7a1f8`.**

| Survivors | File (`a2a-server`) | In the per-file table above? |
|---:|---|---|
| 9 | `dispatch/grpc/service.rs` | yes — driven to 0 |
| 9 | `handler/event_processing/sync_collector.rs` | yes — driven to 0 |
| 8 | `handler/event_processing/background/state_machine.rs` | **no** |
| 8 | `streaming/event_queue/in_memory.rs` | yes — driven to 0 |
| 7 | `dispatch/axum_adapter.rs` | yes — driven to 0 |
| 7 | `dispatch/websocket.rs` | yes — driven to 0 |
| 7 | `push/sender.rs` | yes — driven to 0 |
| 6 | `rate_limit.rs` | yes — driven to 0 |

Seven of those eight are files the per-file sweeps above already took to zero
survivors on 2026-08-09 — against the branch that became PR #103, which the
weekly sweep had not yet seen. The counts line up closely with that table's
`Was` column but not exactly (`in_memory.rs` 8 vs 7, `rate_limit.rs` 6 vs 5),
so the two were not measured at the same commit and the overlap must not be
treated as an identity.

**What this does and does not license.** It does not license subtracting 61
and claiming 64: the per-file runs and the sweep are different commits, and
rewritten code generates new mutants as readily as it retires old ones. The
only honest statement is that the current-`main` survivor count is **not
measured**, that 125 is an upper bound carried from a superseded tree, and
that the next scheduled sweep is what settles it. That sweep, not this
paragraph, is the thing to check.

The remaining **64 survivors are in files unchanged since `041c3666`**, so
those are valid against current `main` and are the ones worth triaging today.

### Re-checked 2026-08-12 at `6ebf821` — and the sweep was NOT re-run

Stated plainly, because the distinction is the whole point of this page: the
mutation sweep was **NOT RUN** in the 2026-08-12 session. Not "unchanged", not
"still 125" — **not measured**. The machine had 4 cores (`nproc`), and a sweep
that needs 21 CI shards does not fit there in a session; running a partial one
and reporting it beside a full one is how denominators move silently. So the
headline row of the [ledger](./sweeps.md#the-ledger) is still `041c366`'s, and `git rev-list --count
041c366..6ebf821` puts it **91 commits behind** `main` — up from the 44 recorded
above against `af7a1f8`.

What *was* re-measured is the staleness itself, which is cheap and does not need
a sweep:

* All **eight** named survivor-cluster files still changed between `041c366` and
  `6ebf821`, so the "61 of 125 sit in changed files" half holds unchanged.
* The set of changed `crates/*/src` sources grew from **10** files at `af7a1f8`
  to **19** at `6ebf821` — nine more files entered it (counted with
  `:(glob)crates/*/src/**/*.rs`; the pathspec without `:(glob)`, which is blind
  to files directly under `src/`, gives 9 and 13). Since the 64 "still valid"
  survivors were never enumerated per file, it cannot be said which of them
  those nine touch. **64 is therefore an upper bound at `6ebf821`, not a
  current count**, and the same refusal to subtract applies as above.

The honest position is unchanged and now further from its evidence: the
current-`main` survivor count is not measured, 125 is an upper bound carried
from a tree 91 commits back, and the next full sweep settles it.

## Triage of the 64 still-valid survivors (2026-08-10)

| Bucket | Count | Disposition |
|---|---:|---|
| Body-size limit boundaries (`> max` at exactly `max`) | 6 | real — one boundary test per call site kills two each |
| Whole-function replacements with no asserted return | ~18 | real — names an untested layer, per the section above |
| `> / >= / ==` and `+ / -` off-by-one arithmetic | ~14 | real, mostly cheap |
| `Debug`/`Display` `fmt` impls replaced | 3 | low value — needs assertions on formatted output |
| `authenticates -> bool` on interceptor impls | 3 | real — no test asserts the flag |
| Logging-only (`warn_unrecognized_params`) | 2 | **equivalent unless log output is asserted**; the function has no effect but a trace event |
| `serve` / process-lifecycle replaced with `Ok(())` | 4 | hard — the servers block; no test asserts they actually serve |
| `days_from_civil` negative-year branch | 2 | **equivalent** — see below |
| remainder | 12 | not individually classified |

**Eight of these were killed on 2026-08-10** (see the commit adding the tests):
three in `signing.rs` canonicalization depth, four in
`handler/capability.rs::activated_extensions`, one in
`parse_iso8601_to_unix_millis`. Each was verified by re-applying the mutant by
hand and confirming the new test goes red — a test written to kill a mutant,
without checking that it does, is the unverified green this page is about.

**Two are classified equivalent rather than fixed.** Both `days_from_civil`
mutants sit on the `y < 0` branch of
`let era = if y >= 0 { y } else { y - 399 } / 400;`. That branch is reachable
only for year 0 with a January/February date, and every input reaching it is
pre-epoch — which `parse_iso8601_to_unix_millis` rejects afterwards via
`(total >= 0).then_some(total)`, under the mutant as much as under the
original. No observable difference through the public API, so there is nothing
to test. Checked, not assumed: both mutants were reasoned through to a
returned `None` either way.

The remaining caution on the combined row stands: it was measured on `main`,
not on any branch in progress.

An earlier revision of this section said the honest statement was "not
measured since 2026-08-03". That was true when written and false within
hours — the Monday sweep it did not account for had already run. A ledger
whose freshness claims are hand-maintained will keep going stale like this;
the run date above is the thing to check, not the prose.

The two whole-crate rows are current and were produced on this branch:
`a2a-protocol-types` on 2026-08-09 (674 mutants, exit 2) and
`a2a-protocol-client` on 2026-08-10 (804 mutants, 37 minutes, exit 2). Both
still carry survivors; they are listed below rather than left implicit.

Both reproduce the 2026-08-03 recovered counts exactly — `a2a-client` 357/10
and `a2a-types` 605/8, matching the table above to the mutant. Two things
follow. The recovered figures were not an artefact of the archaeology: an
independent sweep a week later, on a different machine, agrees. And these
survivors are long-standing rather than newly introduced — neither crate's
tested behaviour has moved in the interval.

## Standing survivors outside `a2a-server`

Recorded so the count is visible rather than inferred from a missing table.
Neither crate has been burned down; these are the live lists as of the sweeps
above.

`a2a-protocol-client` — 10:

| Location | Mutation |
|---|---|
| `error.rs:126` | `parse_retry_after` → `None` |
| `error.rs:126` | `parse_retry_after` → `Some(Default::default())` |
| `token_provider.rs:86` | `*` → `+` |
| `token_provider.rs:136` | `Debug for StaticTokenProvider::fmt` → `Ok(())` |
| `token_provider.rs:174` | `Debug for BearerAuthInterceptor::fmt` → `Ok(())` |
| `token_provider.rs:403` | `<` → `<=` in `OAuth2ClientCredentials::cached` |
| `transport/mod.rs:50` | `*` → `+` |
| `transport/mod.rs:76` | `>` → `==` in `collect_response_limited` |
| `transport/mod.rs:76` | `>` → `>=` in `collect_response_limited` |
| `transport/jsonrpc.rs:303` | `delete !` in `JsonRpcTransport::execute_request` |

`a2a-protocol-types` — 8:

| Location | Mutation |
|---|---|
| `lib.rs:200` | `+` → `-` in `parse_iso8601_to_unix_millis` |
| `lib.rs:254` | `-` → `+` in `days_from_civil` |
| `lib.rs:254` | `-` → `/` in `days_from_civil` |
| `signing.rs:87` | `>` → `==` in `write_canonical` |
| `signing.rs:87` | `>` → `>=` in `write_canonical` |
| `signing.rs:142` | `+` → `*` in `write_canonical` |
| `proto/convert/mod.rs:67` | `Display for ConvertError::fmt` → `Ok(())` |
| `proto/convert/mod.rs:165` | `<` → `<=` in `timestamp_to_rfc3339` |

The last three fell to the same move rather than to cleverer tests: each was a
decision buried behind machinery a test cannot drive, so the decision was moved
somewhere it could be reached.

| Was standing | How it died |
|---|---|
| `push/sender.rs` — scheme-to-port default | Sat behind a DNS lookup whose only successful outcome needs a hostname resolving to a *public* address, so a hermetic test always errored before the port was observable. Extracted as `webhook_port`, a pure mapping. The inverted form pins an https webhook to port 80 and delivers it in cleartext, so it was worth reaching. |
| `rate_limit.rs` — window comparison in the write-lock double-check | Reachable only through a genuine race between callers. Extracted as `admit_or_roll_window`, taking the bucket directly, it is an ordinary state transition; two tests pin both arms. |
| `websocket.rs` — sign of the back-pressure `-32000` | Not the timing test it looks like: the permit is taken before the handler task spawns and released only when it finishes, so an executor that never returns holds its permit for the life of the connection. 65 requests exhaust `Semaphore::new(64)` by construction. |

## Equivalent is not the same as unkillable

The last one to fall, `sync_collector.rs:240`, had been recorded here as a
provably equivalent mutant: the append revert runs only when a store save
fails, both callers of `process_event` propagate with `?`, and the
`CollectState` the revert repairs is dropped without being read. Correct code
and code that reverted *the wrong artifact* were observationally identical.

The proof was sound and the conclusion was too narrow. The revert was
untestable because it was welded to an error path, not because its logic is
unobservable. As a free function — `revert_artifact_append(task, id, len,
meta)` — it is an ordinary transformation with an ordinary assertion, and the
mutant dies.

Two artifacts are what make that test bite, which is the same lesson as the
axum routing tests above: with one artifact `!=` finds nothing and the revert
is a no-op, indistinguishable from correctly reverting an untouched artifact.
With two it truncates the bystander and leaves the intended artifact holding
the failed append.

The revert was **not** deleted, and the contrast with the websocket size guard
is the point. That guard was redundant with a cap enforced by the same constant
one layer up, so deleting it lost nothing. This one is the only thing that
would repair in-memory state if a caller ever handled a `process_event` error
instead of propagating it. Deleting it would have traded a survivor for a
latent correctness hole. "Delete the dead branch" and "extract the buried
decision" are both available; which one applies depends on whether the code
would still be wrong if it ever ran.

## A refactor can manufacture a survivor

Worth recording, because it cost a round trip. Splitting `check` for clippy's
`too_many_lines` originally extracted the read-lock **fast path**. That handed
mutation testing an unkillable target: the fast path is a pure optimization, so
replacing the whole function with `None` still reaches the same decisions
through the slow path, and only lock contention differs. The sweep duly
reported a survivor that had not existed before the refactor.

Extracting the **slow path** instead fixes it — stub that and no bucket is ever
created, so every caller is admitted forever, which the enforcement tests catch
at once. The rule generalises: when splitting a function for length, extract
the half whose absence changes behaviour.

The 2026-08-09 per-file table (see "Per-file measurements") settles two open
questions:

* **`a2a-protocol-types` 605/8** matches the 2026-08-03 artifact recount and
  the 2026-08-07 CI figure exactly. Three independent measurements agree, so
  that crate's contribution to the survivor total is confirmed at 8.
* **`agent_card/caching.rs` is at zero**, which the 2026-08-03 artifacts
  (7 survivors) contradict. The artifacts are simply older than the three
  commits that fixed it. This is the trap the survivor list sets: a cluster
  list is only meaningful against the commit it was measured on.

All nine `a2a-server` files in that table report **exit 0** — every mutant
caught, and no exclusions anywhere. `state_machine.rs` is not in the table: it
was verified by `--list` only (12 mutants → 7), without a full sweep.

Three things this burndown is worth remembering for:

* **The same bug wore four hats.** `if len > CAP { len - CAP }` appeared in
  `messaging.rs`, `sync_collector.rs`, `background/state_machine.rs` and
  `eviction.rs`. Found by grepping for the *shape* rather than by waiting for
  a sweep to rediscover it, and fixed with `saturating_sub` — which deletes
  the operators rather than excluding their mutants. `rate_limit.rs` was the
  same defect in a different dress: one predicate written twice, where only
  the copy on the reachable path was ever tested.

* **A passing test is not a killing test.** The first `axum_adapter` routing
  tests asserted 404 for an unknown id — green, and worthless, because every
  mutant routes the request somewhere that also 404s. Seven survivors became
  five, not zero. The fix was to seed the task so a correct parse answers 200
  and each wrong parse answers 404. 404 is the most common answer that router
  gives, which made it the worst possible thing to assert against a routing
  bug.

* **A survivor can mean the code is wrong to exist.** Three `websocket.rs`
  survivors lived in an oversized-message guard that cannot execute:
  tungstenite is configured with the same constant and rejects during the
  read, which `ws_oversized_message_rejected` already proves. It was deleted,
  not tested. Conversely `push/sender.rs:730` *looked* like a latent
  underflow — `max_attempts - 1` on a public unvalidated `usize` — and is not
  one, because the enclosing loop is `for attempt in 0..max_attempts`. Checked
  rather than reported as a bug; its five survivors were a missing
  retry-boundary test, which now measures the backoff on the clock.
