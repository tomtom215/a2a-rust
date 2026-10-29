<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Conformance testing

How to check an A2A server against the specification with the suites this
repository runs: its own `a2a-tck` runner, the A2A project's official
`a2aproject/a2a-tck`, and ACTS from `a2aproject/a2a-itk`. Each grades the
server from the outside, over the wire, so each works on any A2A v1.0 server,
not only one built with this SDK. The results this repository has recorded
are in [Conformance History](./conformance-history.md).

## Which tool do I need

| Tool | Grades | Runs as | Use it to |
|---|---|---|---|
| The `conformance` feature | Your `AgentExecutor`, in process | `cargo test` | Check the code you wrote before it is behind a server; see [Testing Your Agent](../deployment/testing.md) |
| In-repo `a2a-tck` (`tck/`) | A running server, over JSON-RPC, HTTP+JSON, gRPC and WebSocket, and §5.1 cross-binding equivalence | A Rust binary | Check a server quickly, on every binding, with nothing but Cargo |
| Official `a2aproject/a2a-tck` | A running server, against the specification's MUST requirements, over JSON-RPC, HTTP+JSON and gRPC | A Python suite | Measure what the A2A project's own suite says |
| ACTS (`a2aproject/a2a-itk`) | A server that implements the ACTS test behaviours, at the MUST, SHOULD and MAY levels | Docker, through `itk/run_itk.sh` | Grade against the A2A project's second suite, as CI does |
| ITK cross-language interop (`itk/`) | This SDK against agents built on the official Python, JavaScript, Go and Java SDKs | Agents per language, then `a2a-tck` | Check interoperability with the reference SDKs |

## The in-repo `a2a-tck`

`a2a-tck` checks the wire format of every A2A v1.0 method: ProtoJSON naming,
discriminated unions, security-scheme shapes, agent-card discovery, and the
errors each binding must answer (§5.4). It depends on none of the
`a2a-protocol-*` crates and compiles `a2a.proto` itself, so it does not share
this SDK's reading of the specification. `--url` is always the agent's HTTP
origin: the card is fetched there, and for `websocket` and `grpc` the
listener's address is read from the card.

```bash
# JSON-RPC (§9), the default
cargo run -p a2a-tck -- --url http://localhost:3000 --binding jsonrpc

# HTTP+JSON (§11)
cargo run -p a2a-tck -- --url http://localhost:3000 --binding rest

# WebSocket (§12 custom binding)
cargo run -p a2a-tck -- --url http://localhost:3000 --binding websocket

# gRPC (§10)
cargo run -p a2a-tck -- --url http://localhost:3000 --binding grpc

# Cross-binding equivalence (§5.1), over every binding the card advertises
cargo run -p a2a-tck -- --url http://localhost:3000 --equivalence
```

| Flag | Purpose |
|---|---|
| `--ws-url ws://host:port`, `--grpc-url host:port` | The endpoint of a listener the card does not advertise. Passing one with a binding that will not dial it is a configuration error. |
| `--skip <tests>` | Comma-separated test names to skip; repeatable. Only for documented deviations of the target: a skipped test is reported, and a skipped test that passes fails the run as a stale skip. Not accepted with `--equivalence`. |
| `--auth-token <t>` | The bearer token `BIND-EQUIV-004`'s enforcement probe presents. Only with `--equivalence`; without it the check grades the rejection half only. |

| Exit code | Meaning |
|---|---|
| `0` | Every check passed |
| `1` | A check failed, or a `--skip`ped check now passes |
| `2` | A configuration error, an endpoint that cannot be discovered, or a run that graded nothing |

A check that cannot apply to a binding is reported `N/A` with its reason and
left out of the score; it is never counted as a pass. A run that grades no
check exits 2.

### Cross-binding equivalence

`--equivalence` grades four MUST requirements about the relation between
bindings: the same operations on every binding (`BIND-EQUIV-001`), equivalent
results (`002`), consistent error mapping (`003`), and the same
authentication schemes (`004`). It needs at least two bindings, and its
`4/4` counts requirements, not bindings: the "Comparing N bindings" line says
how many were compared. CI asserts the card advertises all four before it
grades.

`BIND-EQUIV-004` has two halves. Against a server whose card declares
`securityRequirements`, and with `--auth-token`, the check sends requests
without credentials (every binding must refuse them) and with the token
(every binding must serve them). Against a card with no security
requirements it grades only that the card declares its schemes once, at card
level.

### Targets in this repository

| Target | Serves | Start it with |
|---|---|---|
| `examples/echo-agent` | JSON-RPC and HTTP+JSON, plus WebSocket with `A2A_WS_BIND_ADDR` | `A2A_BIND_ADDR=127.0.0.1:9090 A2A_WS_BIND_ADDR=127.0.0.1:9091 cargo run -p echo-agent` |
| `tck/sut` | All four bindings | `SUT_HOST=127.0.0.1:9999 SUT_GRPC_HOST=127.0.0.1:9998 SUT_WS_HOST=127.0.0.1:9997 cargo run -p a2a-tck-sut` |

`tck/sut` is the system under test the official suite runs against too.
`SUT_PROFILE` picks what its card declares: unset for the full profile,
`minimal` for one without streaming, push notifications or an extended card
(so the capability-rejection paths can be graded), `extension` for the full
card plus one extension declared `required: true`, and
`secured` for one that requires a bearer token, which is
`tck-bind-equiv-004-token`. `SUT_HOST` defaults to `127.0.0.1:9999`, and
`SUT_GRPC_HOST` to the port below it. CI grades the secured profile like this:

