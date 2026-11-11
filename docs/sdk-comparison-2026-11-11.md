<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# SDK comparison, 2026-11-11: a2a-rust against a2a-rs on the same host

Written because an independent review of 2026-11-09 said, correctly, that no current
head-to-head existed: [`sdk-comparison-2026-10-02.md`](sdk-comparison-2026-10-02.md)
measured a2a-rust 0.14.1. This measures this tree. **On this workload a2a-rs is the
faster server, and this document does not argue otherwise.** Raw output:
[`benches/sdk-comparison/results/2026-11-11/`](../benches/sdk-comparison/results/2026-11-11/).

## What was measured

- **a2a-rust:** this repository at `da17606c`, as a path dependency (`harness/agent-a2a-rust`),
  default features, plus the change described under *What was changed* for the rows labelled
  `boxed`. **a2a-rs:** the crates.io releases `a2a-lf` 0.4.2, `a2a-server-lf` 0.5.3,
  `a2a-client-lf` 0.2.9 (the versions the review names at a2a-rs `main`; this is the published
  code, not a build of the git `main`, and the two were not diffed). **a2a-rust 0.14.1** is the
  control: the harness's committed manifest and lockfile before this change.
- **Workload:** the echo agent of the harness (one artifact, then `completed`), a closed-loop
  JSON-RPC load generator (`harness/loadgen`, hdrhistogram), blocking `SendMessage` (`send`) and
  `SendStreamingMessage` (`stream`). Server pinned to CPUs 0 and 1, load generator to 2 and 3
  (`scripts/run_perf.sh`), a fresh server process per cell, server order rotated per repetition,
  3 s warm-up and 10 s measured. Two a2a-rs configurations: `tuned` sets `TCP_NODELAY`
  (`NODELAY=1`), `asshipped` does not, as a2a-rs's own `helloworld` example.
- **Host:** Intel Xeon @ 2.10 GHz, 4 vCPUs (one thread per core), a shared virtual machine, kernel
  6.18.44, rustc 1.99.1 (58cf8f9c3 2026-10-13), the harness's release profile (no LTO), glibc malloc in
  every server. The matrix started on an otherwise idle machine; the one-minute load average
  was 3.3, left by the builds that had just ended.
- **Server CPU per request** is the server process's user plus system time from `/proc/PID/stat`
  (10 ms ticks) over its whole 13.5 s life, divided by the requests it served in that time
  (the measured rate times 13 s): good to a few percent, not exact.
- **Variance.** The machine is shared. Repetitions of one cell varied up to 20%, the 0.14.1 control
  measured between 3.0k and 6.2k requests/s in consecutive bisect steps and 2.9k to 6.4k across the six
  repetitions of table 4, and the same 0.14.1 build measured 6.1k and 5.7k requests/s at one
  connection in two five-repetition runs an hour apart (a2a-rs 7.8k and 7.1k). Read a difference
  under about 15% as unproven unless the ranges are disjoint.

## 1. The full matrix, before the change (a2a-rust at `da17606c`)

Three repetitions per cell; the one a2a-rs `tuned` stream cell at one connection had a repetition
with an error and 0 requests/s.

