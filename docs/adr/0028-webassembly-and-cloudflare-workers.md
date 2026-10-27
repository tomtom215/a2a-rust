<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0028: WebAssembly, and Cloudflare Workers as a deployment target

**Date:** 2026-10-26
**Status:** Proposed. Evidence gathered and an option recommended; nothing is
landed in the crates. The decision it asks for (qualifying ADR 0003) is the
maintainer's.
**Author:** Claude Code, for Tom F. to decide

---

## Context

The request: run separate fan-out agents (small agents, each deployed on its
own, started on demand) on Cloudflare Workers, without taking the path of
least resistance, which would be a Worker that forwards to an agent running
somewhere else.

Two accepted ADRs bear on it. ADR 0003 makes Tokio "the mandatory async
runtime". ADR 0002 lists "cross-compilation to `musl` and `wasm32` is
possible" as a consequence of the types crate's small dependency set. ADR 0013
records that the OTLP exporter cannot run on a WebAssembly target at all.

A Worker is a V8 isolate. The properties that matter here, each tested below
rather than assumed:

1. One thread, no sockets. Outbound HTTP is `fetch`.
2. `std::time::Instant::now` and `SystemTime::now` panic.
3. No Tokio reactor or timer driver.
4. I/O and timers belong to the *request's* context. When the response is
   sent the context ends, and a timer armed in it is dropped, unless the
   request holds it open with `ctx.wait_until`.
5. JavaScript handles are not `Send`.

Everything below was run in `workerd` through `wrangler dev --local`. Nothing
was deployed to Cloudflare's network. The numbers are in
[`0028-wasm-spike/results.txt`](0028-wasm-spike/results.txt) and regenerate
with [`reproduce.sh`](0028-wasm-spike/reproduce.sh).

## What was measured

### Does it build for `wasm32-unknown-unknown`? (HEAD `6a82edc`)

| Crate | Result at HEAD |
|---|---|
| `a2a-protocol-types` | Builds. |
| `a2a-protocol-types` + `signing` | Fails in `getrandom`. Builds once the *application* adds `getrandom` 0.2 `js` and `ring` `wasm32_unknown_unknown_js`; without the second, `SystemRandom` is not a `SecureRandom`. |
| `a2a-protocol-server` | Fails: `mio` (48 errors, "unsupported by mio. If using Tokio, disable the net feature") and `uuid` (1). |
| `a2a-protocol-client` | Fails: `mio` (48), `getrandom` (1). Still fails with the patch below: `hyper-rustls`, its default TLS connector, enables `hyper-util`'s `client-legacy` feature, which enables `tokio/net`, and the client's own HTTP transport is `hyper_util::client::legacy`. |

### Does what builds, run? Primitives on `workerd`

| Probe | Result |
|---|---|
| `tokio::sync` (oneshot, mpsc, broadcast, `Mutex`, `Semaphore`) | Works. |
| `std::time::Instant::now` | Panics: "time not implemented on this platform". |
| `tokio::time::sleep` | Panics, same message (it reads the clock). |
| `tokio::spawn` | Panics: "there is no reactor running". |
| `wasmtimer` sleep + timeout | Works, **once** (finding 2). |
| `tokio_with_wasm` spawn + timeout | Works. Its futures are `!Send` (finding 3). |
| `fetch` inside a `Send` future via `worker::send::SendFuture` | Works. |
| **`a2a-protocol-types` itself:** `TaskStatus::with_timestamp` | **Panics at HEAD** (`SystemTime::now`, `unsupported.rs:35`). Returns a timestamp once the types crate reads `web_time`. |

The last row is the reason a green `cargo check` is not evidence here: the
types crate "builds for wasm32" and traps on its first timestamp. ADR 0002's
sentence was true of the compiler and false of the program.

### The real `RequestHandler`, in `workerd`

With the patch below, `a2a-protocol-server` builds for wasm32 and
`RequestHandler` runs inside `workerd`. [`battery.py`](0028-wasm-spike/battery.py)
asserts, against a Worker that calls the handler directly:

| Case | Result |
|---|---|
| Blocking `SendMessage` | Completes, artifact returned. |
| `GetTask` in a later request | Same task, completed. |
| Executor that awaits an 80 ms timer, three requests in a row | Each completes in 91–92 ms. |
| Executor that never returns, `executor_timeout` 400 ms | Task `FAILED` in 412 ms. |
| `CancelTask` on a running executor | `CANCELED`, and `GetTask` agrees. |
| `SendStreamingMessage` | SSE: task, status, artifact, status. |
| 30 concurrent sends | 30 completed, 30 distinct task ids. |
| Non-blocking send whose executor needs a timer, with `ctx.wait_until(handler.wait_idle())` | Completes. **Without it: stays `WORKING` for good** (finding 4). |
| ES256 content signing (ADR 0027): sign, verify, verify a tampered copy | Verifies; tampered copy refused. |

11 of 11 pass on a clean checkout, with zero panics in the `workerd` log. The
Worker with signing is 1,784,549 bytes of wasm in the final run (615,897
gzipped; builds from other checkout paths differed by a few KB, because panic
locations are embedded). Cloudflare's limit is 64 MiB uncompressed on every plan
(the compressed limits were removed on 2026-09-04). Its startup limit, 1 second,
was **not** measured.