```bash
SUT_PROFILE=secured SUT_HOST=127.0.0.1:9899 \
  SUT_GRPC_HOST=127.0.0.1:9898 SUT_WS_HOST=127.0.0.1:9897 \
  ./target/release/a2a-tck-sut &
./target/release/a2a-tck --url http://127.0.0.1:9899 --equivalence \
  --auth-token tck-bind-equiv-004-token
```

## The official `a2aproject/a2a-tck`

The A2A project's suite, in Python. It reports each requirement as `PASS`,
`FAIL`, `ERROR`, `SKIPPED` or `NOT TESTED`, and writes
`reports/compatibility.json`. Run it against `tck/sut` (or your own server)
with [uv](https://docs.astral.sh/uv/):

```bash
cargo build --release -p a2a-tck-sut
SUT_HOST=127.0.0.1:9999 SUT_GRPC_HOST=127.0.0.1:9998 ./target/release/a2a-tck-sut &

git clone --depth 1 https://github.com/a2aproject/a2a-tck /tmp/a2a-tck
cd /tmp/a2a-tck && uv venv && uv pip install -e .
./.venv/bin/python run_tck.py --sut-host http://127.0.0.1:9999
```

Each run overwrites `reports/compatibility.json`; copy it out before the next
profile's run. `official-tck.yml` runs three profiles, each against its own
`tck/sut`: the full one, `minimal`, and `extension` scoped with
`-k "TestCapabilityExtensionRequired"` (the suite does not send
`A2A-Extensions` on ordinary requests). It then gates each report with
`tck/scripts/check_conformance.py` against `tck/conformance-baseline.json`:

```bash
python3 tck/scripts/check_conformance.py \
  --report /tmp/a2a-tck/reports/compatibility.json \
  --baseline tck/conformance-baseline.json \
  --min-graded 88
```

The gate fails on a MUST failure the baseline does not list, on a baselined
failure that now passes (fix the baseline in the same change, with
`--update`), and, with `--min-graded`, on a run that graded fewer MUST
requirements than the floor (88 for the full profile, 66 for the minimal).
The extension run is gated with `--require-pass CORE-CAP-004` instead, so a
renamed test fails loudly rather than selecting nothing.

Two numbers need care when you quote a result. The suite's MUST compatibility
percentage is computed over the requirements it tested, so it says nothing
about the ones it skipped or has no test for. And the suite floats: CI runs
upstream `main`, so two runs are comparable only at the same `a2a-tck`
commit. [`docs/official-tck-findings.md`](https://github.com/tomtom215/a2a-rust/blob/main/docs/official-tck-findings.md)
§6 has every reproduction command, including the wire capture.

## ACTS

ACTS is the A2A project's second conformance suite, in `a2aproject/a2a-itk`.
It grades tests at the MUST, SHOULD and MAY levels, and its tests drive the
agent with messages whose text starts with a `tck-*` prefix (complete the
task, ask for input, reject, fail, stay working until canceled, and so on).
This repository's ITK agent implements them: the prefixes are listed in
`acts/sut-behaviors.yaml`, which the runner reads, and implemented in
`itk/src/acts.rs`.

Run it the way `acts.yml` does, with Docker or Podman. `itk/run_itk.sh`
clones `a2a-itk` at the revision you name if `itk/a2a-itk` does not exist:

```bash
cd itk
A2A_ITK_REVISION=bcce1a15fdce343e2d816ee7e7119f68147385de \
  ITK_ACTS_RUN=1 ITK_ACTS_TRANSPORTS=jsonrpc,grpc,rest \
  bash run_itk.sh
```

The run writes `itk/acts-report-*.json`, one per binding. `acts.yml` then
applies `scripts/acts_predicate.py`, which fails on any failed or errored test
at any level; upstream's own `--require-conformant` fails on a MUST alone.
`acts.yml` runs on every pull request and push to `main`, and `release.yml`
runs it on the tagged commit and attests the summary onto each `.crate`.
`acts/reports/` keeps a committed run, with the command that tallies it.

## Cross-language interop

`itk/` holds echo agents written with the official SDKs (`a2a-sdk` for
Python, `@a2a-js/sdk`, `a2a-go/v2`, `a2a-java`) and with dependency-light
stubs in each language. `tck.yml` starts each agent and runs `a2a-tck`
against it, which checks those SDKs' servers with this repository's runner,
and runs the official Python SDK's client against `echo-agent`. To run the
stub agents and the Rust echo agent locally:

```bash
docker compose -f itk/docker-compose.yml up --build --abort-on-container-exit
```

The official-SDK agents are not in the Compose file; the setup commands for
each are the `python-sdk`, `js-sdk`, `go-sdk` and `java-sdk` legs of
`tck-cross-language` in `tck.yml`.
[`itk/README.md`](https://github.com/tomtom215/a2a-rust/blob/main/itk/README.md)
has the ports, the manual steps and the history of the reference-SDK
divergences found this way.

## What CI runs

| Workflow | Runs | Gate |
|---|---|---|
| `tck.yml` | `a2a-tck` against `echo-agent` and `tck/sut` on every binding, equivalence on both SUT profiles, and the cross-language matrix | Yes |
| `official-tck.yml` | The official suite's three profiles against `tck/sut` | Yes, through `check_conformance.py` |
| `acts.yml` | ACTS over JSON-RPC, gRPC and HTTP+JSON | Yes, and on each release |
| `itk-nightly.yml` | The upstream ITK's interop scenarios against every peer SDK | No: metrics only |
| `pin-freshness.yml` | Every three weeks, re-resolves each official SDK pin to its latest release and grades it | Reports; never commits a pin bump |

[Suppression register](./conformance-history/suppressions.md) lists every
mechanism that narrows what these jobs measure, and when each can go.