| mode | conns | server | requests/s, median [min–max] | p50 µs | p99 µs | server CPU µs/request | RSS MiB | errors |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| send | 1 | a2a-rs-asshipped | 7960 [7886–8294] | 112 | 215 | 103 | 156 | 0 |
| send | 1 | a2a-rs-tuned | 7757 [7527–8121] | 115 | 231 | 107 | 151 | 0 |
| send | 1 | a2a-rust | 4228 [3586–4823] | 206 | 474 | 228 | 64 | 0 |
| send | 16 | a2a-rs-asshipped | 9821 [9102–10080] | 1583 | 2375 | 143 | 228 | 0 |
| send | 16 | a2a-rs-tuned | 10072 [9641–10141] | 1548 | 2265 | 139 | 232 | 0 |
| send | 16 | a2a-rust | 7790 [6959–8155] | 2000 | 3285 | 213 | 68 | 0 |
| send | 64 | a2a-rs-asshipped | 9347 [9009–9454] | 6671 | 8695 | 151 | 230 | 0 |
| send | 64 | a2a-rs-tuned | 9336 [9127–9549] | 6655 | 8951 | 152 | 232 | 0 |
| send | 64 | a2a-rust | 7496 [7458–7863] | 8367 | 12151 | 223 | 74 | 0 |
| send | 256 | a2a-rs-asshipped | 8543 [7683–8762] | 29423 | 40351 | 165 | 189 | 0 |
| send | 256 | a2a-rs-tuned | 8718 [7891–8731] | 28959 | 37279 | 162 | 183 | 0 |
| send | 256 | a2a-rust | 6775 [6351–7249] | 37279 | 52063 | 248 | 98 | 0 |
| stream | 1 | a2a-rs-asshipped | 23 [23–23] | 43999 | 45279 | 424 | 8 | 0 |
| stream | 1 | a2a-rs-tuned | 5721 [0–5959] | 151 | 285 | 134 | 128 | 1 |
| stream | 1 | a2a-rust | 4524 [4387–4648] | 203 | 414 | 223 | 67 | 0 |
| stream | 16 | a2a-rs-asshipped | 363 [363–363] | 43999 | 47967 | 196 | 16 | 0 |
| stream | 16 | a2a-rs-tuned | 10509 [10222–10802] | 1468 | 2107 | 162 | 237 | 0 |
| stream | 16 | a2a-rust | 6405 [6126–7042] | 2381 | 4659 | 268 | 68 | 0 |
| stream | 64 | a2a-rs-asshipped | 1445 [1440–1446] | 44031 | 48159 | 155 | 41 | 0 |
| stream | 64 | a2a-rs-tuned | 10103 [9932–10297] | 6147 | 8147 | 166 | 239 | 0 |
| stream | 64 | a2a-rust | 6684 [6403–6825] | 9375 | 13927 | 258 | 72 | 0 |
| stream | 256 | a2a-rs-asshipped | 5619 [5610–5619] | 44191 | 51967 | 146 | 137 | 0 |
| stream | 256 | a2a-rs-tuned | 8956 [8609–9732] | 27887 | 36415 | 184 | 234 | 0 |
| stream | 256 | a2a-rust | 6896 [6390–7018] | 36575 | 49183 | 251 | 88 | 0 |

## 2. Where a2a-rust stands against its own 0.14.1 (interleaved, five repetitions)

Before the change:

| mode | conns | server | requests/s, median [min–max] | p50 µs | p99 µs | server CPU µs/request | RSS MiB | errors |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| send | 1 | a2a-rs-tuned | 7763 [7667–8123] | 115 | 247 | n/r | n/r | 0 |
| send | 1 | a2a-rust-0.14.1 | 6090 [5606–6278] | 136 | 489 | n/r | n/r | 0 |
| send | 1 | a2a-rust-head | 4301 [4284–4620] | 202 | 471 | n/r | n/r | 0 |
| send | 16 | a2a-rs-tuned | 10012 [9673–10097] | 1557 | 2253 | n/r | n/r | 0 |
| send | 16 | a2a-rust-0.14.1 | 8354 [8000–8402] | 1868 | 2927 | n/r | n/r | 0 |
| send | 16 | a2a-rust-head | 7827 [7565–8058] | 1996 | 3225 | n/r | n/r | 0 |

After it (the `boxed` build):

| mode | conns | server | requests/s, median [min–max] | p50 µs | p99 µs | server CPU µs/request | RSS MiB | errors |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| send | 1 | a2a-rs-tuned | 7136 [6834–7853] | 124 | 298 | 120 | n/r | 0 |
| send | 1 | a2a-rust-0.14.1 | 5690 [5035–6016] | 149 | 518 | 149 | n/r | 0 |
| send | 1 | a2a-rust-boxed | 4919 [4057–5100] | 180 | 413 | 210 | n/r | 0 |
| send | 16 | a2a-rs-tuned | 9603 [9429–9909] | 1612 | 2405 | 152 | n/r | 0 |
| send | 16 | a2a-rust-0.14.1 | 8067 [6472–8498] | 1931 | 3077 | 193 | n/r | 0 |
| send | 16 | a2a-rust-boxed | 7964 [7720–8030] | 1958 | 3281 | 216 | n/r | 0 |

## 3. One core: no cross-thread hand-offs (five repetitions)

The server pinned to a single core, so the runtime has one worker.

| mode | conns | server | requests/s, median [min–max] | p50 µs | p99 µs | server CPU µs/request | RSS MiB | errors |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| send | 1 | a2a-rs-tuned | 9241 [8470–9534] | 96 | 198 | 76 | n/r | 0 |
| send | 1 | a2a-rust-0.14.1 | 7426 [6954–7536] | 109 | 410 | 99 | n/r | 0 |
| send | 1 | a2a-rust-boxed | 6997 [5665–7037] | 128 | 287 | 111 | n/r | 0 |
| send | 16 | a2a-rs-tuned | 18203 [17478–19109] | 821 | 1866 | 52 | n/r | 0 |
| send | 16 | a2a-rust-0.14.1 | 9427 [8575–9890] | 1665 | 3133 | 100 | n/r | 0 |
| send | 16 | a2a-rust-boxed | 9067 [8584–9142] | 1757 | 3417 | 104 | n/r | 0 |

