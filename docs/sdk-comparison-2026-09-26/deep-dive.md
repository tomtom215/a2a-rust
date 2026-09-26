<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Deep dive: interoperability, broken promises, and performance

This follows up on [`README.md`](README.md) §3.2, §3.6 and §3.7. It goes a
level further into three areas:

- **Interop.** Real-world paths the first matrix did not exercise, the actual
  bytes on the wire, and the other official SDKs.
- **Broken promises.** Root cause, history and test gaps for each claims
  failure, each with a verified patch.
- **Performance.** What the CPU and memory gaps actually consist of, tested
  with experiments rather than inferred.

Each result is labelled:

- **VALIDATED:** it was run, and the result file is named;
- **SOURCE-CONFIRMED:** it was read in the published source;
- **CONJECTURED:** it is inferred and not checked.

**Headline findings:**

1. **Interop.** The core paths hold under a much harder test. The failures
   that matter are in less common paths, on both sides.
   - Fourteen more cross-SDK checks were added: multi-turn, failure reasons,
     message replies, typed parts, Unicode, 256 KiB payloads, pagination,
     mid-stream cancel, concurrency and push authentication. These pass in all
     12 client × server × binding cells.
   - Two new defects are ours:
     - A streamed Message reply is preceded by a Task snapshot, which breaks a
       spec MUST.
     - Our REST client drops the error type of an AIP-193 error that carries no
       ErrorInfo.
   - Six are a2a-rs's. The most practical: its client and server turn every
     JSON integer into a float, and reject integers above 2^53.
   - One is shared: every Message reply leaves an orphan `SUBMITTED` task
     behind.
2. **The `params: null` defect is wider than a2a-rs.**
   - It fails against three of the six other official SDK servers: a2a-rs,
     Python and .NET.
   - It has shipped in every release since v0.2.0. No test sent this request
     through our client to a foreign server.
   - `params: {}` is the only form all seven SDKs accept.
3. **All six claims defects are confirmed, and one is worse than reported.**
   - Every patch was checked with a regression test that fails before the patch
     and passes after.
   - F3 turned out to be security-relevant. Its details are withheld pending a
     private advisory, per `SECURITY.md`.
4. **Performance gaps, explained:**
   - **Memory per stream:** the 2.5× figure comes almost entirely from our
     default event-queue capacity, 256 against a2a-rs's 32. At equal capacity
     the difference is 13 %.
   - **CPU gap:** it is spread thinly: more allocations, one extra spawned task
     per request, larger futures, and per-task buffer allocation. Two one-line
     changes each recover a measured 2–5 % at concurrency 16 and above. The rest
     is architectural.

---

## 1. Interop, deeper

### 1.1 `GetExtendedAgentCard` sends `"params": null`

**Root cause (SOURCE-CONFIRMED).**

- `client/src/methods/extended_card.rs:34-37` builds `serde_json::Value::Null`
  when no tenant is configured.
- `JsonRpcRequest::with_params` wraps it in `Some(..)`
  (`types/src/jsonrpc.rs:206-216`, `params: Some(params)` at line 215). The
  field is an `Option` with `skip_serializing_if = "Option::is_none"`
  (`:188-189`), so it serializes as `"params": null`. Only `None` would omit
  it.
- Both the HTTP and the WebSocket JSON-RPC transports go through
  `with_params` (`transport/jsonrpc.rs:183`, `transport/websocket.rs:1128-1130`).
- No other production call site builds a `Null` params value.

**History (SOURCE-CONFIRMED on the full-history clone).**

- The null is present in the extended-card method of **every tag from v0.2.0
  to v0.14.0**.
- `b05c9c23` (2026-09-09) added the tenant form and kept `Null` for the case
  with no tenant.

**Why no test caught it (SOURCE-CONFIRMED).**

- The in-repo TCK hand-builds `{"params": {}}` for this call
  (`tck/src/equivalence.rs:319-334`), so it never goes through our client.
- The cross-SDK interop jobs run official-SDK agents that do not configure an
  extended card, so those servers refuse before reading `params`.
- Our client was therefore only ever tested against our own lenient server.

**Blast radius (VALIDATED).** Every official SDK's server was run with an
extended card configured, and sent five `params` forms. Their clients' requests
were captured on the wire. Full table: [`params-matrix.md`](params-matrix.md).

