<!-- SPDX-License-Identifier: Apache-2.0 -->
# Hash-locked Python requirements for CI

Each `*.txt` here is generated from the `*.in` beside it and installed with
`pip install --require-hashes -r`, so a workflow installs exactly the
distributions recorded here (OpenSSF Scorecard, Pinned-Dependencies).

Regenerate one after changing its `.in`, or to take newer releases:

```sh
uv pip compile --generate-hashes --python-version 3.12 --universal \
  --no-header --output-file NAME.txt NAME.in
```

Then check it installs with hashes and has no advisories:

```sh
python3 -m venv /tmp/v && /tmp/v/bin/pip install --require-hashes -r NAME.txt
osv-scanner scan --lockfile=requirements.txt:NAME.txt
```

The a2a-sdk pins (`a2a-sdk.in`, `a2a-sdk-http.in`) were the latest release
(1.2.2) when they were locked on 2026-10-12. Nothing moves them: neither
Dependabot nor `pin-freshness.yml` (which grades the newest SDK on its own)
reads these files, so regenerating them is a deliberate step.
