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

The a2a-sdk pins (`a2a-sdk.in`, `a2a-sdk-http.in`, `a2a-sdk-signing.in`) were
the latest release (1.2.2) when they were locked on 2026-10-12. The signing
pin moved from 1.1.4 that day, after `itk/interop/card_signature_interop.py`
matched all six of its cards' expectations against 1.2.2, the two expected
divergences included. Its lock kept protobuf 6.33.6 until the same day's
full regeneration took 7.36.2, which the other three locks already had; the
interop matched all six again on it, under Python 3.12. Nothing moves them:
neither Dependabot nor `pin-freshness.yml` (which grades the newest SDK on
its own) reads these files, so regenerating them is a deliberate step.
