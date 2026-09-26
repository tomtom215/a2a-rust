<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-rust vs a2a-rs, measured on 2026-09-26

**Purpose.** This is a self-check, not a competition. It asks three
questions:

- Does a2a-rust interoperate in real use with the official A2A Rust SDK?
- Does what a2a-rust promises actually work?
- Where does each SDK genuinely lead?

Every number below traces to a file under [`results/`](results/) and to a
command in [`harness/`](harness/) that reproduces it.

## 1. Bottom line

1. **Interop works in both directions, with one defect of ours.**
   - The matrix is both clients × both servers × JSON-RPC, HTTP+JSON and gRPC,
     with 17 checks per cell. **203 of 204 checks pass**, with and without a
     real model in the loop, and on every repeat run (§3.2).
   - The one failure is ours. Our client sends `"params": null` on
     `GetExtendedAgentCard`; JSON-RPC 2.0 forbids that, and a2a-rs's server
     rejects it. So do the Python and .NET servers
     ([`deep-dive.md`](deep-dive.md) §1.1).
   - A second tier of 15 checks covers multi-turn, typed parts, pagination,
     mid-stream cancel, concurrency and push auth. It passes on the core paths
     in all 12 cells, and it found three more defects on our side, six on
     a2a-rs's and one shared (`deep-dive.md` §1.2).
2. **Official conformance (ACTS).** The grader is the A2A project's own corpus,
   `a2a-itk` `429945f`, and each project's own ITK agent is the system under
   test.
   - **a2a-rust:** conformant on all three bindings, missing only one SHOULD.
   - **a2a-rs:** not conformant on any binding. It passes 41/55, 37/46 and
     35/48 of the graded MUSTs.
   - Some a2a-rs failures come from its ITK agent's configuration, not its
     SDK. §3.1 attributes every one of them. Twelve are confirmed as SDK
     behaviour by probes that do not involve either project's ITK agent.
3. **With a real model in the loop, the SDK choice does not show.**
   - Each SDK adds about 7 ms to time-to-first-token over calling llama-server
     directly.
   - All 24 prompts produced byte-identical text on all three paths (§3.4).
4. **a2a-rs is faster and leaner on the unary hot path.**
   - It serves about 12–30 % more requests per second at 11–22 % less server
     CPU per request (§3.6).
   - Our earlier apparent lead on streaming came from a socket default
     (`TCP_NODELAY`), not from the SDK core. Enabling the same option on the
     a2a-rs listener made it faster than ours there too.
   - Our own gRPC `serve_with_listener` lacks that option, which costs a 44 ms
     tail latency.
   - The CPU gap has no single hot spot, and the memory gap is our default
     queue capacity. Both are explained and measured in `deep-dive.md` §3.
5. **Our feature promises mostly hold.**
   - Of the 42 README feature rows, **27 are verified** end-to-end against the
     published 0.14.0 crates, **7 partial**, **4 failed** and 4 not tested
     (§3.7, [`claims-audit.md`](claims-audit.md)).
   - The four failures are real, reproducible defects in features we
     advertise. Each is root-caused, with a verified patch, in `deep-dive.md`
     §2. One of them (F3) is security-relevant, and its details are withheld
     pending a private advisory.
6. **a2a-rs leads on everything code cannot change:**
   - 10 external reverse dependencies on crates.io against our 1, a stale optional one;
   - 10 human commit authors against 1;
   - OpenSSF Scorecard 9.4;
   - an institutional home (§3.9).

## 2. Subjects, method, and what could bias it

| | a2a-rust | a2a-rs (official) |
|---|---|---|
| Crates (latest on crates.io, 2026-09-26) | `a2a-protocol-{types,client,server,sdk}` 0.14.0 | `a2a-lf` 0.3.1, `a2a-client-lf` 0.2.5, `a2a-server-lf` 0.4.4, `a2a-grpc` 0.3.7, `a2a-pb` 0.2.1 |
| Source commit (from `.cargo_vcs_info.json`) | `10f3435` | `365d056` (client, grpc); `ab76562` (lf, server, pb, unchanged at `365d056`) |
| Published == git tree | yes, 4/4 byte-identical | yes, 6/6 byte-identical |
| `.crate` sha256 | [`results/gov/checksums.txt`](results/gov/checksums.txt) | same file |

