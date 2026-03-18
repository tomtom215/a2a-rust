<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# Project Governance

This document describes the governance model for the a2a-rust project.

## Roles

### Contributor

Anyone who submits a pull request, files an issue, or participates in
discussions. No special permissions are required.

### Committer

Trusted contributors granted write access to the repository. Committers may
merge pull requests and triage issues. Committers are nominated by maintainers
based on sustained, high-quality contributions.

### Maintainer

Maintainers set the project's technical direction, approve architectural
changes, manage releases, and administer repository settings. Maintainers have
final decision-making authority.

**Current Maintainers:**

| Name   | GitHub        | Role               |
| ------ | ------------- | ------------------ |
| Tom F. | @tomtom215    | Initial Maintainer |

## Decision-Making Process

The project uses **lazy consensus**:

1. Proposals (features, design changes, policy updates) are submitted as GitHub
   issues or pull requests.
2. If no objections are raised within **7 calendar days**, the proposal is
   considered accepted.
3. Trivial changes (typos, minor docs fixes) may be merged immediately by any
   committer.

### Escalation

If consensus cannot be reached:

1. The topic is discussed in a dedicated GitHub issue with all viewpoints
   documented.
2. Maintainers call for a decision with a clear deadline (minimum 7 days).
3. If disagreement persists, maintainers hold a majority vote. In the event of
   a tie, the initial maintainer casts the deciding vote.

## Code of Conduct

All participants are expected to behave respectfully and professionally.
Violations may be reported to the maintainers at security@a2a-rust.dev.

## Release Process

1. **Semantic Versioning** -- All crates follow [SemVer 2.0.0](https://semver.org/).
2. **Changelog** -- Every release must include an updated `CHANGELOG.md` entry
   following the [Keep a Changelog](https://keepachangelog.com/) format.
3. **Release Steps:**
   - A maintainer opens a release PR updating version numbers and the changelog.
   - The PR must pass CI and receive at least one maintainer approval.
   - After merging, the maintainer tags the commit and publishes to crates.io.
4. **Hotfixes** -- Security and critical bug fixes may bypass the normal review
   period at a maintainer's discretion.