## What the numbers support

- **PLAUSIBLE→VALIDATED (tables 1 and 2):** a2a-rs serves 1.25 to 1.9 times the blocking-send
  throughput of this tree at every concurrency tested (1.8 to 1.9 at one connection, 1.25 to 1.3 at 16 to 256), and has lower p50 and p99. At one
  connection the ranges do not overlap (a2a-rs 7.8k, this tree 4.3k before the change, 4.9k after).
- **VALIDATED (table 2):** this tree was slower than 0.14.1 at one connection (4.3k against 6.1k
  requests/s, p50 202 against 136 µs, ranges not overlapping) and slightly slower at sixteen
  (7.8k against 8.4k, ranges touching). The project's in-process benchmark
  (`docs/readiness-bar.md`, 2026-10-24) had read "not slower than 0.14.1"; it runs client and
  server in one process on one worker, which table 3 shows hides the cost.
- **VALIDATED (table 1):** a2a-rust holds 2.5 to 3 times less resident memory (64 to 98 MiB
  against 150 to 230 MiB for a2a-rs `tuned`).
- **VALIDATED (table 1):** a2a-rs `asshipped` (no `TCP_NODELAY`) is very slow when streaming
  (about 44 ms per request at one connection) because of the interaction of Nagle's algorithm with
  delayed acknowledgements; `tuned` is not. This is a configuration finding about a2a-rs, not a
  property of the protocol.
- **VALIDATED (table 3):** with one worker the gap to 0.14.1 is small (111 against 99 µs of CPU
  per request at one connection, 104 against 100 at sixteen) and a2a-rs is about twice as
  CPU-efficient per request at sixteen connections (52 against 104 µs).

## Why: what was found, and what was not

1. **Wake-ups, attributed.** `strace -f -c` over a fixed run, per request, a2a-rust 0.14.1 /
   this tree (boxed) / a2a-rs: `write` 0.11 / 2.2 / 0.04, `epoll_wait` 1.1 / 2.7 / 1.1, `futex`
   2.0 / 2.1 / 2.0, `sched_yield` 0 / 0.5 / 0, `mprotect` 0.15 / 0.62 / 0.19. The extra `write` calls
   are 8-byte writes to the runtime's wake-up descriptor (`mio::Waker::wake`): one thread waking
   the one that is parked in the I/O driver. With frame pointers and `strace -k` (257 such writes in a
   2 s sample at one connection; `results/2026-11-11/strace-k-wakeup-callers.txt`), the callers
   were: **76** `Sleep::poll` → `time::Handle::reregister` inside hyper's HTTP/1 connection (most
   likely the header-read timer `ServeConfig` sets, ADR 0042: hyper only arms it when given a timer,
   and it is the one timer the connection arms per request; the frame says only `Sleep`); **50** the same
   `reregister` under a `tokio::time::timeout` around a `watch::Receiver` in `streaming`; **73**
   `transition_worker_from_searching` (the scheduler waking a second worker); **48** a task scheduled
   from another thread. A timer armed with a deadline earlier than the driver's next wake-up has to wake
   the driver, and a timer is armed, and cancelled, on every request. 0.14.1 and a2a-rs arm none per
   request. **VALIDATED** as where the wake-ups come from; **CONJECTURED** that they are most of the
   two-core gap (table 3 shows the gap mostly closing with one worker, which fits, but the cost per
   wake-up was not isolated). Not tried: it is a security feature (slowloris), so the fix is a
   cheaper way to bound a header read, not removing it.
