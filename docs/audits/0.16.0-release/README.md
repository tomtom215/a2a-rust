<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Release register for 0.16.0

Written on 2026-11-18, while 0.16.0 was prepared, to answer one question for every row that was still open in the audit
registers: **does it block this release?** `findings.json` has one row per open item, with `id`, `source`, `status`, `finding`,
`blocker`, `basis` and `evidence`.

No row blocks the release. One would have: external review row `X-10` (a store outage longer than `store_write_retry` loses the
event being written) is a data-loss path that the release notes did not disclose. The pull request that adds this register
discloses it in the changelog's Known Limitations, with the workaround; it does not fix it.

## The rule

A row blocks the release only if it is a correctness, security or data-loss defect that a user of 0.16.0 would hit **and** the
release notes do not already disclose it. A missing feature, a missing measurement, a document, a deployment example, a
performance cost and a maintainer's decision do not block. A defect the notes disclose, with its risk and a workaround, does not
block either.

## Counts

Read them from the file, not from this page:

```bash
python3 -c "import json,collections;r=json.load(open('docs/audits/0.16.0-release/findings.json'));print(len(r),collections.Counter(x['blocker'] for x in r),collections.Counter(x['basis'] for x in r),collections.Counter(x['source'].split('/')[2] if x['source'].startswith('docs/') else 'CHANGELOG.md' for x in r))"
```

On 2026-11-18 it printed 130 rows, all `blocker: no`; 92 with basis `read` and 38 with basis `summary`; 81 from the 2026-11-01
triage, 13 from the 2026-11-11 external review, 9 from the 2026-11-12 review, 4 from the 2026-11-13 review, the 22 Known
Limitations of the release, and one row found while the register was written (`CI2`, listed under `0.16.0-release`).

## What was included

- **`docs/audits/2026-11-13-adversarial-review/findings.json`**, **`2026-11-12-adversarial-review/findings.json`** and
  **`2026-11-11-external-review/findings.json`**: every row whose status is not a closed one. Closed means `FIXED`, a status that
  starts `CONFIRMED, FIXED`, `FOUND BY THE NEW GATE, FIXED` or `NOT AFFECTED`, `VERIFIED`, or `NOT SUPPORTED` (the review's claim
  was not borne out).
- **`docs/audits/2026-11-01-triage/*.json`**: these 205 rows (149 triaged and 56 new findings) have no status field. Their
  status since the triage is the table in section O of `docs/audits/2026-10-29/README.md`, which was joined to them by id: the
  ids a table row is about are the ones before its first colon, and the last row that names an id wins. 119 are `fixed`, 3
  `not a defect` and 2 `refuted (live)`; the other 81 are here, 76 as `fixed (part)` and 5 as `open`.
- **`CI2`**, a test that failed once on `main` while this was written (`ci.yml` run 44190122806). It is a risk to the tag, not to a
  user, and its row says what the job's log shows.
- **The Known Limitations of the 0.16.0 section of `CHANGELOG.md`**, as `KL-01` to `KL-22`, in the order they appear. Entries 20
  to 22 were added with this register.

## How far the answers can be trusted

`basis` says how each answer was reached, and neither value means the code was tested:

- `read`: the row's finding and its note were read in full on 2026-11-18. Where the evidence cites a file and line, that line
  was opened. The Known Limitations were audited entry by entry against the code and the documents they name by a read-only
  review; every claim it reported as contradicted was then opened by hand (seven entries were corrected, and an eighth lost a claim
  nothing supports), and the claims it reported as verified were not opened again.
- `summary`: only the first line of the row's summary, its recorded severity and its theme were read. The answer is by class:
  the `evidence` names the class (documentation, deployment example, security hardening, performance, configuration) and why
  that class does not block. All 38 are rows the triage recorded as `low` or `info`.

Three limits of the method:

1. Section O gives one status to a table row that names several ids, so an id can be listed here as `fixed (part)` when its own
   part is done. `G-18` is one: its evidence says the code closes it. The list errs towards including a row.
2. Nothing was re-run. A row that says "measured" or "reproduced" is the register's statement, dated there.
3. Whether a row is a defect "a user would hit" is a judgement. The rows closest to the line are `X-10`, `E-F8`, `X-19`, `A3`,
   `SEC-13` and `E-F26`; their evidence says why each does not block and what would change that.

## Where the work goes

Every row that needs work is in `ROADMAP.md`, section *After 0.16.0*, in the group its evidence names.
