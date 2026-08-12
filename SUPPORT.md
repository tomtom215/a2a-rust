<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Support

How to get help with `a2a-rust`, and what to expect.

## Set expectations first

This project has **one maintainer** ([`MAINTAINERS.md`](MAINTAINERS.md)) and no
commercial support offering. There is no SLA on anything below except the
security channel, which has a stated acknowledgement window because a
vulnerability report should not sit unread.

If you are evaluating this for production use, that staffing fact belongs in
your decision alongside the test and conformance evidence. Both are real.

## Pick a channel

| You want to | Use | Not |
|---|---|---|
| Report a security vulnerability | [`SECURITY.md`](SECURITY.md) — GitHub Security Advisories, privately | a public issue |
| Report a bug | [GitHub Issues](https://github.com/tomtom215/a2a-rust/issues) | email |
| Ask "how do I …?" | [GitHub Discussions](https://github.com/tomtom215/a2a-rust/discussions) — enabled, verified 2026-08-11 | email |
| Propose a feature or design change | An issue first, per [`GOVERNANCE.md`](GOVERNANCE.md)'s lazy-consensus process | a surprise pull request |
| Contribute code | [`CONTRIBUTING.md`](CONTRIBUTING.md) | — |
| Report a conduct concern | [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) | — |

**Do not report security vulnerabilities in public issues.** See
[`SECURITY.md`](SECURITY.md) for the private channel and the disclosure
timeline.

**Addresses at `a2a-rust.dev` do not work.** The domain is unregistered — DNS
returns NXDOMAIN, re-verified 2026-08-11 — so `security@a2a-rust.dev` and
`conduct@a2a-rust.dev` are undeliverable. Anything sent there is silently lost.
Use the GitHub channels, or `tomf@tomtomtech.net`.

## Read these before asking

Most questions are answered by material already in the repository, and it is
unusually detailed for a project this size:

- **[The book](https://tomtom215.github.io/a2a-rust/)** — `book/src/`, built
  from this repository. Start at "Building Agents".
- **[`README.md`](README.md)** — what the crates are and which one you want.
- **[`examples/`](examples/)** — six runnable agents, including the
  `echo-agent` the conformance suite runs against.
- **[`SPEC_COMPLIANCE.md`](SPEC_COMPLIANCE.md)** — what of the A2A v1.0
  specification is implemented.
- **[`book/src/reference/configuration.md`](book/src/reference/configuration.md)**
  — every builder option.
- **[`docs/adr/`](docs/adr/)** — ten architecture decision records, for "why is
  it like this?"

If you are asking whether a behaviour is conformant, check
[`book/src/reference/conformance-history.md`](book/src/reference/conformance-history.md)
first. It records what the conformance suites actually measured, and — equally
usefully — the register of everything they did *not* ask.

## What makes a bug report useful here

The project's own gates are strict, so a report that lets them be pointed at
your case is worth far more than a description:

1. The crate and version (`a2a-protocol-server 0.7.0`, not "latest").
2. The transport binding — JSON-RPC, HTTP+JSON, gRPC or WebSocket. Behaviour
   differs by binding and the first question will otherwise be this one.
3. A minimal reproduction, ideally a failing test against one of the
   `examples/` agents.
4. What you expected, with a pointer to the specification section or the book
   page that led you to expect it.
5. `RUST_LOG=debug` output if the failure is at runtime.

If you believe the behaviour violates the A2A specification, say which
requirement. The official TCK grades this SDK against 88 MUST-level
requirements on its full profile, so if your case is covered by one of them,
that is a strong report and it will be treated as one.

## Supported versions

See [`SECURITY.md`](SECURITY.md) for the supported-version policy. In short:
the project is pre-1.0, and the current minor release is the supported one.

## Commercial support

None is offered.