2. **Large futures, found and fixed.** callgrind (`scripts/callgrind_per_request.sh`, instructions
   per blocking send, the difference of a 25 s and a 6 s run): 0.14.1 about 320,000; this tree
   about 604,000. 261,000 of the 284,000 added were `memcpy`, called from `tokio::spawn` and its
   wrappers: the executor's `async` block and the blocking send's collector block are each
   about 36,000 bytes of state machine, and `TaskLocalFuture`, `Instrumented` and the task
   tracker each moved the whole of it by value, four layers per spawn. Boxing each once before
   the wrappers took the count to about 277,000. **Caution, measured:** callgrind counts a
   `rep movsb` copy per byte, so a 36,000-byte copy is 36,000 "instructions" and a few hundred
   cycles, and the instruction count over-states the gain. The timing is the result, and it is
   small: table 4, six interleaved repetitions of the unmodified tree, the boxed tree and the 0.14.1
   control, shows 4.4k to 4.7k requests/s and 232 to 216 µs of server CPU at one connection, 7.5k to
   7.9k and 229 to 216 µs at sixteen. The direction is the same in all four comparisons; the ranges
   overlap, so this is not proven to be more than noise. The change is kept because it removes about
   230 KB of copying per request deterministically at the cost of one allocation the task cell was
   making anyway, and the whole server suite passes with it (5,044 of 5,045 tests, the one failure a
   test of the same session's, fixed).
3. **A design already tried.** The 2026-10-03 commit `701b02a1` records that 0.14.1 already ran a
   blocking send on two tasks (executor and collector), and that the collection in the request's own
   future behind a detach-on-drop guard was measured and rejected (-21% at two workers: every event
   re-polled hyper's connection future). That route is not a fix for table 2.
4. **A bisect that did not work.** `git bisect` over the 1,291 commits since 0.14.1 on
   one-connection throughput, each step interleaved with the 0.14.1 build, landed on a commit that
   moves tests inside the SLIMRPC binding (`e8e7d8bd`). The signal (a 29% fall in all) is spread over
   many commits and the noise of this host is larger than any one of them. The result is recorded
   in `results/2026-11-11/bisect-inconclusive.log` and is not a finding.
5. **Not established.** Where the rest of the per-request CPU goes (allocation is about 25% of
   self time by callgrind, a `SipHash` map about 4%, glibc `malloc`; a profile by time was not
   possible: no `perf` here). Whether the gap holds for HTTP+JSON, gRPC, WebSocket, with a real
   executor (a model's latency dominates any real agent), on other hardware, or with the features
   off (`default-features = false` was not measured). Throughput at 64 and 256 connections and
   the streaming matrix after the change (table 1 is from before it).

## 4. The boxing change, directly: six interleaved repetitions

The unmodified tree (`da17606c`), the boxed tree and the 0.14.1 control, server on CPUs 0 and 1,
load generator on 2 and 3, 3 s warm-up and 10 s measured.

| conns | build | requests/s, median [min–max] | p50 µs | p99 µs | server CPU µs/request |
|---:|---|---:|---:|---:|---:|
| 1 | 0.14.1 | 5379 [2859–6370] | 154 | 546 | 157 |
| 1 | boxed | 4719 [2851–5387] | 182 | 488 | 216 |
| 1 | unboxed | 4384 [3938–4875] | 203 | 468 | 232 |
| 16 | 0.14.1 | 7824 [6894–8439] | 1946 | 3289 | 199 |
| 16 | boxed | 7896 [7089–8367] | 1978 | 3259 | 216 |
| 16 | unboxed | 7523 [6015–7814] | 2070 | 3494 | 229 |

## 5. With a real model

`scripts/llm_e2e.py`: each SDK's agent streams a real model's answer over `SendStreamingMessage`, and the
text is compared with calling the model directly. llama.cpp `llama-server` f3b407ca with Qwen3.5-2B
(Q4_K_M, thinking off, temperature 0, seed 42, 64 tokens, one slot so batching cannot change the logits),
five prompts, two rounds, the three targets interleaved in rotating order, this tree at the boxed build:

| target | samples | time to first token, median / p90 ms | whole answer, median ms | text identical to direct | task completed |
|---|---:|---:|---:|---:|---:|
| direct to the model | 10 | 197 / 291 | 4220 | 10 of 10 | 10 of 10 |
| a2a-rs | 10 | 198 / 359 | 4200 | 10 of 10 | 10 of 10 |
| a2a-rust | 10 | 212 / 324 | 4382 | 10 of 10 | 10 of 10 |

The same run with Gemma 4 E2B (Q4_0, ggml-org; `results/2026-11-11/llm-e2e-gemma-4-e2b.txt`): direct 97 ms to
the first token and 1017 ms for the answer, a2a-rs 97 and 1059, a2a-rust 99 and 1016; text identical to direct
10 of 10 for both and 10 of 10 tasks completed.

The paired first-token overhead on Qwen was -3.7 ms (a2a-rs) and +17 ms (a2a-rust) at the median, with ranges of
about ±200 ms (on Gemma -3.7 and +11 ms, ranges about ±70 ms): **a sample of ten cannot separate either SDK from the model's own variance, and with a
model in the loop the echo benchmark's gap is not what a user would see.** What the run does show is that the
streaming path (executor, event queue, SSE) delivers a real model's answer byte for byte on this tree. Raw:
`results/2026-11-11/llm-e2e-qwen3.5-2b.txt`.

## What was changed

`RequestHandler::executor_future` and `respond_blocking` box their large inner future once,
before `TenantContext::scope`, the span wrapper, the task tracker and `tokio::spawn`
(`crates/a2a-protocol-server/src/handler/messaging/execute.rs`, `respond.rs`). Behaviour is
unchanged; it adds one allocation per blocking send of the size the task cell was already
allocating. No other performance change was made. The remaining gap is listed, not closed.