**Machine.** 4 vCPU and 15 GB RAM, running Linux 6.18 in a cloud container.
The build profile is `release`, compiled with Rust 1.94.1. The model is
Qwen3-0.6B Q8_0 (sha256 `361cc681…`), served by llama.cpp `81bc6b83f`. Full
pins are in [`harness/README.md`](harness/README.md).

**Fairness controls.**

- Both harnesses depend on the **crates.io releases only**. Each resolves its
  own lockfile, as a new user of that SDK would.
- The code that calls the model, the push-webhook receiver and the result
  reporting are **the same source files**, included by both sides.
- Both agents use the same behaviour contract, the same card capabilities and
  input/output modes, and each SDK's own push-URL guard with only the
  loopback opt-out.
- One asymmetry was found and removed mid-run. Our card initially declared no
  input modes, which silently disabled our content-type check. It now matches
  the a2a-rs card, and the probe log records the before and after.
- For conformance, each SDK is graded through **its own maintainers' ITK
  agent** under the upstream runner, so this report did not write either
  system under test.
- Load tests ran on an otherwise idle machine, with CPUs pinned and three
  repetitions. §3.6 reports the spread.

**Bias disclosure.** An AI assistant produced this report inside the a2a-rust
repository, at its maintainer's request, and wrote both harnesses. The
controls above limit that bias; they do not remove it. Treat §3.1's
attribution and §3.6's configuration as the places to check first. The
comparison is also an SDK-to-SDK one. It does not cover a2a-rs's `a2acli` or
SLIMRPC, or our WebSocket and SLIMRPC bindings.

## 3. Results

### 3.1 Official conformance: ACTS

The command was `run_acts.py --transport all`, with each project's own `itk/`
directory mounted. Raw reports and logs are in [`results/acts/`](results/acts/).

| Binding | a2a-rust: tests passed / MUST | a2a-rs: tests passed / MUST |
|---|---|---|
| JSON-RPC | 101/101 · MUST 59/59 | 72/101 · MUST 41/55 (14 failed, 4 skipped) |
| gRPC | 88/88 · MUST 47/47 | 69/88 · MUST 37/46 (9 failed, 1 skipped) |
| HTTP+JSON | 91/92 · MUST 49/49 (fails SHOULD `REST-CT-001`) | 67/92 · MUST 35/48 (13 failed, 1 skipped) |
| Verdict | CONFORMANT | NOT CONFORMANT |

**Repeatability.** Each SDK was run 3 times. The scores and the exact set of failing (binding, test) pairs were identical in all 3 runs: 66 pairs for a2a-rs and 1 for a2a-rust (`results/acts/*-rep{2,3}`).

**Attribution of the 25 a2a-rs failures on JSON-RPC.** The gRPC and REST
failures are a subset of these plus the REST-only rows.

| Class | Tests | Evidence |
|---|---|---|
| **SDK behaviour, confirmed by a neutral probe** (§3.3), 12 tests | `CARD-CACHE-001` (P01), `JSONRPC-ERR-002` (P02), `CORE-ERR-006` (P03), `CORE-SEND-002` and `CORE-ERR-004` (P04), `CORE-MULTI-003` and `CORE-MULTI-006` (P05), `CORE-LIST-003` (P06), `VER-NEG-001` (P07), `CORE-HIST-003` (P11), `STREAM-SUB-003` (P12), `CORE-SEND-004` (P14) | The same failure reproduces on the neutral bench agent, whose configuration matches ours. |
| **Likely SDK behaviour, not probed**, 5 tests | `VER-NEG-002` (a missing `A2A-Version` is not treated as 0.3; P07 shows the header is not checked at all), `CORE-HIST-005` and `CORE-HIST-006` (follow-up turns are not appended to history), `PUSH-ERR-001` (error type), `CORE-ERR-009` (SHOULD) | ACTS messages and source reading only |
| **The ITK agent's configuration, not the SDK**, 7 tests | `PUSH-DELIV-001..003` and `SEC-PUSH-001/002`: the agent enables push through `with_push_config_store`, whose default sender blocks loopback URLs as SSRF protection, so the ACTS webhook at 127.0.0.1 is never called. `DM-SERIAL-001`: the agent emits `timestamp: None`. `CORE-CAP-002`: the agent does not pass its capabilities to the handler (its own comment calls the setting inert); whether 0.4.4 would enforce a declared `streaming: false` was not tested, so this row is CONJECTURED. | Our interop matrix shows a2a-rs push delivery **working** once the guard is opted out (C13, §3.2). |
| **Not attributed**, 1 test | `JSONRPC-ERR-003` ("error.message missing") | The neutral probe P08 sees `message` and `ErrorInfo` present. |

