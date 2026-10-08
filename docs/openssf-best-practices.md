<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# OpenSSF Best Practices — passing-level evidence

Answers for the [OpenSSF Best Practices badge](https://www.bestpractices.dev/)
self-certification, one row per criterion, each pointing at the file or
measurement that supports it. **The badge itself is not yet claimed:**
registering the project at bestpractices.dev is done by the maintainer while
signed in to GitHub, and this file is what to paste from. Once registered, add
the project number to the README badge and to the line below.

- Project entry: _not yet registered_.
- Criteria source: `criteria/criteria.yml` and `config/locales/en.yml` of
  `coreinfrastructure/best-practices-badge` at `80662ad8`, read 2026-10-08:
  67 active passing-level criteria — 43 MUST, 10 SHOULD, 14 SUGGESTED.
- Evidence checked 2026-10-08 at the commit that adds this file.

A row marked **Met** says what was checked. A row that rests on a judgement
rather than a measurement says so.

## Basics

| Criterion | Level | Answer | Evidence |
|---|---|---|---|
| `description_good` | MUST | Met | README opening paragraph and <https://a2a-rust.com> (HTTP 200, checked 2026-10-08): a Rust SDK for the Agent2Agent protocol. |
| `interact` | MUST | Met | README (install), `SUPPORT.md` (questions, bug reports), `CONTRIBUTING.md` (contributing). |
| `contribution` | MUST | Met | <https://github.com/tomtom215/a2a-rust/blob/main/CONTRIBUTING.md> — pull requests, DCO sign-off, the PR checklist. |
| `contribution_requirements` | SHOULD | Met | Same file: "Coding Standards", "Test Requirements", "Quality Gates", "PR Checklist". |
| `floss_license` | MUST | Met | Apache-2.0 (`LICENSE`; `license = "Apache-2.0"` in every published manifest). |
| `floss_license_osi` | SUGGESTED | Met | Apache-2.0 is OSI-approved. |
| `license_location` | MUST | Met | <https://github.com/tomtom215/a2a-rust/blob/main/LICENSE> |
| `documentation_basics` | MUST | Met | The book at <https://a2a-rust.com>; README; crate READMEs. |
| `documentation_interface` | MUST | Met | API reference on docs.rs, built from `#![deny(missing_docs)]` crates; the book's reference section. |
| `sites_https` | MUST | Met | github.com, a2a-rust.com, crates.io and docs.rs all serve HTTPS. |
| `discussion` | MUST | Met | GitHub issues and pull requests: searchable, URL-addressable, open to anyone with a GitHub account, no proprietary client. |
| `english` | SHOULD | Met | All documentation is in English; reports are accepted in English. |
| `maintained` | MUST | Met | Commits within the last week; releases 0.14.0 (2026-09-26) and 0.14.1 (2026-09-30). |

## Change control

| Criterion | Level | Answer | Evidence |
|---|---|---|---|
| `repo_public` | MUST | Met | <https://github.com/tomtom215/a2a-rust> |
| `repo_track` | MUST | Met | git: author, committer, date and DCO sign-off on every commit (`dco.yml` enforces sign-off). |
| `repo_interim` | MUST | Met | Every pull request and its commits are public before a release. |
| `repo_distributed` | SUGGESTED | Met | git. |
| `version_unique` | MUST | Met | Each release has a unique SemVer version; `release.yml` refuses a tag that disagrees with the manifests. |
| `version_semver` | SUGGESTED | Met | SemVer, pre-1.0 rules stated in `STABILITY.md`. |
| `version_tags` | SUGGESTED | Met | Annotated tags `v0.8.0` onward; `release.yml` refuses a lightweight tag. |
| `release_notes` | MUST | Met | <https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md> (Keep a Changelog), mirrored in each GitHub release. |
| `release_notes_vulns` | MUST | Met | 0.14.1's CHANGELOG entry names GHSA-hr9h-6jvf-wvg6. |

## Reporting

| Criterion | Level | Answer | Evidence |
|---|---|---|---|
| `report_process` | MUST | Met | <https://github.com/tomtom215/a2a-rust/issues>, described in `SUPPORT.md`. |
| `report_tracker` | SHOULD | Met | GitHub issues. |
| `report_responses` | MUST | Met | The one bug report opened between 2025-10-08 and 2026-08-08 (the 2-to-12-month window on 2026-10-08), #66, was answered and closed the next day. Measured from the issue list on 2026-10-08: three issues in total, all closed. |
| `enhancement_responses` | SHOULD | N/A in practice | No enhancement requests in the window; nothing to respond to. Answer Met if the form requires a value, with that note. |
| `report_archive` | MUST | Met | GitHub issues are public and searchable. |
| `vulnerability_report_process` | MUST | Met | <https://github.com/tomtom215/a2a-rust/blob/main/SECURITY.md> |
| `vulnerability_report_private` | MUST | Met | `SECURITY.md`: GitHub private security advisories (preferred) or email. No PGP key yet; the file says so. |
| `vulnerability_report_response` | MUST | Met | `SECURITY.md` commits to acknowledgement within 3 business days. GHSA-hr9h-6jvf-wvg6 was fixed and released in 0.14.1. |

## Quality

| Criterion | Level | Answer | Evidence |
|---|---|---|---|
| `build` | MUST | Met | `cargo build`; CI builds on Linux, macOS and Windows. |
| `build_common_tools` | SUGGESTED | Met | Cargo. |
| `build_floss_tools` | SHOULD | Met | rustc and Cargo; `protoc` is vendored (`protoc-bin-vendored`). |
| `test` | MUST | Met | `cargo test --workspace`; documented in `CONTRIBUTING.md` "Running Tests". 4,283 passed, 0 failed with all features on 2026-10-08. |
| `test_invocation` | SHOULD | Met | `cargo test`. |
| `test_most` | SUGGESTED | Met | Coverage via `coverage.yml` (Codecov); per-PR mutation testing on changed lines (`mutants.yml`). |
| `test_continuous_integration` | SUGGESTED | Met | `ci.yml` and the other workflows run on every pull request. |
| `test_policy` | MUST | Met | `CONTRIBUTING.md` "Test Requirements" and the PR checklist ("New code has tests"). |
| `tests_are_added` | MUST | Met | Enforced, not only stated: the incremental mutation gate fails a PR when a changed line can be mutated without a test failing. |
| `tests_documented_added` | SUGGESTED | Met | `CONTRIBUTING.md` PR checklist. |
| `warnings` | MUST | Met | `RUSTFLAGS=-D warnings`; clippy with `pedantic` and `nursery` on every crate root. |
| `warnings_fixed` | MUST | Met | CI fails on any warning. |
| `warnings_strict` | SUGGESTED | Met | Same. |

## Security

| Criterion | Level | Answer | Evidence |
|---|---|---|---|
| `know_secure_design` | MUST | Met (judgement) | The maintainer's self-assessment; supported by the design record in `docs/adr/` (auth, SSRF guards, tenant isolation) and the audits in `docs/`. |
| `know_common_errors` | MUST | Met (judgement) | As above; `book/src/deployment/security.md` and `security-testing.md` name the errors this class of software makes and the mitigations. |
| `crypto_published` | MUST | Met | Only published algorithms: ES256, RS256, HS256 (JOSE), SHA-256, TLS via rustls. |
| `crypto_call` | SHOULD | Met | All cryptography is `ring` and `rustls`; nothing is implemented in-tree. |
| `crypto_floss` | MUST | Met | `ring` and `rustls` are FLOSS. |
| `crypto_keylength` | MUST | Met | RSA verification is `ring`'s `RSA_PKCS1_2048_8192_SHA256`, which rejects keys under 2048 bits; ECDSA is P-256; HS256 secrets under 32 bytes are refused since 2026-10-08 (`MIN_HS256_SECRET_LEN`, RFC 7518 §3.2). |
| `crypto_working` | MUST | Met | No MD4, MD5, single DES, RC4 or Dual_EC_DRBG anywhere in the published crates. |
| `crypto_weaknesses` | SHOULD | Met | No SHA-1 in any default mechanism; rustls offers no CBC suites. |
| `crypto_pfs` | SHOULD | Met | rustls key exchange is ephemeral (ECDHE) only. |
| `crypto_password_storage` | MUST | N/A | The SDK stores no user passwords. API keys and bearer tokens are operator-supplied and compared in constant time; they are not a password store. |
| `crypto_random` | MUST | Met | Signing nonces come from `ring::rand::SystemRandom` (`crates/a2a-protocol-types/src/signing.rs`). The SDK generates no long-term keys. |
| `delivery_mitm` | MUST | Met | crates.io and GitHub releases over HTTPS; release artifacts carry SLSA build-provenance attestations. |
| `delivery_unsigned` | MUST | Met | `SHA256SUMS` is published over HTTPS alongside attestations, never fetched over HTTP. |
| `vulnerabilities_fixed_60_days` | MUST | Met | No unpatched medium-or-higher vulnerability in a published crate. The unpublished SLIMRPC binding carries one dated waiver (RUSTSEC-2026-0285, blocked upstream; `SECURITY.md`, `bindings/a2a-protocol-slimrpc/deny.toml`). |
| `vulnerabilities_critical_fixed` | SHOULD | Met | GHSA-hr9h-6jvf-wvg6 was fixed in a patch release. |
| `no_leaked_credentials` | MUST | Met | gitleaks 8.28.0 over all 1,374 reachable commits on 2026-10-08: 12 matches, all test fixtures — 11 JWT test vectors signed with test keys, and one 32-hex-character idempotency key in a store test. No working credential. |

## Analysis

| Criterion | Level | Answer | Evidence |
|---|---|---|---|
| `static_analysis` | MUST | Met | clippy (`pedantic`, `nursery`) on every PR; CodeQL (`codeql.yml`: Rust, Actions, Python) on every PR and weekly. |
| `static_analysis_common_vulnerabilities` | SUGGESTED | Met | CodeQL `security-extended` queries; cargo-deny and OSV-Scanner for known-vulnerable dependencies. |
| `static_analysis_fixed` | MUST | Met | CI fails on clippy findings; CodeQL alerts are triaged in code scanning. |
| `static_analysis_often` | SUGGESTED | Met | Every commit (clippy), every PR (CodeQL). |
| `dynamic_analysis` | SUGGESTED | Met | 13 libFuzzer targets (`fuzz.yml`, nightly and on PRs); soak tests (`soak.yml`). |
| `dynamic_analysis_unsafe` | SUGGESTED | N/A | The published crates are `#![forbid(unsafe_code)]` Rust. |
| `dynamic_analysis_enable_assertions` | SUGGESTED | Met | cargo-fuzz adds `-Cdebug-assertions` unless `-O` is passed (`src/project.rs`, `if !build.release \|\| build.debug_assertions …`); `fuzz.yml` runs `cargo +nightly fuzz run` without `-O`. |
| `dynamic_analysis_fixed` | MUST | Met | A crash fails the fuzz job and uploads its input (`fuzz.yml`, "Upload crash artifacts on failure"); no fuzz-found defect is open in the tracker. Crashing inputs are not committed as regression seeds — the fix's own unit test is the regression test. |
