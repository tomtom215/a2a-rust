<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ACTS and in-repo TCK runs

Part of [Conformance History](../conformance-history.md). How to run both is in [Conformance Testing](../conformance-testing.md).

## ACTS (`a2aproject/a2a-itk`)

The A2A project's second conformance suite, run against this repository's ITK
agent (`itk/`) over JSON-RPC, gRPC and HTTP+JSON. Counts are ACTS tests passed
of tests run, at every level.

| Date | a2a-itk | JSON-RPC | gRPC | HTTP+JSON | Not passed |
|---|---|---|---|---|---|
| 2026-10-08 | `82458cea` | 101/101 | 88/88 | 94/94 | none |

By level (MUST, SHOULD, MAY): JSON-RPC 59/59, 29/29, 13/13; gRPC 47/47, 28/28,
13/13; HTTP+JSON 49/49, 32/32, 13/13 (`acts/reports/2026-10-08/`). `REST-CT-001`,
the SHOULD failure recorded under [Deliberate deviations](../conformance-history.md#deliberate-deviations), passes in this
run.

Since 2026-10-09 `acts.yml` runs ACTS on every pull request and every push to
`main`, and `release.yml` runs it on each tagged commit and attests the summary
onto the `.crate` files. `acts.yml` pins a2a-itk at `bcce1a15` (2026-10-23), so
the row above was graded at an earlier revision than the one the gate uses now,
and the gate's own runs are not rows here. Its verdict is
`scripts/acts_predicate.py`, which fails on any failed or errored test at any
level; upstream's `--require-conformant` fails on a MUST alone.

## In-repo `a2a-tck` runner

22 conformance checks per binding. `Skipped` counts documented deviations of
the *target* implementation, never of this SDK — see register entries W5/W6.

| Date | Target | Binding | Passed | Failed | N/A | Skipped | Exit |
|---|---|---|---:|---:|---:|---:|---:|
| 2026-08-09 | `examples/echo-agent` | jsonrpc | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-09 | `examples/echo-agent` | rest | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-09 | `itk/agents/js-sdk` (`@a2a-js/sdk` 1.0.0) | jsonrpc | 20/20 | 0 | 0 | 2 | 0 |
| 2026-08-09 | `itk/agents/js-sdk` (`@a2a-js/sdk` 1.0.0) | rest | 21/21 | 0 | 0 | 1 | 0 |
| 2026-08-10 | `examples/echo-agent` | jsonrpc | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-10 | `examples/echo-agent` | rest | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-10 | `examples/echo-agent` | websocket | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-10 | `tck/sut` | jsonrpc | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-10 | `tck/sut` | rest | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-10 | `tck/sut` | websocket | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-10 | `tck/sut` | grpc | 20/20 | 0 | 2 | 0 | 0 |
| 2026-08-11 | `tck/sut` | jsonrpc | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-11 | `tck/sut` | rest | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-11 | `tck/sut` | websocket | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-11 | `tck/sut` | grpc | 20/20 | 0 | 2 | 0 | 0 |
| 2026-08-12 | `tck/sut` | jsonrpc | 22/22 | 0 | 0 | 0 | 0 |
| 2026-08-12 | `tck/sut` | rest | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-12 | `tck/sut` | websocket | 21/21 | 0 | 1 | 0 | 0 |
| 2026-08-12 | `tck/sut` | grpc | 20/20 | 0 | 2 | 0 | 0 |

The `N/A` column is new on 2026-08-10 and is not cosmetic. Until then
`jsonrpc_envelope_format` opened with `if binding != "jsonrpc" { return Ok(()) }`,
so the `rest` rows above scored 22/22 while only 21 checks ran. Not-applicable
outcomes are now excluded from the denominator and printed with their reason,
and a run that grades zero checks exits 2 rather than green.

### Cross-binding equivalence (§5.1)

A separate mode — `--equivalence` — because these four requirements are about
the relation *between* bindings and each is trivially satisfied by any one of
them. Requirement texts and IDs are quoted from
`a2aproject/a2a-tck@5996b79` (2026-06-29).

| Date | Target | Bindings compared | Passed | Failed | Exit |
|---|---|---|---:|---:|---:|
| 2026-08-10 | `tck/sut` | JSONRPC, HTTP+JSON, GRPC, WEBSOCKET | 4/4 | 0 | 0 |
| 2026-08-11 | `tck/sut` | JSONRPC, HTTP+JSON, GRPC, WEBSOCKET | 4/4 | 0 | 0 |
| 2026-08-12 | `tck/sut` | JSONRPC, HTTP+JSON, GRPC, WEBSOCKET | 4/4 | 0 | 0 |

**`4/4` counts requirements, not bindings, and the two are independent.** The
"Bindings compared" column is copied from the run's own `Comparing N bindings:`
line, which is a separate number the `4/4` does not constrain: the tool's floor
is **two** bindings (`equivalence.rs:1048`, `ifaces.len() < 2` → hard error,
"reporting a pass would mean nothing"), not four. A run against a card
advertising three bindings prints `Comparing 3 bindings` and then `4/4`, and
exits 0.

Measured 2026-08-11, by starting `tck/sut` without `SUT_WS_HOST` so the card
advertises no WebSocket interface: `--equivalence` reported `Comparing 3
bindings` … `Results: 4/4 requirements passed` and **exit 0**. Nothing in the
tool objected.

That is not a hole in CI, because `tck.yml`'s `tck-all-bindings` job asserts
the four-interface card *before* grading, in a step of its own. Proven the same
way on the same day — against the same three-binding SUT that step printed
`card is missing WEBSOCKET` and exited 1. The four-binding claim in this table
rests on that assertion, not on the `4/4`. It is recorded here because reading
`4/4` as "four bindings agreed" is the natural misreading and it would be
wrong.

### `BIND-EQUIV-004`'s enforcement half — closed 2026-08-11

Until 2026-08-11 this section read: *"`BIND-EQUIV-004` is graded structurally
only … Proving each binding enforces those schemes identically needs a target
configured to require credentials, which no job in this repo currently
provides. That half is unmeasured, not passing."*

There is now such a target. `tck/sut` gained a `SUT_PROFILE=secured` profile
whose card declares a bearer scheme and whose handler enforces it with a single
`BearerTokenAuthInterceptor` — one interceptor above the dispatchers, which is
the property being verified: JSON-RPC, HTTP+JSON, gRPC and WebSocket are guarded
by one implementation reading one `CallContext`.

| Date | Target | Bindings | Structural | Enforcement | Exit |
|---|---|---|---|---|---:|
| 2026-08-11 | `tck/sut` (`SUT_PROFILE=secured`) | 4 | PASS | **PASS — both halves** | 0 |
| 2026-08-12 | `tck/sut` (`SUT_PROFILE=secured`) | 4 | PASS | **PASS — both halves** | 0 |

"Both halves" is the load-bearing phrase. The check sweeps twice:

* **without credentials, every binding must refuse** — one binding serving an
  anonymous caller while the others refuse is the asymmetry §5.1 forbids, and
  it is the realistic defect, since a transport that forgets to forward the
  header its authenticator reads looks completely normal until someone tries it;
* **with credentials, every binding must serve** — without this, the check
  passes trivially against a server that is simply broken.

That second sweep earned its place immediately. The first draft of the probe
sent the JSON-RPC method as `tasks/list` where this SDK's name is `ListTasks`;
it authenticated correctly and then failed method dispatch, so JSON-RPC and
WebSocket both reported a refusal and the check declared a binding asymmetry
that did not exist. **On the rejection sweep alone, a probe that can never
succeed is indistinguishable from enforcement working.** Recorded because it is
the same failure shape as the gates this repo has found that could not fail
(the mutation history numbers eight in the mutation gate alone), caught this
time before it shipped.

Measured the same day, by injection, that the check can fail: run with a
deliberately wrong token it exits 1 and names all four bindings as refusing.
Run against the ordinary `full` profile it correctly declines to grade
enforcement at all and says the card declares no `securityRequirements` —
against an unsecured target the probe could not fail, so it is not run.

A secured run grades `BIND-EQUIV-004` **and nothing else**: `BIND-EQUIV-001..003`
compare answers about a fixture task an anonymous client cannot create there.
Those three are graded by the ordinary unsecured run in the table above.
Scoping, not waiving — the same argument as the official suite's extension
profile, and neither run alone covers §5.1. The pair does.

Both are gated in `tck.yml`'s `tck-all-bindings` job.

One upstream discrepancy, found while reading the requirement definitions:
`a2a-tck`'s backlog ticket `task-28` summarises `BIND-EQUIV-004` as "Streaming
equivalence", while `tck/requirements/interop.py` — the file the suite actually
loads — defines it as the authentication-scheme requirement. This repo follows
`interop.py`.
