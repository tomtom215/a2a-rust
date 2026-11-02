<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Slice h-designs: audit rows H5-H14, H16-H19 on `main` at `4a8243c`

I read the code; nothing was built or run. The full evidence (with `file:line`), the options and the ship-now pieces for each row are in `h-designs.json`.

## Counts

| Verdict | Rows |
|---|---|
| needs_decision | 14 (H5, H6, H7, H8, H9, H10, H11, H12, H13, H14, H16, H17, H18, H19) |
| confirmed / already_fixed / partly_fixed / not_a_defect / cannot_check / duplicate | 0 |

Every row is still open on main. Each one is a design or policy choice, but each also has an additive XS/S piece that could ship without that choice. Seven new findings are listed in the JSON (`NEW-h-designs-1..7`).

Severity changes from the report:
- **H7:** lowered S1 to S2.
- **H8:** lowered S1 to medium.
- **H11:** lowered S1 to S2.

The rest are unchanged.

## ADRs 0030-0034: built or only designed

| ADR | Status in code |
|---|---|
| 0030 resume + journal | **Built.** Resume-then-cancel and `reattach` came in `660196e`; the journal trait, the in-memory journal and `PostgresDelegationJournal` came in `49f5509`. Tests exist; I read them and did not run them. Not built, as the ADR itself says: a SQLite journal, owner enforcement, and restoring the lease or budget on reattach. |
| 0031 discovery beyond one link | **Not built.** The discovery crate has only the mDNS from ADR 0029. |
| 0032 HTTP pool | **Not built.** Adjacent progress only: `default_tls_config` is now built once per process (`c4404e8`), and HTTP has `with_tls_config`. |
| 0033 group delegation | **Not built.** |
| 0034 task lineage | **Not built.** `HaltScope` is still `All` or `Tenant`. |

## The five rows that matter most

1. **H5: control-plane cost per child.**
   - A renewal is a full `GetTask` every ttl/3.
   - Each running leased turn has its own watcher, reading the lease store once per second.
   - Idle subscribers poll every 250 ms (twice per poll on the SQL stores), with no cap on subscribers.
   - Under the production profile, every renewal also signs every artifact (NEW-3).
   - Ship now: `history_length = Some(0)`, a staggered first renewal, and subscriber caps.
   - Lease groups and a sweeper need a new ADR that amends ADR 0020.
2. **H6: retries amplify overload.**
   - There is no retry budget, no breaker and no cap on HTTP calls in flight.
   - Retries are off by default. When a client turns them on, it follows the server's constant 1 s `Retry-After` with no jitter, so refused callers come back in lockstep (NEW-1).
   - Delegation resume adds a second retry layer with no jitter (NEW-2).
   - Ship now: jitter on server-directed delays, `with_max_elapsed`, and a per-client cap on calls in flight. The shared form is ADR 0032 §5.
3. **H10: no lineage and no job-scoped halt.**
   - ADR 0034 covers it.
   - Before any new `HaltScope` exists, the halt store must accept a scope key it does not know. Today one unknown row fails the whole load: older replicas freeze their halt view, and a restarted older replica refuses every send (NEW-4).
4. **H7: readiness.**
   - `/ready` checks draining and the task store only.
   - A worse case than the report names: until the halt store has been read once, every send is refused as a non-retryable `Halted` while `/ready` answers 200 (NEW-7).
   - Recommendation: a load signal rather than readiness flapping, and keep a halted replica ready.
5. **H13: memory and payload.**
   - All four sub-claims hold.
   - One part needs no decision: the SQL stores still do not override `save_appending_history`, while the trait's doc says they do (NEW-6).
   - Byte budgets, blob offload and compaction of a live task's event log need a new ADR.

## ADR coverage for open rows

- **Existing ADRs:**
  - H6, H8 and H12 → 0032.
  - H8 → 0031 as well.
  - H10 → 0034.
  - H18 → 0033, partly.
- **Amendments to existing ADRs:**
  - H17 → 0022.
  - H11 → 0013, or a new ADR.
  - H5 → 0020, through a new ADR.
- **New ADRs (next free number is 0035):** H7, H9, H13, H14 and H19.
- **No ADR needed:** H16, whose fix can be decided in the pull request.

## Numbers

**Re-derived from code:**
- H5: 100k renewals/s, and 1M lease reads/s at 1M running children. The report's lower bound of 250k reads/s does not follow from the code.
- Idle subscribers: 40k reads/s per 10k subscribers, or 80k/s on the SQL stores.
- H6: four attempts per refused send, but only when the client has opted in to retries.
- The 1024 stream cap and the 256 / 16 MiB / 1024 limits.
- H16: 10,000 buckets and 100 requests per minute. The lockout lasts as long as the existing callers stay active, not one window.

**Could not re-derive:**
- Finding 9's p95 of about 1 s (a measurement).
- About 64 B per rate-limit bucket.
- The model being the bottleneck by three orders of magnitude.
- Socket counts.

**Wrong or stale in the report:**
- H11: budget-exhausted endings were already counted at `8dc4331`, and overload and halt refusals are counted by class.
- H8: the root store is no longer rebuilt on every fetch (`c4404e8`), but no test pins this.
- H17: a model 429 now ends the task `Transient` (`84e0dbe`).

## Not done

- No `cargo` and no live probes. The JSON gives the exact probe for H5, H6, H8, H12, H14, H17 and H19.
- I did not check how a2a-python or a2a-go handle W3C trace context or the a2a-rust extensions.
- Rows H1-H4 and H15 belong to other slices.
