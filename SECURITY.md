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
| 0.14.x  | :white_check_mark: |
| < 0.14  | :x:                |

### How long a line is supported

Exactly as long as it is the latest minor line. The day a new minor release
(`0.15.0`) is published, the previous line (`0.14.x`) stops receiving fixes,
security fixes included; there is no overlap window and no long-term-support
line. A fix for a vulnerability ships as a patch release of the current line
— `0.14.1` for GHSA-hr9h-6jvf-wvg6 is the precedent. Pin and plan
accordingly: if you need a longer support period than that, you need to
carry it yourself (see the next section).

This is the policy as it stands at `0.x`; `STABILITY.md` says what changes at
`1.0`.

## For manufacturers integrating these crates (EU Cyber Resilience Act)

This section is for anyone who places on the EU market a product with
digital elements that contains these crates. It is a description of what this
project provides, not legal advice.

**The project's own position, as the maintainer reads it.** These crates are
free and open-source software published by an individual and not monetised:
no paid support, no charge for the crates. Recital 18 of Regulation (EU)
2024/2847 treats such software as not made available in the course of a
commercial activity, and an open-source software steward (Article 24) is a
legal person, which an individual maintainer is not. If that changes — paid
support, or hosting by a foundation — this section changes with it.

**Your obligations reach these crates.** Article 13(5) requires you to
"exercise due diligence when integrating components sourced from third
parties", free and open-source software included; Annex I, Part II(1)
requires an SBOM covering at least your top-level dependencies; and Article
13(6) requires you, on identifying a vulnerability in a component, to report
it to whoever maintains that component. Article 14's reporting obligations
apply from 11 September 2026 (Article 71(2)). What this project gives you for
each of those:

| You need | This project provides |
|---|---|
| Due diligence on the component (Art. 13(5)) | Per-crate CycloneDX SBOMs and SLSA build-provenance attestations on every GitHub release (`PROVENANCE.md`); `SHA256SUMS`; the conformance and audit record under `docs/`; this policy; the OpenSSF Best Practices evidence in `docs/openssf-best-practices.md` |
| Your SBOM (Annex I, Part II(1)) | The release SBOMs list each crate's own dependency tree, to merge into yours |
| Vulnerability monitoring | `cargo-deny` and OSV-Scanner run on every pull request and daily (`osv.yml`); GitHub security advisories for this repository |
| A place to report a vulnerability you found in these crates (Art. 13(6)) | The two channels under **Reporting a Vulnerability** below. Say in the report that it is an Article 13(6) report; it gets the same 3-business-day acknowledgement as any other. If you have a fix, send it — Article 13(6) asks you to share it, and we will credit it |
| A support period longer than the current minor line | Not available. Pin a version and backport fixes in your own fork, or upgrade with each minor release |

**Your legal deadlines win over our embargo.** The coordinated-disclosure
timeline below is a default for reporters with no other obligation. If the
law requires you to notify a CSIRT or ENISA about an actively exploited
vulnerability in a product that contains these crates, do so on the law's
timetable, and tell us at the same time; nothing in this policy asks you to
delay a notification you are required to make.

## Scope

This policy covers **every crate published from this repository**:

* **The four workspace crates** — `a2a-protocol-types`, `a2a-protocol-client`,
  `a2a-protocol-server` and `a2a-protocol-sdk`. These are the `members` of the
  root `Cargo.toml` that are published; the example, benchmark, book-test and
  TCK members are `publish = false` and are not covered.
* **`bindings/a2a-protocol-slimrpc`**, which is publishable and **is covered by
  this policy**, even though it is deliberately *outside* the root workspace —
  it is not in `Cargo.toml`'s `members` list and carries its own `Cargo.lock`
  and its own `deny.toml`, because `agntcy-slim-rpc` brings 359 transitive
  dependencies into a tree the SDK crates must not inherit. "All crates in the
  workspace" therefore did not reach it, which is why this section now names
  it. Two things a reporter should know about it: it has **never been
  published to crates.io** (see `RELEASING.md`), so no released artifact is
  affected today; and it carries a **live advisory waiver** —
  `RUSTSEC-2026-0285`, ignored in `bindings/a2a-protocol-slimrpc/deny.toml`
  because `slim-auth 0.15.4` pins `aws-lc-rs =1.16.2` while `rustls 0.23.45`
  needs `^1.18`, with no upstream release resolving it as of 2026-10-08
  (the newer slim-auth 0.16 line pins `aws-lc-rs =1.16.3` the same way). A
  report about that advisory is not new information; a report about anything
  else in the binding is in scope and welcome.

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
| Git tags `v0.2.0` … `v0.7.0` | **No** | Nothing to verify. These ten are lightweight — unannotated and unsigned — so they carry no tagger identity, no date, and no signature. |
| Git tags after `v0.14.1` | **Signed** | `release.yml` refuses a release whose tag is not signed by a key in `.github/release-signers/` on `main`. Check one yourself with `scripts/verify_tag_signature.sh <tag>`. |
| Git tags `v0.8.0` … `v0.14.1` | **Not signed, but annotated** | `git cat-file -t v0.9.0` prints `tag`, and `git for-each-ref` shows a tagger and a date. That establishes *who cut the release and when*; it does not establish authenticity, because nothing is GPG/SSH-signed, so `git tag -v` still cannot verify any release. `release.yml` refuses a lightweight tag, so this holds for every future release. |
| Release binaries / SBOMs | Yes | Attested in the release workflow; see [`PROVENANCE.md`](PROVENANCE.md). |
| Published crates | **No** — integrity only | crates.io records a SHA-256 checksum per `.crate`, which cargo verifies on download. A checksum proves the bytes are the ones crates.io holds, not who published them; this file said "signed" until 2026-09-24. The release's `SHA256SUMS` asset and build attestation are what tie a `.crate` to this repository. |

If you need a cryptographic link between a published version and this
repository, use the build provenance attestations described in
`PROVENANCE.md`. Annotation was adopted at `v0.8.0` and is enforced. Signing
is enforced from the first release after `v0.14.1`: the trusted keys are the
files in `.github/release-signers/`, which is also how an adopter obtains
them.

### What to Include

- Description of the vulnerability and its potential impact.
- Steps to reproduce or a minimal proof of concept.
- Affected crate(s) and version(s).
- Any suggested fix, if available.

## Advisories in the dependencies you compile

Your lockfile, not this repository's, decides which version of a dependency
you build. When a dependency of these crates publishes a security fix, cargo
moves you to it only if you update that dependency, or if a new release of
these crates raises its minimum to the fixed version. Updating an a2a crate
alone does neither when the old version still satisfies the requirement —
an adopter found exactly that with rustls and RUSTSEC-2026-0285 on 0.12.1.

From 0.14.0 on, no requirement in a published manifest admits a
version with a RustSec advisory: `scripts/check_advisory_floors.py`, a CI gate, tests every
published version each normal and build dependency admits against the RustSec
database. So upgrading these crates moves you off every advisory, known
when the release was cut, in the crates they depend on directly. It says
nothing about those crates' own dependencies, which their requirements
govern; for those, and for advisories published since, update the
dependency yourself (`cargo update -p rustls`) and run `cargo audit` or
`cargo deny check advisories` against your own lockfile.

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