REST adds `SEC-EXTCARD-001/002/004`, where the ITK wrapper serves the
extended card without authentication; that is agent configuration.

Our one gap, `REST-CT-001` (a SHOULD), is shared: neither SDK sends
`application/a2a+json` on REST responses.

Our ITK agent does not compensate for SDK gaps. The neutral agent built on
the same SDK passes the same probes (§3.3).

### 3.2 Cross-SDK interop matrix

The matrix is {a2a-rust, a2a-rs} client × {a2a-rust, a2a-rs} server ×
{JSON-RPC, HTTP+JSON, gRPC}: 12 cells, each running checks C01–C17. The
checks are:

- card discovery and binding selection;
- blocking send;
- streaming, with appended artifact chunks reassembled and `lastChunk` seen;
- `GetTask` returning the stored artifact, which must equal the streamed text;
- `ListTasks`;
- non-blocking send;
- push config create, get, list and delete;
- `SubscribeToTask`, with the first event checked;
- cancel, and the subscription observing `CANCELED` and closing;
- **push delivery to a real webhook**;
- `GetExtendedAgentCard`;
- typed errors for `TaskNotFound` and `TaskNotCancelable`.

| Mode | Runs | Result per run |
|---|---|---|
| echo executor | 3 | 203/204 |
| Qwen3-0.6B via llama.cpp (streamed tokens) | 2 | 203/204 |

The single failing cell is identical in every run: **our client → a2a-rs
server, JSON-RPC, C15**. Our client sends `{"method":"GetExtendedAgentCard",
"params":null}`. JSON-RPC 2.0 §4.2 requires `params`, when present, to be an
Array or an Object, and the A2A spec's own example (§9.4) omits it entirely.
a2a-rs rejects both of those forms: the spec's canonical form and our `null`.
Only `{}` works (probe P13). Our server accepts all three.

**What this establishes.** Messages, streams, task state, push delivery and
typed errors all cross the SDK boundary correctly in both directions, over
all three bindings, with a real model producing the content.

### 3.3 Raw-wire spec probes

Each probe is a raw HTTP request sent identically to both neutral agents
([`harness/probe.py`](harness/probe.py)). The table is from runs 3 and 4, which gave identical results. Runs 1 and 2 predate P15 (and, for run 1, the card fix), and agree on P01–P14.

| Probe | Spec level | a2a-rust | a2a-rs |
|---|---|---|---|
| P01 agent card carries `Cache-Control` or `ETag` | SHOULD | pass | fail |
| P02 invalid JSON → HTTP 200 + `-32700` | MUST | pass | fail (HTTP 400, plain text) |
| P03 missing `method` → `-32600` | MUST | pass | fail (HTTP 422, plain text) |
| P04 message to a terminal task is rejected | MUST | pass | fail (accepted) |
| P05 `taskId` from another `contextId` is rejected | MUST | pass | fail (accepted) |
| P06 `ListTasks` always has `nextPageToken` | MUST | pass | fail (omitted) |
| P07 `A2A-Version: 99.0` → `-32009` | MUST | pass | fail (processed) |
| P08 A2A error carries `message` + `ErrorInfo` | MUST | pass | pass |
| P09 task status has a timestamp | MUST | pass | pass |
| P10 REST `Content-Type: application/a2a+json` | SHOULD | **fail** | fail |
| P11 `historyLength: 0` → no history | SHOULD | pass | fail (1 entry) |
| P12 subscribe to a terminal task → `-32004` | MUST | pass | fail (`-32001`) |
| P13 `GetExtendedAgentCard` with `params` omitted (spec §9.4 example) | spec example | pass | fail (`-32700`) |
| P14 undeclared media type → `-32005` | MUST | pass ¹ | fail (accepted) |
| P15 `SendStreamingMessage` stream begins with a `Task` (§3.1.2) | MUST | pass | fail (begins with `statusUpdate`) |