The patch moves no native behaviour: `cargo test -p a2a-protocol-server --lib`
gives 1,545 passed, 0 failed, 1 ignored on both the patched tree and HEAD, and
the per-test name-and-outcome lists are identical. The server's integration
test binaries were not run.

## Findings that compiling would not have shown

1. **`TaskTracker::spawn` needs a Tokio reactor.** It calls `tokio::spawn`
   itself. The handler has seven such sites; the first request panicked in one
   of them ("there is no reactor running"). Fix: spawn through the runtime
   seam with `tracker.track_future(f)`, which is what `TaskTracker::spawn`
   does.
2. **`wasmtimer` works for exactly one request.** It runs one global timer
   chain, armed by whichever request first uses a timer; `workerd` drops a
   request's timers when it ends, so every later request's timers never fire.
   The sequence `slow, slow, hello, slow, slow` completed 2 of 5 with
   `wasmtimer` (the first `slow` and `hello`; the other three hung) and 5 of 5
   after the fix. VALIDATED by that before/after on identical
   input.
3. **`tokio_with_wasm`'s timers are `!Send`**, which every trait in this crate
   forbids (`Pin<Box<dyn Future + Send>>`): the server failed to compile with
   them ("future cannot be sent between threads safely", first at
   `handler/messaging/execute.rs`). The patch carries a small `Send`
   timer instead (`rt/timers.rs`, 176 lines): one `setTimeout` per sleep,
   armed by the polling request and cleared on drop, the JS closure parked in a
   thread-local so the future holds only an id and an `Arc`.
4. **Work that outlives its response stalls.** A non-blocking send's executor
   that waits on a timer never resumes once the response is sent. Fixed by
   `ctx.wait_until(handler.wait_idle())`, registered *after* the send returns
   (registered before, the handler looks idle and nothing is held). My first
   A/B of this passed by accident, because an orphaned executor from the
   control arm kept the handler non-idle; the numbers above are one request
   per fresh isolate, and the battery refuses the case unless the handler is
   idle first.
5. **The trap APIs can be linted.** Forbidding `std::time::Instant::now`,
   `SystemTime::now`, `tokio::spawn`, `spawn_blocking`, `TaskTracker::spawn`,
   `tokio::time::{sleep, timeout, timeout_at, interval}` and
   `Handle::try_current` through clippy's `disallowed-methods` fires 101 times
   on the native build, so the rule is live and those are the sites the seam
   replaced. On the wasm32 build of the finished patch it fires 0 times, and
   `reproduce.sh` asserts that. On the patch as first written it fired once, on
   a `spawn_blocking` in `agent_card/hot_reload.rs`; that helper's callers were
   already compiled out for wasm32, so it was dead code there and the hit was
   harmless. What is established is that the rule detects the APIs and that the
   finished wasm32 build is free of them; that it would catch a *reachable*
   regression is the intent of the gate, not something demonstrated here.
