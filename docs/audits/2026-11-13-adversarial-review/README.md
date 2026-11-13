<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Independent adversarial review of 2026-11-13: register

The review examined `main` at `70dc337` and is a 27-page PDF. It added sixteen findings, `N01` to `N16`, to the register of
the 2026-11-12 review (`../2026-11-12-adversarial-review/`). That register's own rows are updated there, not here.

`findings.json` is this review's register. It has one row per finding, one row for the red main CI the review reported (`CI1`),
and one row for each defect found while doing the work (`D1`, `D2`). Each row carries `status`, a `note` for what the status does
not cover, the `commit`, the `evidence` and the `tests`.

Every behaviour defect was **reproduced by a test that failed on the code as it stood** before it was fixed. The evidence column
says what the failure was. `N12` and `N16` add an opt-in and change no default. `N13`, `N14` and `N15` are release and evidence
machinery.

## Counts

Read from `findings.json` with the command below, not copied from a summary.

| Status | Rows | IDs |
|---|---|---|
| FIXED | 17 | N01 to N13, N15, N16, D1, D2 |
| PARTIAL | 1 | N14 |
| OPEN | 1 | CI1 |

```bash
python3 -c "import json,collections;r=json.load(open('docs/audits/2026-11-13-adversarial-review/findings.json'));print(len(r),collections.Counter(x['status'] for x in r))"
```

## Real-model end to end

The fixes that change what a run does were run against a real open-source model: Qwen3-1.7B (Q4_K_M; GGUF SHA-256 matches the
Hugging Face LFS object id) on llama.cpp `llama-server` at `9daa0aa`. The tests are in `tests/tool_loop_review_live.rs`
and `tests/tool_loop_audit_live.rs`. Each is ignored by default, because it needs the server; each file's header has the command.

* **N04 and N07:** a finished run is sealed and replayed from its journal without the model or the tool.
* **N01, N02 and N07:** an approved call goes through the real handler, the approval gate and audit privacy, and its run is
  erased for its owner.
* **N09:** a large tool result is cut before the model sees it.

Writing those tests found `D1`, a hang on single-worker runtimes that predates this review. With all three tests in one file
and one process, the file hung in 5 of 11 runs before the fix and passed 8 of 8 after. The file was then split in two for the
500-line rule. The hang was seen only with the earlier tests in the same process (0 of 6 runs of the approval test alone hung),
so the unit test in `handler/messaging/inline_executor_tests.rs` is its regression guard. The three older live files (`tool_loop_journal_live`, `tool_loop_strict_live`,
`terminal_truth_live`) passed 9 of 9 tests on the same server.

## Not verified here

* The workflows on GitHub, for N13 and N15: their steps are proven by `scripts/prove_workflow_gates_fail.py` and by hand on
  synthetic inputs, not by a tag or a scheduled mutation run.
* The macOS leg (`CI1`).
* PostgreSQL in the N14 data rehearsal.
* Any two-replica run of the new fences outside the tests that pause one log at the race.
