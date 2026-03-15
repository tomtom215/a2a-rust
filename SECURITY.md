<!-- SPDX-License-Identifier: Apache-2.0 -->

# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Scope

This policy covers **all crates** in the a2a-rust workspace, including but not
limited to the core protocol types, server, and client libraries.

## Reporting a Vulnerability

If you discover a security vulnerability in this project, please report it
responsibly. **Do not open a public GitHub issue.**

### Preferred Channels

1. **Email:** Send a detailed report to **security@a2a-rust.dev**.
2. **GitHub Security Advisories:** Open a draft advisory at
   <https://github.com/a2a-rust/a2a-rust/security/advisories/new>.

### PGP Key

You may encrypt sensitive reports using our PGP key:

```
Fingerprint: <TO BE PUBLISHED>
```

The full public key will be published at
`https://a2a-rust.dev/.well-known/security-pgp-key.asc`.

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