6. **The dispatchers cannot be called from a Worker.** `JsonRpcDispatcher`,
   `RestDispatcher` and `CombinedDispatcher` take `hyper::Request<Incoming>`;
   every `Incoming` constructor in hyper 1.12.0 is `pub(crate)` or private. The
   test Worker therefore bypasses the dispatch layer (14,439 lines in 39
   files at HEAD: auth interceptors, rate limiting, card caching, SSE framing)
   and calls `RequestHandler` from a JSON-RPC front written for the purpose (the
   Worker's `fetch` function, 84 lines). **None of that layer is exercised.**
7. **A Worker crate cannot be a member of this workspace.** The workspace's
   release profile sets `strip = true` (`Cargo.toml:104`); with it
   `worker-build` fails ("externref table required for catch wrappers"). Without
   it, exit 0, same crate. Worker crates live outside the workspace.

Also observed: in `tracing`/`otel` default features the handler runs without a
provider (no-op), but the `otlp` feature needs sockets and a Tokio runtime and
is native-only, as ADR 0013 says; a Worker's telemetry is Cloudflare's, not
this crate's exporter.

## What the patch does

[`server-wasm32.patch`](0028-wasm-spike/server-wasm32.patch) (147 files,
+1,024 −596 including `Cargo.lock`; applies cleanly to `6a82edc`):

- `crate::rt`: `spawn`, `spawn_tracked`, `time::{sleep, timeout, timeout_at,
  interval, Instant, SystemTime}`. Native: Tokio's, unchanged. wasm32:
  `wasm_bindgen_futures::spawn_local` (through `tokio_with_wasm`), the `Send`
  timers, `web_time`.
- The types crate reads `web_time` on wasm32 (three sites).
- Native-only items gated off on wasm32: `serve`, the file watchers, the
  bundled `HttpPushSender` (a custom `PushSender` over `fetch` is the
  replacement). The two `Handle::try_current` calls answer for a target with
  no runtime.
- `RequestHandler::wait_idle` (non-cancelling; `finish_in_flight` cancels
  after its window and fires the shutdown token for later tasks, which would
  poison every following request).
- Tokio's `net` and `hyper-util`'s `client` and `client-legacy` dropped from
  the workspace dependency lines, so a wasm build can leave them out; the
  server names them back for native targets, and so does the client. **The
  first version of the patch missed the client**: with default features
  `hyper-rustls` re-enabled `client-legacy` and everything built, and
  `cargo check -p a2a-protocol-client --no-default-features` failed ("cannot
  find `client` in `hyper_util`") where HEAD succeeds. Found only by building
  each crate on its own both ways; `reproduce.sh` now does.

## Options

| | A. No WASM; a Worker fronts a native agent | B. Runtime seam in the existing crates (spiked) | C. Extract a runtime-agnostic core crate | D. Separate Worker-native crate on `a2a-protocol-types` |
|---|---|---|---|---|
| Agent logic runs in the isolate | No | Yes (measured) | Yes (not built) | Yes, a reimplementation |
| One protocol implementation | Yes | Yes | Yes | **No**: idempotency, halts, budgets, provenance, retention and the rest would be written again and drift |
| Measured cost | none | 147 files, +1,024 −596, 1,545 native tests unchanged | not measured | not measured |
| Largest unknown | Cold start and a second deployment to operate (neither measured) | The dispatch seam (finding 6); durable stores; the client | Whether a boundary between "logic" and "runtime" exists in 292 files without breaking public API | Parity with the server, forever |
| Contradicts ADR 0003 | No | Qualifies it | Qualifies it | No |

A is a legitimate deployment and needs no decision from this ADR; it is the
path the request asked not to stop at. D is rejected: the server's value is the
semantics, and a second copy of them is the cost this repository's gates exist
to prevent. C is the cleaner end state and is not evidenced; B is evidenced and
is the road to C if C is wanted, since the seam B introduces is the boundary C
would draw.

## Decision (proposed)

Pursue **B**, in phases, each with a test that decides whether to go on.
Nothing below lands until the maintainer qualifies ADR 0003.

**Amendment to ADR 0003 (proposed text).** Tokio is the mandatory runtime on
native targets. On `wasm32-unknown-unknown` the crates assume no Tokio runtime:
spawning, timers and clocks go through `crate::rt`, and Tokio is used there
only for `sync` and `macros`.

| Phase | Lands | Done when |
|---|---|---|
| 1 | The `rt` seam, native-neutral; a CI job: `cargo check --target wasm32-unknown-unknown` for types and server, plus the trap-API lint (`clippy-wasm-traps.toml`) with 0 hits | Native lib suite identical to before, as measured; lint 0 on wasm32 |
| 2 | An entry point beside `Dispatcher` that takes `http::Request<Bytes>` rather than `Incoming`, so the dispatch layer runs in a Worker | The battery passes through the dispatchers, with an auth interceptor and rate limit in the path |
| 3 | The client: hyper's transport behind a default feature, a `Transport` over `fetch` (`with_custom_transport` already exists), the clock and spawn sites on `rt` | A Worker fans a task out to two other agents and collects both artifacts |
| 4 | Durable stores: a separate crate with `TaskStore` and `PushConfigStore` over Durable Objects or D1, wrapping JS futures in `SendFuture` | State survives isolate eviction; two concurrent requests to one task are serialised |

Stop and reconsider C if, during Phase 1, the lint hits recur in review: that
would mean the seam is being routed around, which is the dual-runtime cost
this option bets against.

## Not established

- **Deployment.** Everything ran on `workerd` locally. Cloudflare's per-request
  CPU limit, memory limit, isolate eviction and the 1-second startup limit were
  not exercised. Startup of a 1.8 MB module is the first thing to measure.
- **Durable state.** The spike's stores are in memory and vanish with the
  isolate. `SendFuture` makes a `fetch` future `Send` (measured); a Durable
  Object or D1 future was not tried.
- **The client, and fan-out itself.** The client does not build for wasm32. 12
  of its 72 non-test source files name hyper-util's client, `HttpConnector`,
  `TcpStream` or `tokio::net`. A fan-out orchestrator Worker was **not** built; that is Phase 3.
- **Push notifications** from a Worker: the bundled sender is gated off.
- **`signing` on wasm** needs two feature flags in the application's manifest,
  not the crate's; a dependency that adds `ring` to the types crate for every
  wasm build would make a C toolchain a requirement for users who do not sign.
- **Lint coverage** is the listed methods only. It would not catch
  `std::thread`, `std::fs` or `tokio::task::JoinSet`.
- The "no panic" claims cover the paths the battery exercises, 11 cases.

## Consequences

- If accepted, a second target joins the CI matrix, and every new `tokio::spawn`
  or clock read in the server is a lint failure until it goes through `rt`.
- On native, `rt::time` is Tokio's own types, so `tokio::time::pause` keeps
  working in the existing tests (the lib suite passes unchanged). On wasm32
  there is no pausable clock, so tests of timer behaviour run on native only.
- ADR 0002's sentence about `wasm32` should be read with this ADR: true of the
  types crate's compile, not of its behaviour at HEAD.