¹ P14 failed on our side while our card declared no input modes, which is the
mid-run asymmetry noted in §2. An empty `defaultInputModes`, which
`AgentCard::new` produces, turns the check off silently. Finding O-8 in §4.

P15 matters in practice. In these runs a2a-rs's stream began with whatever
the executor emitted first. An executor written like a2a-rs's own `EchoExecutor` example,
which emits `WORKING` first, therefore violates the spec without knowing it.
Our server inserts the `Task` snapshot itself.

### 3.4 Real model in the loop

[`harness/ttft.py`](harness/ttft.py) ran 24 prompts on the idle machine. The
path order rotated every iteration and llama's prompt cache was off. Results
are in [`results/ttft.jsonl`](results/ttft.jsonl).

| Path | TTFT median [min–max] | Total median [min–max] |
|---|---|---|
| llama-server direct | 128 ms [114–334] | 343 ms [205–527] |
| through a2a-rust (JSON-RPC stream) | 135 ms [126–159] | 333 ms [214–440] |
| through a2a-rs (JSON-RPC stream) | 136 ms [127–207] | 342 ms [235–424] |

All three paths gave byte-identical text for 24 of 24 prompts. The SDKs
cannot be told apart at this scale. Each costs about 7 ms at the median
before the first token, and total time is within run-to-run noise.

### 3.5 Robustness battery

[`harness/robust.py`](harness/robust.py) ran each server alone in echo mode.
Results are in [`results/robust/`](results/robust/). Both servers stayed
alive through every probe, and a normal request was served after each one.

| Probe | a2a-rust | a2a-rs |
|---|---|---|
| Body limit (default) | 4 MiB, configurable | 10 MiB (`MAX_REQUEST_BODY_BYTES`) |
| Oversized body | JSON-RPC `-32700 "request body too large"` in HTTP 200, before reading the body | HTTP `413` |
| JSON nested 100k deep / invalid UTF-8 | HTTP 200 + `-32700` (JSON-RPC-conformant) | HTTP 400, plain text |
| 200 slow-header (slowloris) sockets for 15 s; 200 idle sockets for 40 s | none closed by the server; normal requests still served | none closed by the server; normal requests still served |
| 1,000 concurrent open streams | all served; RSS 23 → 169 MB ² | all served; RSS 84 → 114 MB ² |
| 20,000 completed tasks | +25.8 MB (1.3 KiB/task) | +25.0 MB (1.25 KiB/task) |

² Measured in one process after the earlier probes, so these deltas are confounded. The isolated measurement below settles it.

Isolated (fresh process, warm-up request, then 1,000 held streams; 2 runs each, [`results/robust/streams-mem.jsonl`](results/robust/streams-mem.jsonl)):

- **a2a-rust:** 154.0 and 154.4 KiB per held stream.
- **a2a-rs:** 61.8 and 61.7 KiB per held stream.

That is 2.5× more memory per stream for ours. The cause was not investigated.

Neither SDK closed an idle or slow-header connection within those windows;
both leave header-read timeouts to a reverse proxy. Ours has a 30 s
*body*-read timeout (`DispatchConfig::body_read_timeout`), which these
probes did not reach.

### 3.6 Performance (SDK overhead, echo executor)

[`harness/perf.sh`](harness/perf.sh) ran oha 1.10.0 for HTTP and ghz 0.121.0
for gRPC, for 20 s per run after a 3 s warm-up, 3 runs per point. Servers
were pinned to CPUs 0–1 and load generators to CPUs 2–3. Figures are medians
[min–max] from [`results/perf/summary.jsonl`](results/perf/summary.jsonl).
Every HTTP response was 200. The only gRPC non-`OK` results were `Unavailable`,
at or below the concurrency level per run, which is consistent with calls
still in flight when ghz's deadline cut them. Server CPU per request
comes from `/proc/<pid>/stat`.

