<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Vendored IETF RFC texts

The `rfc*.txt` files here are **verbatim copies of IETF RFCs, not authored in
this repository and not to be edited, not even whitespace.** They exist so that
`scripts/check_rfc_ledger.py` can check `docs/compliance/rfc-ledger.json`
against the exact text its line numbers refer to: each file's SHA-256 is pinned
in the ledger, and the check fails when they disagree.

**Licence.** The IETF Trust Legal Provisions, section 3.c.i
(<https://trustee.ietf.org/documents/trust-legal-provisions/tlp-5/>, read
2026-11-06), grant every person a worldwide, royalty-free licence "to copy,
publish, display and distribute IETF Contributions and IETF Documents in full
and without modification". These files are unmodified and carry their own IETF
legends and copyright notice, which is why they carry no SPDX header and why
the provenance is recorded here, not inside them. They are not Apache-2.0
material of this project.

| File | RFC | Source | Retrieved | SHA-256 |
|---|---|---|---|---|
| `rfc7515.txt` | RFC 7515 | <https://www.rfc-editor.org/rfc/rfc7515.txt> | 2026-11-06 | `ecbf84674118e62526511c1237e1deaa86aada86d25b247623a2a7e04ae62e40` |
| `rfc7519.txt` | RFC 7519 | <https://www.rfc-editor.org/rfc/rfc7519.txt> | 2026-11-06 | `fecd930e9ccf2276b95c0017c6c4ff5d09352e4bc3c7629946447894e0f97248` |
| `rfc8725.txt` | RFC 8725 | <https://www.rfc-editor.org/rfc/rfc8725.txt> | 2026-11-06 | `737b7013a2495dcfd5481cf4c57c87e2455d537c4cec1140c9096a71fcef0812` |
