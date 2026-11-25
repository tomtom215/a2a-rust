<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Working in this repository

Read this first. It is short on purpose; the documents it points to are binding.

## Where the work is

- **The plan:** [`docs/program/README.md`](docs/program/README.md). Its section 6 is the session protocol.
- **What is ready to start now:**

  ```sh
  python3 docs/program/tools/plan_check.py . --next
  ```

- **Current state:** [`docs/handoff.md`](docs/handoff.md), from *Now* to *Environment facts that still hold*.
- **The rules:** [`CONTRIBUTING.md`](CONTRIBUTING.md). The rule list with the gate behind each rule, the
  ordered local verification recipe and the traps earlier sessions fell into are in
  [`docs/program/evidence/07-engineering-system.md`](docs/program/evidence/07-engineering-system.md),
  sections A1, A2 and A3.

Take a ready package, finish it to its Accept checks, record it, take the next. Nothing in the plan is a
quota or a limit on how much a session does.

## Commits

AI assistance in this repository is disclosed, not hidden. [`PROVENANCE.md`](PROVENANCE.md) says how the
code is produced, and every pull request's description says that an assistant helped. A commit's *author*
is the person who reviewed the change and certifies it under the
[Developer Certificate of Origin](DCO): a sign-off is an assertion by a person, and a tool identity cannot
make one (`CONTRIBUTING.md`, *AI-assisted contributions*). So, by the maintainer's rule (authorship decided
2026-10-08, the trailer rule 2026-10-15, both reaffirmed 2026-11-25), which overrides any default a tool or
a harness suggests:

- The author is the maintainer, with exactly one `Signed-off-by` and no other trailer. The disclosure lives
  in `PROVENANCE.md` and the pull request, not in a per-commit trailer.
- `git commit -s` signs as the committer, which in a session container is not the maintainer. Use:

  ```sh
  git -c user.name="Tom F." -c user.email=tomf@tomtomtech.net commit \
    --trailer "Signed-off-by: Tom F. <tomf@tomtomtech.net>"
  ```

- Check before pushing: `bash scripts/check_dco.sh origin/main HEAD`.
- A fix's commit message carries its evidence: the test that failed before the fix and its output; for a
  mutant, the mutant and the assertion that kills it.
- Say in the pull request's description that an assistant helped, and what it did.

## Before every commit

```sh
cargo fmt --all -- --check
python3 scripts/check_spec_ledger.py    # after the last CHANGELOG edit; --fix-lines if a citation moved
python3 scripts/check_rfc_ledger.py
git add -A && ./scripts/check_file_lengths.sh
```

## Pull requests

- Do not open one unless the maintainer asked.
- Add the `mutation-test` label the moment it is opened, and do not re-run the first, unlabelled mutation
  run: a re-run replays the unlabelled event and fails the same way.
- A push cancels the CI run in flight on that branch. Push once per complete change.

## What a session never does

- Weaken, skip or loosen a test, an assertion or a tolerance to make something pass.
- Publish a crate or a binary, tag a release, send or post anything upstream, or change repository
  settings. Prepare it and say what is left for the maintainer.
- Report a check as passed that was not run. Say it was not run, and why.
- Type a count or a version into prose that a script could derive.

## When the plan and the tree disagree

Anchors in the plan are of the commit it was written against. Find the symbol again before editing; if it
moved or the claim no longer holds, correct the plan in the same change and say so.