| Workload | Concurrency | a2a-rust rps | a2a-rs rps | a2a-rust CPU µs/req | a2a-rs CPU µs/req |
|---|---|---|---|---|---|
| JSON-RPC `SendMessage` | 1 | 6,448 [6,431–6,505] | 7,589 [7,514–7,626] | 141 | 122 |
| | 16 | 8,578 [8,577–8,630] | 11,110 [10,414–11,342] | 185 | 144 |
| | 64 | 8,362 [8,282–8,450] | 9,673 [9,536–9,738] | 193 | 160 |
| REST `message:send` | 1 | 6,850 [6,553–6,914] | 7,842 [7,804–7,884] | 133 | 118 |
| | 16 | 9,151 [8,964–9,293] | 11,568 [11,444–11,620] | 172 | 138 |
| | 64 | 8,873 [8,827–8,931] | 9,921 [9,801–9,956] | 180 | 155 |
| JSON-RPC `SendStreamingMessage` | 1 | 6,076 [5,895–6,279] | **23** [23–23] | 177 | 263 |
| | 16 | 8,057 [8,027–8,101] | 365 [361–365] | 214 | 162 |
| | 64 | 7,585 [7,541–7,620] | 1,445 [1,435–1,451] | 227 | 162 |

**The streaming gap is a socket option, not the SDK core.** a2a-rs's p50 was
exactly 44 ms at every concurrency, which is the signature of Nagle's
algorithm interacting with delayed ACKs. With `TCP_NODELAY` switched on at
the listener (`NODELAY=1`, using the pattern axum documents), the same a2a-rs
server reached **7,009 rps at c=1 and 10,267 rps at c=16**, faster than ours
([`results/perf/stream-nodelay.txt`](results/perf/stream-nodelay.txt)).

Our `serve()` sets the option. a2a-rs's own `helloworld` example serves
with plain `axum::serve`, which leaves it off, so a user who copies it gets
the 44 ms stall on every short stream. With a real model the stall
does not appear (§3.4), because tokens arrive slower than ACKs.

The 20 s gRPC runs in `summary.jsonl` show the same shape at c=16 (a2a-rust
2,081 rps, a2a-rs 4,348 rps, p99 about 45 ms for both).

**gRPC shows the same effect on both SDKs** (8 s runs, one repetition,
[`results/perf/grpc-nodelay.txt`](results/perf/grpc-nodelay.txt)):

| Concurrency | a2a-rust default | a2a-rust NODELAY | a2a-rs default | a2a-rs NODELAY |
|---|---|---|---|---|
| 1 | 128 rps, p99 44.1 ms | 2,640 rps, p99 0.64 ms | 129 rps, p99 43.9 ms | 3,180 rps, p99 0.50 ms |
| 16 | 2,701 rps, p99 44.9 ms | 8,217 rps, p99 4.5 ms | 4,457 rps, p99 44.4 ms | 9,789 rps, p99 3.7 ms |
| 64 | 8,263 rps | 8,287 rps | 8,672 rps | 9,557 rps |

Our `GrpcDispatcher::serve_with_listener` does not set `TCP_NODELAY`, even
though our HTTP `serve()` does. That is finding O-2 in §4.

**Memory under sustained load.** After a 20 s unary run (up to about 230k
requests), a2a-rs held 270–580 MB against our 60–100 MB. This is consistent
with retention, not per-request cost, because R08 in §3.5 measured almost
the same memory per task on both:

- a2a-rs's `InMemoryTaskStore` has no capacity, TTL or eviction, so memory
  grows without bound under traffic.
- Ours defaults to 10,000 tasks with a 1-hour TTL
  (`TaskStoreConfig::default`).

### 3.7 Do our feature promises hold?

An independent consumer project was built against the published 0.14.0
crates only. It tested every README "Features" row over real sockets,
following our own docs. The full table is in
[`claims-audit.md`](claims-audit.md), the tests are in
[`claims-suite/`](claims-suite/), and the outputs are in
[`results/claims/raw/`](results/claims/raw/).

