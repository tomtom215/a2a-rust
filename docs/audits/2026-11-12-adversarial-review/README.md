<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Independent adversarial review of 2026-11-12: register

The review examined `main` at `124b70f4` (pull request #206) and is a 32-page PDF. This register has one row
per finding it made (17 rows: R1 to R5, P1 to P4, A1 to A4, T1 to T3, G1), one row for each larger
piece of work it asked for that is not one of those (9 rows: `M1`, the strict multi-tenant
profile, and `S1` to `S8`), and one for a defect this work found while measuring (`D1`). `findings.json` is the register; each row has `status`, a `note` for what the status does not
cover, the `commit`, the `evidence` and the `tests`.

Each row was checked against the code before it was worked. The behaviour defects (R1 to R5, P1 to P4, A1 to A3) were each
**reproduced by a test that failed on the code as it stood** before they were fixed; the evidence column says what the failure was.
T1, T3, A4, M1 and the schema check in T2 add an opt-in rather than change a behaviour, so their "before" is the unchanged default, shown in the
evidence column, not a failing test. The new rules were tried with hand-applied mutants (the counts are in each commit message), and each
survivor was either killed by a new test or removed as equivalent logic.

## Counts

Read from `findings.json` by the command below, not copied from a summary.

| Status | Rows | IDs |
|---|---|---|
| FIXED | 17 | R1, R2, A1, A2, R4, R5, P1, P2, P3, P4, A4, T1, T2, T3, M1, S7, D1 |
| PARTIAL | 5 | R3, A3, S1, S5, S6 |
| OPEN | 4 | S2, S3, S4, S8 |
| MAINTAINER | 1 | G1 |

Of the reviewer's own 17 rows: 14 fixed, 2 partial (R3, A3), 0 open, 1 for the maintainer.

```bash
python3 -c "import json,collections;r=json.load(open('docs/audits/2026-11-12-adversarial-review/findings.json'));print(len(r),collections.Counter(x['status'] for x in r))"
```

## What FIXED means here, and what it does not

* A fix that changes a default is not made: T1, T3 and the rest of the tool-loop strictness are **opt-in** (`ToolLoop::strict`, `ArgumentCheck`,
  `AuditedPolicy`), because a release must not change what a deployment relies on without being asked. Their defaults are unchanged and documented.
* FIXED is for the scope in the row's `note`. R5 is proven for REST and JSON-RPC, P2 for four route families, A1 for one store's atomic hold check.
* A row is OPEN when nothing was built: S2 skill content pinning, S3 fleet quota, S4 telemetry-failure visibility and S8 `erase_subject` against a hold across
  replicas. S1 (the durable step journal), S5 (the readiness-bar measurements) and S6 (the release evidence manifest) are PARTIAL, and each row's `note` says what is
  not built. S7 (an approval bound to a call digest) is FIXED for the built-in loop.
* G1 (branch protection) is a repository setting. It cannot be changed from a session and is the maintainer's.

## Verified, and not

Verified on the tree that became `f757caa3` (commands in `docs/handoff.md`, *Now*): the server crate's library tests with every feature the
handler uses and a live PostgreSQL 16 (3,256 passed); the integration tests of the changed areas; workspace clippy `-D warnings`, rustdoc `-D warnings`, rustfmt and the
repository's static gates; the book's doc-tests (351 passed, 4 ignored); against **real models** on llama.cpp (Qwen3-1.7B), the tool-loop
strict profile, argument validation, terminal truth through a real handler, stream and webhook, and the step journal and approval binding (S1, S7); and, on a real server over real sockets, the readiness bar's rows R1 to R4 and C1 to C5 (see S5 and D1).

Not verified: the workflows on GitHub beyond the first run (on `f62284f8`, which found four defects and a mutation-baseline failure, all fixed at `a9847722`; the run on that head had not reported when this was written); the Windows and macOS legs; the mutation job on the changed lines (the hand mutants stand in for it
and are not the same measurement); the `--full` preflight; any multi-replica or load behaviour of the new code.
