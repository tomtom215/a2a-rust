<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Maintainers

The authoritative list of who maintains this project. [`GOVERNANCE.md`](GOVERNANCE.md)
defines what the roles *mean* and how decisions are made; this file records who
currently holds them, so the two do not drift.

## Current maintainers

| Name | GitHub | Email | Role | Since |
|---|---|---|---|---|
| Tom F. | [@tomtom215](https://github.com/tomtom215) | `tomf@tomtomtech.net` | Initial Maintainer | 2026-03-15 |

## Committers

None. No contributor has yet been nominated under
[`GOVERNANCE.md`](GOVERNANCE.md#committer).

## Emeritus

None.

---

## The bus factor is 1, and this file is where that is easiest to see

One row is not a formality to be filled in later. It has three consequences
that are worth stating in the same place as the list itself, rather than
leaving a reader to infer them:

1. **No independent review path.** Every pull request that is merged is
   approved by the person who wrote or directed it. [`.github/CODEOWNERS`](.github/CODEOWNERS)
   names an owner for every path, but with one maintainer that mechanism
   requests review from the author. It is real for outside contributions and
   inert for the maintainer's own.

2. **No independent escalation for a conduct report about the maintainer.**
   [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) and
   [`GOVERNANCE.md`](GOVERNANCE.md) both say this explicitly. It is a
   structural gap that no document can close — it needs a second person.

3. **No continuity.** If this maintainer stops, the project stops. There is no
   succession provision, because there is no one to succeed to.

None of this is a reason to distrust the code, which is gated by mechanisms
that do not depend on who is watching — the conformance suites, the mutation
gate, the fuzzers, the DCO check, and the gate-proof harnesses that verify
those gates can fail. It *is* a reason not to read a complete set of governance
files as a complete governance story. See
[`docs/rust-sdk-assessment.md` §7](docs/rust-sdk-assessment.md) for what that
distinction means in practice for downstream adoption.

## Becoming a maintainer

Per [`GOVERNANCE.md`](GOVERNANCE.md#committer): committers are nominated by
maintainers on the basis of sustained, high-quality contributions, and
maintainers are drawn from committers. Reducing the bus factor is the
project's most valuable outstanding non-code contribution; interest is welcome
via a GitHub issue or the email above.

## Security and conduct contacts

See [`SECURITY.md`](SECURITY.md) for vulnerability reporting and
[`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) for conduct reports. Note that
addresses at `a2a-rust.dev` do **not** work — the domain is unregistered
(NXDOMAIN, re-verified 2026-08-11). Use the GitHub channels or the email
above.