| Server | omitted | `null` | `{}` | `[]` | what its own client sends |
|---|---|---|---|---|---|
| a2a-rust 0.14.0 | OK | OK | OK | OK | **`null`** |
| a2a-rs (server-lf 0.4.4) | -32700 | **-32700** | OK | -32700 | `{}` |
| Python a2a-sdk 1.1.5 | OK | **-32602** | OK | OK | `{}` |
| Go a2a-go v2.6.0 | -32700 | OK | OK | -32602 | `{}` |
| JS @a2a-js/sdk 1.2.1 | OK | OK | OK | OK | `{}` |
| Java 1.3.2.Final | OK | OK | OK | -32600 | `{"tenant":""}` |
| .NET 1.0.0-preview2 | -32602 | **-32602** | OK | -32602 | `{}` |

**Fix.** Send `{}` when no tenant is set. Omitting `params` is what the spec's
§9.4 example shows, but three official servers (a2a-rs, Go and .NET) reject
that form. `{}` is the only form all seven accept. A regression test should
push this call through our client to a strict server.

### 1.2 A second tier of cross-SDK checks

Both harness agents gained the same extended behaviour contract (`deep.rs`).
Both drivers gained 15 checks, D01–D15 (`driver-*/src/bin/deep_*.rs`). The
table covers 3 runs plus a final run using binaries built from this
directory; every cell gave the same result in all four
([`results/deep/interop/`](results/deep/interop/)). Column headings are
*server ← client, binding*.

| Check | ours←ours JR | ours←ours REST | ours←ours gRPC | ours←rs JR | ours←rs REST | ours←rs gRPC | rs←ours JR | rs←ours REST | rs←ours gRPC | rs←rs JR | rs←rs REST | rs←rs gRPC |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| D01 multi-turn (INPUT_REQUIRED → continue) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| D02 history holds both user turns | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| D03 FAILED carries its reason | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| D04 direct Message reply | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| D05 text/data/url/raw parts + metadata round-trip | ✓ | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ | ✗ | ✗ | ✗ |
| D06 Unicode + 256 KiB exact | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| D07 pagination (5 tasks, pageSize 2) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| D08 cancel mid-stream | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| D09 32 concurrent sends | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| D10 empty `parts` → typed `-32602` | ✓ | ✗ | ✓ | ✓ | ✗ | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| D11 push `Authorization: Bearer` delivered | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| D12 `historyLength: 1` honoured | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| D13 streamed Message reply = one Message | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| D14 subscribe to a finished task → `-32004` | ✓ | ✓ | ✓ | ✓ | ✗ | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| D15 cancel an unknown task → `-32001` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

**Harness problems found and fixed before these runs.** Both were my
asymmetries, not SDK behaviour:

- Our agent emitted `WORKING` before the new behaviours ran, so its `msg:`
  reply created a task first.
- Our card declared only `text/plain`, so our server correctly refused D05's
  JSON, PNG and octet-stream parts with `-32005`, a check a2a-rs does not
  enforce.

Both cards now declare the same four input modes, and both agents run the hook
before `WORKING`.

**Attribution of every ✗ (each VALIDATED at the wire, §1.3):**

| Cells | Owner | What happens |
|---|---|---|
| D13, our server | **a2a-rust** | For a streamed Message reply, our server first emits a `Task` snapshot in state `SUBMITTED`, then the Message. §3.1.2 says a message-only stream "MUST contain exactly one Message object". The snapshot that P15 rightly adds to task streams is also added here. |
| D10 REST, our server | **a2a-rust client** + a2a-rs client | Our server's REST 400 `INVALID_ARGUMENT` carries no ErrorInfo. That is correct: §11.6 requires ErrorInfo only for A2A-specific errors. Our REST client then gives up and returns `UnexpectedStatus 400` instead of mapping the AIP-193 `status` to InvalidParams. a2a-rs's client maps the same body to `-32603` (`parse_rest_error`, `a2a-client-lf-0.2.5/src/rest.rs`). |
| D14 REST, our server ← a2a-rs client | **a2a-rs client** | Our server correctly answers `400 UNSUPPORTED_OPERATION` with ErrorInfo. a2a-rs's `should_retry_subscribe_with_legacy_path` treats that exact code as a cue to retry the v0.3 path `/tasks/{id}/subscribe`, gets 404, and surfaces `-32603 "not found"`. The spec-mandated answer is always discarded. |
| D05, a2a-rs anywhere | **a2a-rs** | Its client and server rewrite every JSON integer in data parts and metadata as a float (`7` → `7.0`, `-3` → `-3.0`), and reject integers above 2^53 with `-32700`. The sender-side conversion is visible on the wire: `"metadata":{"n":7.0}`. Ours preserves `9007199254740993` exactly. A consumer that deserializes these fields into integer types breaks. |
| D05, a2a-rs client → our server | a2a-rs | The same conversion, applied by the client before sending; our server echoed exactly what it received. |
| D02, a2a-rs server | a2a-rs | Follow-up turns are not appended to task history. |
| D10, a2a-rs server | a2a-rs | A message with zero parts is accepted. |
| D14, a2a-rs server | a2a-rs | Subscribing to a finished task answers `TaskNotFound`, not `UnsupportedOperation` (§3.1.6). |