**27 VERIFIED, 7 PARTIAL, 4 FAILED, 4 NOT TESTED** (42 rows; row #11, signing, is counted as FAILED because its documented verification path fails). Each FAILED test was run
four times with the same result. The cause of each was then confirmed by
reading the published source:

| # | Defect | Source |
|---|---|---|
| F1 | The `A2aRouter` (axum) agent card sends no `ETag`/`Last-Modified` and never returns `304`. The README claims all three for "agent card endpoints". | `server/src/dispatch/axum_adapter.rs:518-523` returns `axum::Json(card)` |
| F2 | `RestDispatcher` `/ready` returns a constant `200 {"status":"ok"}` and does not probe the task store. `JsonRpcDispatcher` answers `GET /ready` with **HTTP 200** and a JSON-RPC error body, so an HTTP-status probe passes on an endpoint that does not exist. | `server/src/dispatch/rest/mod.rs:120-126` |
| F3 | `PathSegmentTenantResolver` can never resolve over HTTP. It reads a `:path` header that only the WebSocket dispatcher inserts. | `server/src/tenant_resolver.rs:315-318` vs `dispatch/websocket.rs:673` |
| F4 | `verify_agent_card` rejects the key format its rustdoc specifies (DER `SubjectPublicKeyInfo`). The code hands the bytes to ring's `ECDSA_P256_SHA256_FIXED`, which expects the raw 65-byte point. Our own unit test passes the raw point, so the documented path was never exercised. | `types/src/signing.rs:450` vs `:480-481` |

PARTIAL items worth acting on:

- **TLS.** A JSON-RPC or REST client cannot be made to trust a private CA:
  the documented builder method does not exist. a2a-rs's client can.
- **Rate limiting.** Refusals come back as `-32603` inside HTTP 200, or as
  **HTTP 500** on REST, with no 429 and no `Retry-After`. All
  unauthenticated callers share one bucket.
- **README overstatements.**
  - `ServeReport` "names" tasks, but it only counts them.
  - Retries on 502 and 504 apply to reads, not sends.
- **Hot-reloaded cards.** They cannot be served by any built-in dispatcher.
- **Task TTL.** Expired tasks are evicted on the next write, not "on access",
  despite the book.
- **Tenancy.** A tenant resolver combined with the default store silently
  gives no isolation.

### 3.8 Static and quality measurements

Summarised from [`quality.md`](quality.md); raw outputs are in
[`results/quality/`](results/quality/).

| | a2a-rust | a2a-rs |
|---|---|---|
| Own test suite (default features) | 4,132 pass, 0 fail, 112 ignored | 661 pass, 0 fail |
| Library `src/` code lines | 76,501 | 17,510 (+1,736 generated) |
| Deps, client+server+types (default / all features) | 69 / 207 | 153 / 161 |
| RustSec advisories reachable from the libraries | none | RUSTSEC-2026-0285, via `a2a-slimrpc` only |
| `missing_docs` warnings | 0 | 498 |
| Declared MSRV holds (`--locked`) | 1.88: yes | 1.85: only `a2a-lf` |
| `cargo-semver-checks` gate in CI | yes | not found |

### 3.9 Adoption and governance

These were read on 2026-09-26 from the crates.io API, full-history clones of
both `main` branches, and the OpenSSF Scorecard API. Raw data is in
[`results/gov/`](results/gov/).

| | a2a-rust | a2a-rs |
|---|---|---|
| Home | personal repository | `a2aproject` (Linux Foundation); listed as the Rust SDK at a2a-protocol.org/latest/sdk (checked 2026-09-26) |
| Downloads, last 90 days (types / server / client crate) | 53,931 / 2,359 / 2,335 (umbrella `a2a-protocol-sdk`: 770) | 49,045 / 29,676 / 28,495 |
| External reverse dependencies on crates.io | 1: `adk-server` 2.2.0, optional, pinned `^0.5` | 10: `agentplane`, `agntcy-agentbridge-cli`, `agntcy-shadi-{a2a,cli,mas}`, `everruns-platform`, `sapphire-agent`, `sapphire-agent-server`, `stateknot-integrations`, `styrene-a2a` |
| Commits on `main` / human commit authors | 1,430 / 1 (479 authored as `Claude`) | 229 / 10 (plus 2 bots) |
| Non-merge commits with `Signed-off-by` | 793 of 1,299 | 143 of 206 |
| OpenSSF Scorecard | not scored (the API returns 404) | 9.4 |

Our `a2a-protocol-types` shows 53,931 recent downloads, far above our other
crates. With only one small dependent, this is probably automated traffic
rather than use. That is CONJECTURED; this report did not investigate it.

## 4. Findings for a2a-rust, in priority order

The deep dive adds O-13 to O-18 and updates several rows below
([`deep-dive.md`](deep-dive.md) §4).

| ID | Severity | Finding | Where |
|---|---|---|---|
| O-1 | **High (interop)** | The client sends `"params": null` on `GetExtendedAgentCard`, which the official SDK's server rejects. Omit `params`. | `client/src/methods/extended_card.rs` |
| O-2 | Medium (performance) | The gRPC `serve_with_listener` lacks `TCP_NODELAY`: p99 of 44 ms, and 20× lower throughput at c=1 | `server/src/dispatch/grpc/dispatcher.rs` |
| O-3 | Medium | F1–F4 above: axum card caching, REST `/ready`, the path tenant resolver, and the signing key format | §3.7 |
| O-4 | Medium | The JSON-RPC/REST client cannot trust a private CA; the rustdoc describes an API that does not exist | `client/src/tls.rs:15-16` |
| O-5 | Medium | Rate-limit refusals appear as internal errors (`-32603` / HTTP 500) with no `429`/`Retry-After`, and anonymous callers share one bucket | `server/src/rate_limit/` |
| O-6 | Low (docs) | README overstatements: `ServeReport` "names" tasks, retries on 502 and 504 for sends, "per-caller" limiting, card caching on every endpoint, "no manual `Pin<Box<dyn Future>>`" (the macro cannot reach `self`, so any stateful executor writes the future by hand) | `README.md` |
| O-7 | Low (docs) | Documentation errors: `RequestHandlerBuilder::metrics` should be `with_metrics`; the push chapter never mentions `allow_private_urls()`; "on access" should be "on write" for eviction | [`claims-audit.md`](claims-audit.md) |
| O-8 | Low | `AgentCard::new` leaves the REQUIRED `defaultInputModes`/`defaultOutputModes` empty (`a2a.proto:382-384`), which silently switches off content-type enforcement | `types/src/agent_card/builders.rs` |
| O-9 | Low (SHOULD) | REST responses use `application/json`, not `application/a2a+json`; a2a-rs does the same | ACTS `REST-CT-001` |
| O-10 | Low | An oversized body is reported as `-32700 Parse error`; `-32600` fits better | `server/src/dispatch/jsonrpc/` |
| O-11 | Info | The unary hot path costs 13–28 % more server CPU per request than a2a-rs (the inverse of a2a-rs's 11–22 % saving); not profiled | §3.6 |
| O-12 | Low | Each held stream costs 154 KiB against a2a-rs's 62 KiB (2.5×), measured in isolation. Not profiled; per-task event queues and the event log are guesses, not findings. | §3.5 |

## 5. What the official SDK could take from this, if its maintainers want it

These are observations, not requests. None was filed upstream.

- **Spec conformance.** The MUST-level gaps in §3.3 (P02–P07, P12, P14, P15)
  all reproduce without either project's ITK agent.
- **Rejecting the spec's own example.** `GetExtendedAgentCard` with `params`
  omitted is refused, and with `-32700 Parse error`.
- **Unbounded memory.** `InMemoryTaskStore` has no eviction.
- **Example configuration.** The `helloworld` example serves without
  `TCP_NODELAY`, which causes a 44 ms stall per short stream.
- **MSRV.** The declared 1.85 does not hold for client, server or grpc
  (see [`quality.md`](quality.md) for the upstream cause).
- **ITK push configuration.** The ITK agent's push setup blocks the ACTS
  webhook, so its push score understates the SDK.

## 6. Limitations

- **Hardware and runs.** One machine, a shared-tenancy cloud VM. The load
  results come from 3 runs per point; the `TCP_NODELAY` follow-ups come
  from 1.
- **Echo isolates the SDK, not the workload.** Echo measures SDK overhead;
  real agents are dominated by their executor (§3.4).
- **Model.** A 0.6B model tests the integration, not answer quality.
- **Scope.** WebSocket, SLIMRPC, `a2acli`, PostgreSQL throughput and
  multi-node deployment were out of scope.
- **The claims audit was run by a delegated agent.** Its four FAILED rows
  were re-confirmed from the published source here; the other rows rest on
  its test runs, which are committed and rerunnable.
- **ACTS and its agents move.** ACTS and the ITK agents change on `main`;
  re-run at the pinned commits to reproduce.

## 7. Reproduce

The harness prerequisites and pins are in [`harness/README.md`](harness/README.md).
Then:

```sh
BENCH=/opt/bench RESULTS=/tmp/results REPS=3 docs/sdk-comparison-2026-09-26/harness/run_all.sh
```

To rerun the claims suite: `cd docs/sdk-comparison-2026-09-26/claims-suite/suite && cargo test -- --nocapture --test-threads 1`.
