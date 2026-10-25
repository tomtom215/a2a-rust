# Cyber Resilience Act

If you supply, in the course of a commercial activity, a product that contains
a2a-rust, the Cyber Resilience Act (Regulation (EU) 2024/2847) may make you its
manufacturer, and the crates are a third-party component you have to take care
over (Article 13(5)). This page says what the project publishes that helps you
do that, which of the Act's security properties the SDK carries, and where it
does not.

> **Not legal advice, and not a claim of compliance.** The page follows the
> [control map](https://github.com/tomtom215/a2a-rust/blob/main/docs/compliance/control-map.md)'s
> 25 Cyber Resilience Act rows (article text read in the Official Journal
> edition on 2026-10-21): 4 *supported*, 3 *documented*, 13 *partial*, and 5
> that are *yours by nature*. Each row cites what proves the SDK's part and
> says what is left.

When the Act applies (Article 71(2), as the map records it): Article 14, the
reporting obligations, from 11 September 2026, also for products placed on the
market before 11 December 2027 (Article 69(3)); Chapter IV (Articles 35 to 51)
from 11 June 2026; the rest from 11 December 2027.

## The project's position

`SECURITY.md` states it. The crates are free software published by an
individual and not monetised, which Recital 18 treats as not made available in
the course of a commercial activity, and an open-source software steward
(Article 24) is a legal person, which an individual maintainer is not. If that
changes (paid support, or hosting by a foundation), the position changes with
it. The project publishes what a manufacturer would want from a supplier
anyway.

## What the project publishes

| Evidence | What it is |
|---|---|
| **SBOMs** | A CycloneDX 1.5 SBOM per published crate, listing every dependency (optional ones included) at the version the repository's lockfile resolved, with the dependency graph. Attached to every GitHub release since `v0.7.0` and attested against that crate's `.crate`. Releases `v0.2.0` to `v0.6.0` carry none |
| **Build provenance** | SLSA build-provenance attestations on every release, with `SHA256SUMS`. Each `.crate` is checked to have been built from the tagged commit |
| **Conformance attestation** | The A2A conformance suite's summary, attested onto each `.crate` |
| **Signed tags** | Since 2026-10-08 the release workflow refuses a tag not signed by a key in `.github/release-signers/`. `v0.15.0` is the first tag to pass. Check one with `scripts/verify_tag_signature.sh <tag>` |
| **Publishing** | crates.io over HTTPS with a short-lived token from Trusted Publishing; cargo verifies each download's checksum |
| **Dependency scanning** | `cargo-deny` on every pull request, push to `main` and release, with no advisory waived; OSV-Scanner over all nine tracked Cargo lockfiles on every pull request and push and daily, blocking |
| **Testing** | 17 fuzz targets on every pull request and nightly; mutation testing of the lines a pull request changes, and of every crate weekly; CodeQL on every pull request and weekly; the tests on Linux, macOS and Windows with every feature |
| **Policy** | `SECURITY.md`: two private channels, acknowledgement within three business days, triage within 14 days, a 90-day coordinated-disclosure ceiling, credit to reporters, and a section for manufacturers |

Verify the attestations with `gh attestation verify`, as step 3 of
[`RELEASING.md`](https://github.com/tomtom215/a2a-rust/blob/main/RELEASING.md)
shows.

## Obligations of yours that reach the crates

- **Due diligence on components** (Article 13(5)), free software included. The
  SBOMs, the scanning and the evidence above are what the project gives you
  toward it.
- **An SBOM of your product** (Annex I, Part II(1)), covering at least your
  top-level dependencies.
- **Reporting a vulnerability you find in a component** (Article 13(6)) to its
  maintainer. `SECURITY.md` accepts these reports and shared fixes on the same
  three-business-day acknowledgement, and defers to your statutory deadlines.
- **Notifying under Article 14.** This is yours, and it applies now.
  `SECURITY.md` tells a manufacturer that its legal deadlines win over the
  project's embargo: notify on the law's timetable and tell the project at the
  same time. The project has no channel to tell integrators of an actively
  exploited vulnerability beyond a GitHub advisory and the changelog.
- **The support period** (Article 13(8)). Fixes, security fixes included, go to
  the latest minor line only, for exactly as long as it is the latest: no
  overlap and no long-term line. The component offers far less than five years,
  and no end date can be known in advance. If you rely on the crates for a core
  function, pin them and plan to carry fixes in your own fork.
- **Risk assessment, technical documentation, declaration of conformity,
  delivering updates to your users.** These are the product's. The
  [Security](../deployment/security.md) chapter and
  [Production Hardening](../deployment/production.md) are material for your
  assessment; the project publishes no threat model of its own.

## The Act's security properties (Annex I, Part I)

Several of the SDK's defaults are not the secure choice. The
[overview](./overview.md#several-protections-are-off-until-you-switch-them-on)
lists them.

| Property | Map status | What the SDK carries, and what is left |
|---|---|---|
| (a) No known exploitable vulnerabilities | Partial | `cargo-deny` and OSV-Scanner, so no published manifest admits a dependency version with a RustSec advisory. The one advisory in the crates themselves is not in RustSec or OSV (below) |
| (b) Secure by default | Partial | TLS through rustls is on by default in the client; the server refuses a card whose security requirements no interceptor checks, caps bodies at 4 MiB and streams at 1,024, and refuses a tenant its stores cannot isolate. An agent whose card declares no security requirement serves every caller, and the server's own TLS is off until enabled |
| (c) Security updates | Yours | A fix ships as a patch release of the current minor line. A library has no update channel of its own |
| (d) Access control | Partial | API key (constant-time), bearer, JWT (fixed algorithm list; `alg: none` and algorithm confusion refused; HS256 secrets of at least 32 bytes) or client certificate; a per-task authorizer. None applies until you install it |
| (e) Confidentiality | Partial | TLS and mutual TLS; `EncryptedTaskStore` for data at rest, which leaves ids, roles, states, timestamps, artifact names and `metadata` in the clear. Off until enabled |
| (f) Integrity | Partial | Signed agent cards, signed messages and artifacts, sealed stores that fail the read if altered, and a hash-chained audit log. Each is opt-in, and the sealed store does not detect reordered or removed messages |
| (g) Data minimisation | Supported | Audit records hold content only as digests; spans carry no content unless you opt in; the in-memory store evicts terminal tasks after an hour |
| (h), (i) Availability, limiting impact | Partial | Body, stream, tenant and rate limits, an executor timeout and graceful shutdown. Rate limits are off until their interceptor is installed and count per process |
| (j) Small attack surface | Partial | Every transport, store and integration is behind a Cargo feature; the server's default features are tracing and OpenTelemetry only |
| (k) Reducing the impact of an incident | Partial | The four crates forbid `unsafe`, a CI ratchet fails if their panic surface grows, and the parsers of untrusted input are fuzzed. The dependencies contain `unsafe` code |
| (l) Logging and monitoring | Partial | With `with_audit`, every call and task event in a per-tenant hash chain; spans, metrics and typed failures. The audit trail is opt-in, and watching it is yours |
| (m) Secure removal of data | Partial | Erasure and purge calls. See [GDPR](./gdpr.md#what-erasure-does-not-reach) for what they do not reach |

## Handling vulnerabilities (Annex I, Part II)

- **SBOM** (II(1)): supported, above.
- **Regular testing** (II(3)): supported; fuzzing, mutation testing and CodeQL.
- **Reporting policy** (II(5), (6)): documented in `SECURITY.md`.
- **Fixing and disclosing** (II(2), (4), (8)): partial. The project's one
  precedent is GHSA-hr9h-6jvf-wvg6 (high, CVSS 8.1, a tenant-isolation bypass),
  fixed in 0.14.1, a patch release whose changelog holds the fix and internal
  changes only. The advisory was published with the fix and states the impact,
  the affected crates and version ranges, the patched version, workarounds and
  references.
- **Distribution** (II(7)): partial. Tags `v0.2.0` to `v0.7.0` are lightweight
  and `v0.8.0` to `v0.14.1` annotated but unsigned. A `.crate` on crates.io
  carries a checksum, not a signature; the attestations live on GitHub. Whether
  a repository rule restricts who may create `v*` tags is a setting the source
  tree cannot show.

### The advisory your scanner may not show

GHSA-hr9h-6jvf-wvg6 exists only on GitHub. Checked on 2026-10-21, the RustSec
database has no entry for it and OSV has no record, so `cargo audit`,
`cargo deny` and OSV-Scanner on your lockfile do not flag
`a2a-protocol-server` or `a2a-protocol-sdk` below 0.14.1. Filing it with
RustSec would close that; a draft entry is in
[`docs/upstream/`](https://github.com/tomtom215/a2a-rust/blob/main/docs/upstream/rustsec-ghsa-hr9h-6jvf-wvg6.md),
and filing it is the maintainer's action. Until it is filed, read the GitHub
advisory and the changelog, and keep the crates at 0.14.1 or later.

## Where to go next

- [Compliance Overview](./overview.md)
- [Security](../deployment/security.md): what holds by construction, what is
  left to you, and the known gaps.
- The [Cyber Resilience Act section of the control map](https://github.com/tomtom215/a2a-rust/blob/main/docs/compliance/control-map.md)
  has all 25 rows, with the tests and CI steps that prove them.
