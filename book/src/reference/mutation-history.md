<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Mutation Testing History

A dated record of full-workspace mutation sweep results, so the
surviving-mutant count has a durable history rather than only living in
GitHub Actions artifacts (retained 90 days — see
[CI/CD](../deployment/cicd.md#mutation-testing-workflow)) or transient CI
logs.

## Why this page exists

`mutants.yml` runs a full sweep weekly and fails the workflow on any
surviving mutant, but a failing scheduled workflow is a notification, not a
record: it says *this week* had a survivor, not *when the score last
changed* or *what the trend looks like*. Nothing before this page captured
that — a real gap this project should not let recur, in the same spirit as
`docs/official-tck-findings.md`'s conformance history.

## Pages in this chapter

| Page | What it holds |
|---|---|
| [Full-sweep ledger](./mutation-history/sweeps.md) | Every full-workspace sweep since 2026-08-07: score, caught, missed, timeouts, and what each run found. Read its caveat first: every score before 2026-09-23 counts some uncompilable mutants as caught. |
| [How the ledger came to be empty](./mutation-history/gate-defects.md) | The eight gate defects that let the mutation gate report success over work it had not done, and how each was found and fixed |
| [Per-file measurements](./mutation-history/per-file.md) | Per-file sweeps used to burn down survivor clusters, the triage of the survivors, and the lessons about equivalence and refactoring |

This page keeps what you need to use the record: how to add an entry, and the
mutants argued to be equivalent.

## How to add an entry

After a full sweep completes (`mutants-summary` job in `mutants.yml`), copy
the aggregated table from that run's `GITHUB_STEP_SUMMARY` output into a new
row of the [ledger](./mutation-history/sweeps.md#the-ledger), dated to the run, with the commit it ran against. A clean sweep
(zero missed) is still worth recording — "still zero" is signal too, not a
no-op.

Copy the **Timeout** column too, and read it as unfinished business rather than
a footnote. `mutants.yml` scores `caught / (caught + missed)`: a timeout is in
neither term, so it is not a mutant the score counts against — it is a mutant
the score does not describe. "Zero missed" and "zero timeouts" are different
statements. The ledger's 2026-08-14 row is what it costs to read one as the other.

## Known equivalent mutants

Mutants that no test can kill, because the mutation does not change observable
behaviour. Each is listed with the argument for its equivalence, so the claim
can be checked rather than taken on trust. Per
[ADR 0006](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0006-mutation-testing.md#equivalent-mutants) the
burden is "no test can distinguish it", not "no test occurred to me".

> **Superseded 2026-08-09 — this list is now empty, and nothing is excluded.**
> The two `evict` mutants below were real equivalents, and they were briefly
> excluded by an `--exclude-re` pattern. Both the exclusion and the mutants are
> gone: the equivalence turned out to be a property of the *operator*, not of
> the logic, and rewriting the guard removed it at the source. See
> "How the last equivalents were retired" below. The analysis is kept because
> the reasoning is what makes the retirement checkable.

Neither was marked with `#[mutants::skip]`: that attribute resolves through the
`mutants` crate, which this workspace did not then depend on, and adding a
regular dependency to a published crate is a decision to take deliberately
rather than in passing. It was taken on 2026-09-10 for the three equivalents
named in the 2026-09-07 row (see ADR 0006).

### How the last equivalents were retired

`evict` computed `store.len() - max` under an `if store.len() > max` guard, at
two sites. Weakening `>` to `>=` differed only at `len == max`, where the mutant
entered with `overflow` of 0, collected, sorted, and then `take(0)` removed
nothing — unkillable by construction.

The earlier note here argued this guard could not simply be deleted the way
`messaging.rs`'s was, because it skips an O(n log n) collect-and-sort and
deleting it would pay that sort on every write sitting exactly at capacity.
That argument was correct, and the fix respects it: the guard is **kept**, and
only its spelling changes.

```rust
# let store: Vec<u8> = Vec::new();
# let max = 10;
let overflow = store.len().saturating_sub(max);
if overflow != 0 { /* collect, sort, evict */ }
```

`saturating_sub` is zero exactly when `len <= max`, so this is the same
predicate and the same short-circuit — the sort is still skipped at or below
capacity. But `!=` mutates only to `==`, which inverts the guard and is caught,
where `>` mutated to an equivalent `>=`.

Measured rather than asserted:

| Check | Before | After |
|---|---:|---:|
| Mutants in `eviction.rs` (`--list`) | 25 | 17 |
| Matches for the old exclusion pattern, crate-wide (2097 mutants) | 2 | 0 |
| Full sweep of `eviction.rs` | — | 17 tested, **17 caught, exit 0** |

The generalisable part: an equivalent mutant is usually a branch guarding a
no-op, or an operator whose weakened form reaches the same state. Both are
often fixable in the source, and deleting the mutant beats teaching the gate to
ignore it.

While it existed it was passed on the command line, not as a `mutants.toml`
`exclude_re` entry. That detail outlives the exclusion and still governs any
future one, because **cargo-mutants 27.1.0 silently ignores that config key** — verified
with `--list` on 2026-08-09, after a precise pattern added there still produced
the mutant it named. The same version ignores `test_tool` and `profile`
identically. The pre-existing `^tracing::` and `^log::` entries in that file
are therefore inert, which nothing had noticed because neither ever matched a
real mutant. If you add an exclusion, confirm it took effect with
`cargo mutants --file <path> --list | wc -l` before and after; an exclusion you
believe in but that does nothing is the same class of defect as a gate that
cannot fail.

| Location | Mutation | Why no test can kill it |
|---|---|---|
| `store/task_store/in_memory/eviction.rs:100` in `evict` | `store.len() > max` → `>=` | They differ only at `len == max`, where the mutant enters the capacity branch with `overflow == 0`. It collects and sorts the terminal tasks, then `take(0)` removes none, and the fallback's own `len > max` is false. It burns a sort and changes nothing observable. |
| `store/task_store/in_memory/eviction.rs:123` in `evict` | `store.len() > max` → `>=` | The same at the fallback guard: `len == max` gives `remaining == 0` and `take(0)`. |

Both are the same shape: **a branch whose only purpose is to skip work, whose
two arms coincide at the boundary the comparison tests.** That pattern is worth
recognising, because it will recur wherever a hot path avoids work, and because
it is genuinely different from an untested branch.

### Two of these were retired by deleting the branch, not by testing harder

This table held four rows until 2026-08-08. The two that left were both in
`handler/messaging.rs`, and neither was killed — the code that generated them
was rewritten so the mutation no longer exists:

| Was | Now |
|---|---|
| `shape_response_history` — `msgs.len() > n` → `>=` | `helpers::truncate_history`, branchless via `saturating_sub` |
| `send_message_inner` — `history.len() > MAX` → `>=` | the guard deleted; `drain(..excess)` runs unconditionally |

The reasoning is the same in both cases and generalises. A guard of the form
`if len > n { trim_by(len - n) }` is unkillable at `len == n` *because the body
does nothing there*. Write the amount first — `let excess =
len.saturating_sub(n)` — and the branch has nothing left to decide. `drain(..0)`
is genuinely free: `Drain::drop` skips its memmove when the tail does not move,
so this is O(1) rather than an O(n) shift.

**This does not generalise to every guard, and the two survivors above are
where it stops.** `evict`'s guard skips an O(n log n) collect-and-sort, not a
no-op. Deleting it to retire a mutation-testing artefact would put a sort on
every write that is exactly at capacity — paying real cycles to improve a
score, which is the wrong trade and the reason those two rows remain.

The rule worth carrying forward: **when the guarded work is a no-op at the
boundary, delete the guard; when it is expensive, keep it and record the
equivalence.** The mutant is a symptom either way — sometimes of redundant
code, sometimes of a deliberate optimisation.

It is not a reason to relax the target, and the equivalence rate is nowhere
near uniform. `messaging.rs` went from 17 survivors to **0**, `eviction.rs`
from 13 to 2, `dispatch/grpc/native.rs` from 11 to **0**, and
`handler/lifecycle/list_tasks.rs` from 10 to **0**. `native.rs` produced no
equivalents at all because its survivors were not boundary conditions: ten of
its eleven were whole-method replacements, and a method survives being replaced
by `Ok(Response::new(Default::default()))` only when nothing calls it.

Of the 51 survivors across those four files, 49 were ordinary test gaps or
removable branches. Two remain, both in `eviction.rs`, both for the deliberate
reason given below.

### Whole-method survivors name an untested layer, not a missing edge case

`dispatch/grpc/native.rs` is worth separating from the other two files. Its
survivors were not `>` versus `>=`; they were ten of the eleven methods of the
`A2aService` trait impl, each replaceable in its entirety by an empty `Ok`.

`grpc_dispatch_tests.rs` did exist and did pass. It covers `GrpcConfig`'s
builder, `into_service`, and binding a listener — it never issues an RPC. The
gRPC binding had test files, test names, and no test that called it, which is
exactly the state a line-coverage number is worst at revealing and a mutation
score is best at.

Two things made the difference when killing them:

* **Assert on content, not on `is_ok()`.** The mutation returns a *successful*
  empty response, so `assert!(result.is_ok())` passes against a method whose
  body has been deleted. Every test asserts a field that a default cannot
  carry — a task id, the configured card's name, the registered URL.
* **Some methods can only be caught by their side effect.** Both mutations of
  `delete_task_push_notification_config` return exactly the `Ok(Response::new(()))`
  the real method returns on success; the response cannot distinguish them at
  all. Only deleting a config and then listing it can.

### Redundant code can be what makes a mutant killable

Worth recording because it is counter-intuitive and it cost a wrong claim.

`eviction.rs` originally guarded its fallback with
`if removed < overflow && store.len() > max`, where `removed` counted the
terminal tasks actually evicted. Four of the file's thirteen survivors were on
that counter, and they survive for a provable reason: every evicted id comes
from `store.entries`, so every removal lands, and `len` is `max + overflow` on
entry — which makes `removed < overflow` and `store.len() > max` the *same
predicate*. The counter was dead weight, so it was deleted.

Deleting it was behaviour-preserving, and it did remove those four mutants. It
also turned a *caught* mutant into a missed one:
`overflow = store.len() - max` → `len / max` had been killed by a test that
only asserted the store's final size. The `&&` short-circuit was what made that
work — with a too-small overflow, `removed == overflow` switched the fallback
off and the store was left above the cap. Without the counter the fallback
re-checks the size and quietly mops up the difference, so the store lands at
the cap either way and a size assertion cannot see the bug.

The mutant was still killable, just not by that test: a too-small overflow
evicts one fewer terminal task and then lets the fallback take the oldest entry
*overall*, which can be an in-flight task that should have been spared. The
fix was a test that pins which tasks survive rather than how many
(`evict_prefers_terminal_tasks_over_an_older_in_flight_one`), and it covers a
policy — terminal-first eviction — that nothing had tested.

Two things generalise. **Redundant logic can carry mutation-detection strength
that the non-redundant version does not**, so simplifying can lower the score
without changing behaviour. And **an assertion on a quantity is weaker than an
assertion on identity** whenever a later stage can compensate for an earlier
stage's arithmetic. Neither is an argument against the simplification — the
file ended simpler *and* better tested — but both are arguments for re-running
the sweep after a refactor rather than assuming the score can only improve.

### Two retired by matching the replacement exactly, 2026-10-12

Both survived the sweeps of 2026-09-28, 2026-10-05 and 2026-10-12, and both
were equivalent outright, not untested:

- `TaskStore::earliest_event_seq`'s default returned `Ok(None)`, and the
  mutant replaces the body with `Box::pin(async move{Ok(None)})` — the same
  value, with no side effect either way. The patched cargo-mutants already
  skips a replacement equal to a boxed future's whole body, which is why
  `TaskStore::count`'s `Box::pin(async { Ok(0) })` never produced an `Ok(0)`
  mutant; this body escaped that check only because it began with
  `let _ = task_id;`. The discard is now an `#[allow(unused_variables)]` on the
  method, the body is the bare expression, and the mutant is not generated.
- The conformance harness's `CancelOnFirstWrite::close` forwarded to
  `InMemoryQueueWriter::close`, whose body is itself
  `Box::pin(async move { Ok(()) })` — the channel closes when the last writer
  drops, not on `close`. Forwarding and doing nothing were the same function.
  The wrapper now does nothing, in the form the tool recognises, and
  `close_is_no_op_and_succeeds` was strengthened from "returns `Ok`" to "the
  queue still delivers a write made after `close`", so the equivalence the
  rewrite rests on fails a test the day it stops holding.

Measured with the pinned, patched cargo-mutants 27.1.0 over the two
functions: 4 mutants listed before the change, 2 after (`Ok(Some(0))` and
`Ok(Some(1))`, both caught in the 2026-10-12 sweep).
