<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Security Policy

## Supported Versions

Security fixes are released on top of the latest published minor line. Older
`0.x` lines do not receive backports — upgrade to the latest release to stay
patched. (This table is updated as part of every release; the release
workflow checks that it covers the version being tagged.)

| Version | Supported          |
| ------- | ------------------ |
| 0.9.x   | :white_check_mark: |
| < 0.9   | :x:                |

## Scope

This policy covers **all crates** in the a2a-rust workspace, including but not
limited to the core protocol types, server, and client libraries.

## Reporting a Vulnerability

If you discover a security vulnerability in this project, please report it
responsibly. **Do not open a public GitHub issue.**

### Preferred Channels

1. **GitHub Security Advisories (preferred):** Open a draft advisory at
   <https://github.com/tomtom215/a2a-rust/security/advisories/new>. The report
   stays private to you and the maintainers, and the channel is encrypted in
   transit by GitHub.
2. **Email:** Send a detailed report to **tomf@tomtomtech.net** — the address
   in this project's copyright headers.

> **`security@a2a-rust.dev` does not work.** Earlier revisions of this file
> listed it as the primary channel, but `a2a-rust.dev` is not registered
> (NXDOMAIN), so mail to it is undeliverable and a report sent there would
> have been silently lost. Use one of the two channels above. The dedicated
> address will be restored here once the domain is live.

### PGP Key

**Not available.** There is no published PGP key for this project, so reports
sent by email cannot be encrypted end-to-end. This is a real gap for anyone
who needs to disclose an unpatched vulnerability over untrusted mail.

Until a key is published, prefer **GitHub Security Advisories** (channel 1),
which keeps the report private without needing one. If you must use email and
the contents are sensitive, send a short notice without details and ask for an
encrypted channel first.

### Release Artifact Verification

Know what you can and cannot verify about a release:

| Artifact | Signed? | How to verify |
|---|---|---|
| Git tags (`v0.2.0` … `v0.7.0`) | **No** | Nothing to verify. All ten release tags to date are lightweight — unannotated and unsigned — so they carry no tagger identity, no date, and no signature. A tag alone does not establish who cut the release. |
| Release binaries / SBOMs | Yes | Attested in the release workflow; see [`PROVENANCE.md`](PROVENANCE.md). |
| Published crates | Yes (by crates.io) | Standard crates.io registry checksums. |

If you need a cryptographic link between a published version and this
repository, use the build provenance attestations described in
`PROVENANCE.md`, not the git tag. Signed, annotated tags are a known gap and
have not yet been adopted.

### What to Include

- Description of the vulnerability and its potential impact.
- Steps to reproduce or a minimal proof of concept.
- Affected crate(s) and version(s).
- Any suggested fix, if available.

## Disclosure Timeline

We follow a **90-day coordinated disclosure** timeline:

1. **Day 0** -- Report received; we acknowledge within 3 business days.
2. **Day 1-14** -- We triage the issue, confirm validity, and assess severity.
3. **Day 15-90** -- We develop and test a fix, coordinating with the reporter.
4. **Day 90** -- Public disclosure, with a CVE identifier if applicable.

If a fix requires more time, we will negotiate an extension with the reporter.
We aim to release a patch as quickly as possible, ideally well before the
90-day deadline.

## Credit

We gratefully credit reporters in release notes and security advisories (unless
anonymity is requested).
