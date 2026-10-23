<!-- SPDX-License-Identifier: Apache-2.0 -->
# Hash-locked Python requirements for CI

Each `*.txt` here is generated from the `*.in` beside it and installed with
`pip install --require-hashes -r`, so a workflow that installs from one gets
exactly the distributions recorded in it (OpenSSF Scorecard,
Pinned-Dependencies).

Not every Python install in CI goes through these files. In `tck.yml` the
`python` cross-language leg installs `itk/agents/python/requirements.txt`
without hashes and the `python-sdk` leg installs `a2a-sdk[http-server]`
unpinned; `pin-freshness.yml` installs the newest SDK on purpose; and
`official-tck.yml` installs the cloned upstream TCK with `uv pip install -e .`.

Run the commands below from this directory (`.github/requirements/`), with
`NAME` one of the eight locks: `a2a-sdk`, `a2a-sdk-http`, `a2a-sdk-signing`,
`cryptography`, `httpx`, `itk-selftest`, `pyyaml`, `uv`. `cryptography` (50.0.2,
locked 2026-10-21) is `scripts/crypto_interop/check.py`'s, in `ci.yml`.
On 2026-10-22 `certifi` (2026.10.19, GHSA-6q48-6c7w-9gw2) and `uv` (0.13.5,
GHSA-hq85-5cpj-mr62) were moved alone, with `--upgrade-package`, for the
emSign root both bundled; every other pin is as it was.

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
full regeneration took 7.36.2, which `a2a-sdk.txt`, `a2a-sdk-http.txt` and
`itk-selftest.txt` (the other locks that carry protobuf) already had; the
interop matched all six again on it, under Python 3.12. Nothing moves them:
Dependabot does not read these files, `pin-freshness.yml` installs
`pyyaml.txt` but resolves the newest SDK on its own and never rewrites a
lock, and `osv.yml` does not scan them. Regenerating and auditing them is a
deliberate step.