**Found alongside (VALIDATED, [`results/deep/`](results/deep/)):**

- **Orphan tasks, both SDKs.** Every Message reply, blocking or streaming,
  leaves a task in `SUBMITTED` that never progresses and shows up in
  `ListTasks`. For a chat-style agent that is one orphan per turn.
  - Ours ages them out with the default TTL: 1 h, capacity 10,000.
  - a2a-rs keeps them forever, since its store has no eviction.
  - The spec's Message path says "no task tracking".
- **`ListTasks` order.**
  - Ours is newest first, as §3.1.4 requires ("MUST be sorted by last update
    time in descending order").
  - a2a-rs's is oldest first. On a long-lived a2a-rs server, a new task falls
    off the first page.
- **The spec disagrees with itself on subscribe.**
  - The §11 route table says `POST /tasks/{id}:subscribe`.
  - The normative proto annotation says `GET` (`a2a.proto:78`).
  - Our client uses POST and a2a-rs's uses GET. Our server accepts both, and
    so does a2a-rs's. Worth raising upstream.

### 1.3 What each client actually sends

Every JSON-RPC and REST exchange from both drivers was captured through a
logging proxy (`harness/capture_proxy.py`). That is 601 exchanges across the
four client/server pairs. `harness/analyze_wire.py` then checked each one
against the spec. The gRPC binding was not captured. The audit output is
[`results/deep/wire/AUDIT.txt`](results/deep/wire/AUDIT.txt).

**Clients:**

- Every request carries `A2A-Version: 1.0`.
- Every JSON-RPC envelope is valid: version `"2.0"`, a string id, a v1.0
  method name.
- REST query parameters use the proto's camelCase names.
- **The only envelope violation in the capture is our `params: null`.**
- The only off-table route is a2a-rs's legacy `/tasks/{id}/subscribe`
  fallback, above.

**Servers:**

- Every JSON-RPC response is HTTP 200, echoes its id, and has exactly one of
  `result` or `error`.
- Every SSE event is a JSON-RPC response carrying the request's id.
- Both SDKs send `application/json` on REST responses instead of the SHOULD
  `application/a2a+json`. Ours does this deliberately, for a2a-go
  compatibility (`cf2a7e9`, CHANGELOG).

---

## 2. The claims failures, root-caused

The investigation that produced these reproduced each item independently of
the first audit. It traced each one to its introducing commit and wrote a
patch plus a regression test. Each test failed before its patch and passed
after, in a scratch clone at `10f3435`. The full write-up, with F3 withheld,
is [`claims-root-causes.md`](claims-root-causes.md). Patches are in
[`patches/`](patches/). I spot-checked the commit hashes and first-release
tags and the locked-in test `rest_ready_check` against the history myself.

| Item | Root cause | In every release since | Why tests missed it | Severity |
|---|---|---|---|---|
| F1 axum card: no ETag/304 | `A2aRouter` serves `axum::Json(card)` and bypasses the caching handler. The dynamic handler sets `Last-Modified = now()`. | v0.3.0 (router); the claim since v0.2.0 | Tests check status and body only; no test sends a conditional request. | Low |
| F2 REST `/ready` constant | `rest/mod.rs:120-126`. The store probe exists only on `A2aRouter` (`c86aec31`, v0.9.0). | v0.2.0; the README claim since v0.9.0 | `rest_ready_check` asserts `200` and `"ok"`, which locks the bug in. | Medium |
| F3 path tenant resolver | **Withheld (security)** | v0.3.0 | Resolver unit tests build their context by hand. | Medium, security-relevant |
| F4 signing key format | Documents SPKI DER; ring needs the raw point | v0.2.0 | Every test uses ring's raw `public_key()`. | Medium |
| P1 no private CA for JSON-RPC/REST | Transports, card discovery and the token fetcher all hard-code the default roots | v0.2.0 | TLS tests call the helpers, never a transport. | Medium |
| P2 rate-limit refusals look like server errors | Interceptors can only return `internal`; anonymous callers share one bucket | v0.3.0 | Tests check that `before()` errors, not what HTTP status results. | Medium |

**Corrections to the first audit:**

- **F2's `-32009` body** was an artefact of probes that omitted `A2A-Version`.
  The real defect is broader: `JsonRpcDispatcher` answers every `GET` with
  HTTP 200.
- **P1 is wider than reported:** card discovery and the token fetcher are also
  hard-wired.
- **F1 also lacks CORS on the card.**
- **F3 is understated** (details withheld).

**Other instances of the same bug classes** (in `claims-root-causes.md`):

- `A2aRouter` has no tenant routes and no query-length limit;
- `ClientConfig.tls` is never read;
- capacity refusals also map to `-32603`.

**Patches in this directory** (each applies alone to `10f3435`):

- `F1.diff`, `F2.diff`, `F4.diff`, `P1.diff`, `P2.diff`;
- `TESTS-public.diff`, the F1, F2 and P2 regression tests, with the F3 test
  removed.

F3's patch and the combined diff are held back with F3. None of these are
applied to the crates on this branch.

---

## 3. Performance, explained

### 3.1 Memory per open stream: the default queue capacity

Every task gets a tokio broadcast channel of `DEFAULT_QUEUE_CAPACITY = 256`
(`streaming/event_queue/mod.rs:47`). tokio allocates every slot up front.
a2a-rs uses `EXECUTION_BUFFER_CAPACITY = 32` (`a2a-server-lf-0.4.4/src/handler.rs:13`).

The run was a fresh process with 1,000 held streams, varying capacity through
the builder's existing `with_event_queue_capacity`, 2 runs each
([`results/deep/prof/streams-mem-by-capacity.txt`](results/deep/prof/streams-mem-by-capacity.txt)).
VALIDATED:

| Capacity | 256 (default) | 128 | 32 | 8 |
|---|---|---|---|---|
| KiB per held stream | 154.1 | 106.0 | 69.7 | 61.0 |

- **Cost per slot:** the fit is linear at ≈0.38 KiB per slot.
  `size_of::<StreamResponse>()` is 344 bytes
  ([`future-sizes.txt`](results/deep/prof/future-sizes.txt)), so a slot is one
  event plus bookkeeping.
- **At equal capacity:** ours is 69.7 KiB against a2a-rs's 61.8 KiB, a 13 %
  gap.
- **A trade-off, not a leak.** A larger buffer lets a slow subscriber fall
  further behind before it lags. Preallocating it for every task also costs
  CPU, per §3.2.

### 3.2 CPU per request: many small costs, not one hot spot

**Instructions.** Callgrind counted instructions over 2,000 JSON-RPC sends,
after warm-up. VALIDATED:

- a2a-rust: 421k instructions per request, 52 % of them in `memcpy`;
- a2a-rs: 242k per request, 15 % in `memcpy`.

**Where the `memcpy` comes from** (SOURCE-CONFIRMED plus VALIDATED sizes):

- Our handler future is **12,048 bytes, held inline**. a2a-rs's `async_trait`
  boxes its 4,368-byte future once.
- Per request, our server spawns a background event processor and a response
  collector. Those state machines are 5,640 and 4,160 bytes
  (`-Zprint-type-sizes`,
  [`server-async-sizes-top40.txt`](results/deep/prof/server-async-sizes-top40.txt)).
  Each is moved through tenant-scope and span wrappers into `tokio::spawn`.
- There are 3 spawns per request for us and 2 for a2a-rs.

**Sampled time** (perf 6.8.1, 15 s at concurrency 16, both servers pinned
alike; [`perf.*.txt`](results/deep/prof/)). The per-request breakdown is
CONJECTURED: each category's share of samples is multiplied by the separately
measured CPU per request (185 µs and 144 µs). The categories add up to the
measured 41 µs gap:

| Where the extra time goes | Extra µs/request |
|---|---|
| Allocator (more allocations per request) | +11 |
| SDK code | +8 |
| Broadcast-channel creation (the 256-slot preallocation) | +7 |
| Kernel scheduling and syscalls (the extra spawned task) | +6 |
| tokio | +4 |
| `memcpy` | +2 |

**Two one-line experiments, measured** (alternating A/B, 3 × 15 s per point,
CPUs pinned; [`ab.jsonl`](results/deep/prof/ab.jsonl),
[`ab_cap.jsonl`](results/deep/prof/ab_cap.jsonl)). VALIDATED:

| Change | c=1 | c=16 | c=64 |
|---|---|---|---|
| `Box::pin` the two spawned blocks ([`patches/perf-box-spawned-futures.diff`](patches/perf-box-spawned-futures.diff)); instructions −28 %, `memcpy` −54 % | +0.3 % rps (ranges overlap) | +2.9 % rps, −2.1 % CPU | +4.8 % rps, −4.7 % CPU |
| Queue capacity 256 → 32; memory −55 % per stream | +1.2 % (ranges overlap) | +4.0 % rps, −4.4 % CPU | +2.3 % rps, −2.7 % CPU |

**What the experiments show:**

- Cutting instructions by 28 % bought under 5 % of wall-clock time. Large
  `memcpy`s are cheap per byte, so instruction counts overstated them. My
  earlier inference that the copies explained the gap was wrong, and this
  measurement corrects it.
- Neither change was tested together with the other, or for regressions
  beyond these workloads.
- Most of the gap to a2a-rs remains either way. It is spread across allocation count,
  the extra background task and SDK logic. That is the cost of the
  event-processing design, and no single change closes it.

### 3.3 gRPC and `TCP_NODELAY`

**What:** `GrpcDispatcher::serve_with_listener` wraps the listener in
`BoundedIncoming` and serves it with tonic's `serve_with_incoming`. Nothing on
that path sets `TCP_NODELAY` (SOURCE-CONFIRMED). tonic's `tcp_nodelay` setting
only applies when tonic binds the socket itself.

**Effect (VALIDATED, README §3.6):** 128 → 2,640 rps at concurrency 1, with
p99 dropping from 44 ms to 0.64 ms. This was measured with a
harness-side listener that sets the option, not with a patched SDK.

**Fix:** call `set_nodelay(true)` on each accepted stream inside
`BoundedIncoming`, as our HTTP `serve()` already does.

---

## 4. Updated findings for a2a-rust

These extend README §4 (O-1 to O-12). Changes to earlier rows:

- **O-1:** now known to break against three official servers.
- **O-2:** fix location identified (§3.3).
- **O-3:** patches F1, F2 and F4 verified.
- **O-4 = P1:** patch verified.
- **O-5 = P2:** patch verified.
- **O-11:** decomposed (§3.2).
- **O-12:** explained (§3.1).

| ID | Severity | Finding |
|---|---|---|
| O-13 | Medium (spec MUST) | A streamed Message reply is preceded by a `Task` snapshot, contrary to §3.1.2. |
| O-14 | Medium | Every Message reply leaves an orphan `SUBMITTED` task, visible in `ListTasks` until TTL (a2a-rs does the same). |
| O-15 | Low | The REST client does not map AIP-193 errors without ErrorInfo (for example, 400 `INVALID_ARGUMENT`) to typed errors. |
| O-16 | Security | F3, withheld; see `SECURITY.md`. |
| O-17 | Low (performance) | The default event-queue capacity of 256 preallocates about 96 KiB per task. Lowering it to 32 measured +2–4 % throughput at concurrency 16 and above, and −55 % memory per open stream. |
| O-18 | Low (performance) | Spawned per-request futures of 4–6 KB are moved by value; boxing them gains 3–5 % at concurrency 16 and above. |

## 5. Not done

- **Upstream reports.** None of the a2a-rs or other-SDK findings were filed
  upstream.
- **The spec's subscribe inconsistency** was not raised.
- **gRPC traffic** was not captured at the wire level.
- **The two performance changes** were not combined or tested for
  regressions beyond these workloads.
- **The F2 test assertion.** One assertion in the verified F2 patch
  (`dispatch_edge_tests.rs`) compiles but was not executed in the scratch
  clone.
