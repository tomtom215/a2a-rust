<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Swarm orchestration — what holds, what is missing, and the order to build it

Written 2026-10-02 from measurements, not from a feature list. Every number
below names the run that produced it. The swarm runs are
[`examples/swarm`](../examples/swarm) at `ba864a1`; the store numbers are
from the SDK comparison in
[`sdk-comparison-2026-10-02.md`](sdk-comparison-2026-10-02.md).

## What a swarm needs from this layer

A swarm here means hundreds to thousands of agents delegating to each other
in a tree, running for hours or days, often with no human watching. What
makes that work — planning, judging whether a result is right, deciding
when to escalate to a human — lives above A2A, in whatever drives the
agents. This SDK is the substrate. What a substrate owes a swarm is:

| | Requirement | Why a swarm cannot do without it |
|---|---|---|
| R1 | **Control.** Stopping a root stops everything under it. | An autonomous tree that cannot be stopped is not under control. |
| R2 | **Bounded failure.** A caller can tell retryable from final. | Otherwise it retries forever, or gives up on work that would have succeeded. |
| R3 | **Durability.** A node that restarts does not lose or orphan its subtree. | Over hours, every node restarts eventually. |
| R4 | **Throughput and bounded memory per node.** | A long-running node with unbounded memory does not finish the job. |
| R5 | **Isolation.** One tenant's tree cannot read or cancel another's. | Swarms share hosts. |
| R6 | **Observability across hops.** One job is traceable through the tree. | Debugging a ten-hop failure from ten separate logs does not work. |
| R7 | **Budget and authority.** A child cannot spend more than its parent granted. | Without this, cost and blast radius are unbounded. |

## What is measured today

| | Status | Evidence |
|---|---|---|
| R1 | **Missing in the protocol and the SDK; possible by hand.** A2A has no link from a task to the tasks it started. Cancelling a supervisor's root task left **256 of 256** (64-worker run) and **512 of 512** (256-worker run) children still executing 5 s later. With the cascade written by hand (`run_once` in `examples/swarm/src/supervisor.rs`, which follows each child's stream and cancels it when the parent's token fires), the same trees went from 256 to 0 live executions in 65 ms, and from 512 to 0 in 84 ms. | `swarm` `cancel` and `cancel-control` rows; `benches/sdk-comparison/results/swarm/` |
| R2 | **Holds.** `FailureClass` travels on the failed status. Retry-by-class was exact at both scales: 400 of 400 and 2,000 of 2,000 transient faults retried; 80 of 80 and 400 of 400 invalid requests not retried; every other job completed. | `swarm` `faults` rows |
| R3 | **Building blocks exist; the orchestrator side does not.** A task survives `kill -9` and a restart on the SQLite store (probe in the comparison). The event log and `Last-Event-ID` let a client resume a stream. Nothing records which children a supervisor started, so a supervisor that restarts should orphan its subtree just as the R1 control arm does (CONJECTURED: no run restarted a supervisor). | `feature_results.txt`; `docs/swarm-scale-findings.md` |
| R4 | **Memory bounded; throughput behind the official SDK.** The default store plateaus at about 98 MiB under sustained load, against a2a-rs growing without limit (565 MiB after 374,252 tasks, still climbing). Throughput was 1.17–1.62× behind a2a-rs per node. The TTL-sweep fix in `831b8ef` closed part of that gap (to 1.17–1.31×). Two delegation hops sustained 3,773–3,909 jobs/s on two cores. | comparison §4.3; `swarm` `fanout` rows |
| R5 | **Opt-in, and the default leaks.** The default `RequestHandlerBuilder` store ignores the `tenant` field: tenant B read, listed, subscribed to and cancelled tenant A's task in all 5 probes. With `TenantAware*` stores, all 5 were isolated. | `results/probes/tenant_results.jsonl` |
| R6 | **Exists, not exercised here.** Per source, W3C trace context is propagated by `TracePropagationInterceptor` and joined on the server's RPC span. The swarm runs did not turn it on, so nothing here measures it. | `crates/a2a-protocol-client/src/trace_propagation.rs` |
| R7 | **Absent.** Each child has its own executor timeout (1 h by default), whatever its parent has left. There is no budget field or grant. | `DEFAULT_EXECUTOR_TIMEOUT` |

## The gaps, ranked by what was measured

### G1 — Cancellation does not cascade (R1)

Measured: 100% of children orphaned when their root is cancelled, unless
every orchestrator writes the cascade itself. This is the gap that most
directly answers "can it be controlled".

| Approach | What it covers | Cost |
|---|---|---|
| **A. Client-side delegation handle** in `a2a-protocol-client`. It sends the child, records the child's task id from the first event, and cancels the child when a parent token fires or when the handle is dropped. | Every orchestrator built on this SDK, with no wire change. It does **not** cover a parent that crashes, because the handle dies with the parent. | Small. It is `run_once` from `examples/swarm/src/supervisor.rs`, made public and tested. |
| **B. Lease extension.** The child is sent a deadline and a renewal interval, declared as an extension the way idempotency and failure class are. A child whose lease lapses cancels itself. | Parent crash and network partition: the cases A cannot reach. | Medium. Server-side enforcement, an extension URI, and a renewal path. |
| **C. Lineage in the protocol.** A `parentTaskId`, and server-side cascade. | Cross-SDK, but only once other SDKs adopt it. | Large, and only meaningful upstream. |

**Recommendation: A now, B next, C only if the A2A project wants it.** A
removes the hand-written cascade the measurement shows every orchestrator
needs. B is what makes a long-horizon tree safe against the failure that will
certainly happen over hours: a node that disappears.

### G2 — A restarting supervisor orphans its subtree (R3)

The pieces exist: SQL stores, the event log, resubscribe from a cursor.
What is missing is the supervisor writing "I started child X on agent Y"
to its own durable record *before* awaiting the child, and reading it back
on restart to reattach with `subscribe_to_task_from`. This belongs with
G1's handle, which already learns the child id at the right moment.

### G3 — Tenant isolation fails open by default (R5)

A server that receives a `tenant` field and does not partition by it
should not silently serve the request. The options:

1. The default store becomes tenant-aware. Its cost is unmeasured.
2. A request carrying a tenant is refused when the configured store is not
   tenant-aware.
3. Leave the behaviour and document it more loudly.

**Decided 2026-10-02: option 2.** A request resolving to a non-empty tenant
is refused with `UnsupportedOperation` unless both stores answer
`isolates_tenants()`. An unset push-config store follows a tenant-aware task
store, and `RequestHandlerBuilder::accept_unisolated_tenants()` is the
explicit opt-out for tenants that key only limits. The tenant probe against
the fixed build records `refused` where it recorded five leaks.

### G4 — Per-node throughput (R4)

After `831b8ef`, a2a-rs is still 1.17× (64 connections) to 1.31× (16
connections) faster on the echo benchmark. The patched build's profile
has no SDK function above 1.64% self time. The libc allocator accounts for
about 31% of samples and kernel scheduling and wakeups for about 21%
(`benches/sdk-comparison/results/perf/profile_patched_top.txt`). What is
left is allocation volume and cross-task handoffs, spread across the
request path, not one fixable hotspot (CONJECTURED from the profile shape;
not attributed further).

Attributed further on 2026-10-03, after `1c2ca83` (echo harness, 2 server
CPUs, 16 connections):

| Per request | a2a-rust | a2a-rs |
|---|---|---|
| CPU time (`/proc` utime+stime, 4 interleaved runs, median) | 215 µs unary, 267 µs streaming | 178 µs, 234 µs |
| Instructions (cachegrind, 12 s minus 4 s run) | 413,537 / 477,857 | 228,969 / 260,384 |
| … of which `memcpy` | 216,935 / 243,451 | 34,104 / 33,597 |
| Instructions excluding `memcpy`, unary | 196,602 | 194,865 |
| `memmove` share of perf samples, unary | 6.6% | 7.0% |

The 1.8× instruction gap is almost entirely bulk copying. cachegrind counts
each byte iteration of `rep movsb`, so it inflates copies; on the real CPU
the copy costs both SDKs the same share (VALIDATED by the perf row). The
copies come mostly from futures moved by value: `on_send_message`,
`rpc_span::Call`, and the spawned executor task are the top callers. Other
work is equal to within 1% in instruction count. So the remaining ~16% CPU
gap is not in instructions executed. It is in kernel time (63 vs 57
µs/request) and allocator time, which the profile buckets put at 68 vs 57
µs/request including memmove. A second spawned task per request was the
leading suspect. Removing it (the executor run inside the collecting task)
was measured on 2026-10-03 and did not close the gap: 180.0 vs 185.6
µs/request with overlapping ranges, against a2a-rs's 153.0 on the same runs
(`docs/handoff.md` has the details, and why it was not landed). The cause is
still unattributed at that point.

**Attributed, 2026-10-03, second pass.** The earlier passes measured with
two server cores and with cachegrind, and both hid the cause. On two cores
both SDKs pay about 115 µs per request of cross-core overhead (a2a-rs 47 →
161 µs, a2a-rust 100 → 217 µs), which buries a few µs of difference in ±3%
noise. cachegrind serialises threads and counts a `lock`-prefixed or
cache-missing instruction as one, so equal instruction counts said nothing
about time. Pinned to **one core**, with `/proc` user and system time per
request and perf self time, the gap is plain:

| Per unary request, one core | a2a-rust | a2a-rs |
|---|---|---|
| user CPU (3 runs) | 74–80 µs | 37–40 µs |
| glibc allocator, perf self time | 33.2 µs | 12.6 µs |
| allocations ≥ 1 KB (dhat) | 11.9 | 5.9 |
| system calls (strace -c) | 4.4 | 5.5 |

Three causes, in order of size:

1. **Bounded memory: ~16 µs.** The default store caps at 10,000 tasks, so
   once full every new task evicts one written ~10,000 requests earlier —
   about 31 cache-cold allocations to free. With the cap lifted
   (`UNBOUNDED_STORE=1` in the harness, which is what a2a-rs's store does),
   user CPU is 58–64 µs and throughput 12.0–13.3k against 10.2–10.8k rps.
   This is the price of a guarantee a2a-rs does not make; its memory grows
   without limit (§ R4 of the comparison).
2. **Large allocations: ~4 µs recovered so far.** glibc serves blocks above
   ~1 KB outside its thread cache. `996a736` took a2a-rust from 11.9 to 7.9
   such blocks per send: +4.9% throughput on one core, nothing measurable on
   two. What remains: hyper's 13 KB box of the dispatch future, the 8 KB
   broadcast ring, the 6.6 KB executor task, the 3.3 KB collector task.
3. **The rest, ~17 µs** with the cap lifted — not yet attributed function by
   function.

**Cause 1 is glibc, not eviction (measured 2026-10-03).** Under jemalloc
the bounded and unbounded store cost the same per request (50.4–53.0 vs
50.4–50.8 µs, one core), and cutting the log each eviction frees to one
event (`MAX_EVENTS_PER_TASK=1`, 12 of 31 blocks gone) changed nothing under
glibc. What costs is glibc in the fixed-size, hole-filled heap a bounded
store keeps; its tunables do not help (`tcache_count=64`: no change). So
the store is not redesigned. The recommendation is the allocator, now in
the book's production chapter: on two cores jemalloc takes a2a-rust from
7,189 to 11,862 unary requests/s at 74 MB resident. With both servers on
jemalloc, a2a-rs leads 1.15× (13,522 vs 11,762) holding 265 MB.

Boxing the large futures (tried, not landed) cut copies and simulated cache
misses to a2a-rs's level but replaced each copy with a large allocation; it
measured no faster. The futures are large because of what they hold across
`.await` — `MessageSendParams` (448 B) twice in each of three layers, and
coroutine layout padding — so shrinking them, not boxing them, is the
remaining lever for (2). **Closing the gap on equal allocators (2026-10-03, third pass).** Both
servers on jemalloc, profiled on one core. a2a-rs runs a whole request in
its connection task; a2a-rust spent ~20 µs per unary send in two extra
tasks and three parses of each JSON-RPC body. Landed:

| Change | One worker, unary rps | Two workers, unary rps |
|---|---|---|
| before (`467b668`) | 14,166–14,770 | 12,133 |
| `701b02a` executor shares the collector's task on a single-worker runtime | 19,308–20,105 | 12,353 (no change) |
| `774593c` JSON-RPC body parsed once | 21,033–21,615 | — |

Where it stands (medians; one core 6 runs, two cores 4 runs):

| | a2a-rust | a2a-rs | ratio |
|---|---|---|---|
| one core, unary | 20,457 | 23,068 | 1.13× (was 1.65×) |
| two cores, unary | 12,436 | 13,595 | 1.09× (was 1.15×) |
| two cores, streaming | 10,560 | 12,800 | 1.21× |

Tried and rejected, each measured: the executor in the collector's task on
every runtime (−4.6% on two workers); the collection in the request's own
future behind a detach-on-drop guard (−21% on two workers: every event
re-polled hyper's connection future); the SSE body reading the queue
directly instead of through a forwarder task (slower on one core, and it
lost wakeups on two — a dropped `read()` future deregisters its waker);
skipping the second store lock for a new task (no measurable change).

**What remains for streaming.** The terminal gate — a stream's terminal
event waits for the background processor's verdict before it is
broadcast, so a stream never reports a terminal state the store refused —
costs 6.6% of two-core streaming throughput (probe with the gate removed:
10,893 → 11,614 rps; a2a-rs 12,485). Two ways to cut it were weighed:

- *The processor broadcasts the verdict itself.* Rejected without
  building it: a second broadcaster on one channel makes subscriber order
  depend on scheduling (an event written after the terminal one could be
  seen before it), lets the queue close before the terminal event is
  sent, has no clean timeout answer (duplicate or missing terminal), and
  weakens what a returned terminal `write` means to an executor.
- *Keep one broadcaster and the blocking write, and run the executor in
  the processor's task* — the change that took unary sends +36% on one
  worker (`701b02a`). Measured and rejected: streaming got slower, one
  core 11,273 → 10,425 rps (medians of 6), two cores 10,700 → 9,340
  (medians of 4).

So the gate's cost is not the handoff between two tasks; what it costs is
not attributed further. The likeliest remainder, not measured, is the
waiting itself: a terminal event is stored before it is sent, which any
design that keeps the guarantee keeps. The gate stays as it is.

**The streaming gap, attributed (2026-10-03).** One core, jemalloc for
both, 16 connections. `/proc` CPU per streamed request, 4 interleaved runs:
a2a-rust 86.1 µs (user 61.2, system 24.9), a2a-rs 70.2 µs (46.9, 23.8) —
a 15.9 µs gap, 14.3 of it user time. perf (8 s at 1,999 Hz each, frame
pointers; ~8% inflated) puts it at 17.1 µs and splits it:

| Where | a2a-rust | a2a-rs | Notes |
|---|---|---|---|
| Kernel, socket writes | 24.5 | 22.3 | 4.25 vs 2.66 writes per stream (`strace -c`): a2a-rust sends about one write per SSE frame, a2a-rs batches |
| User space | 66.2 | 51.0 | by leaf: SDK code +10.5, tokio/tracing +3.1, copies +3.1; allocator −3.4, serde −1.8 |

a2a-rust runs a streamed request on four tasks; a2a-rs on one. By task:
connection 62.6 µs, background processor 12.7 (persistence 7.0, of which
status 2.7, artifact 1.7, event log 1.3; future copies 1.9; initial store
read 0.6), executor 8.2 (the agent 4.7), SSE forwarder 7.8 (frame
serialization 2.0, keep-alive timer reset per event ~0.7, copies 0.7,
channel send 0.5). The time in those three tasks beyond their useful work
— about 5.7, 5.8 and 3.5 µs — accounts for roughly the user-space gap
(CONJECTURED as a sum: perf anchors overlap).

Merging tasks does not recover it: the executor in the processor's task
and the SSE body reading the queue directly were both measured slower
(above). Candidates that keep the layout, sized from this profile and not
yet measured: coalesce ready SSE frames into one body frame (the ~1.6
extra socket writes, ~2 µs); stop re-arming the keep-alive timer on every
event (~0.7 µs); shrink the processor's and forwarder's futures (~2.6 µs
of copies).

This
matters for a swarm only once a node is CPU-bound on protocol rather than
on model inference. In the live-model run the model was the bottleneck by
three orders of magnitude: about 2 jobs/s against 3,900 jobs/s with no
model.

### G5 — Budget and deadline propagation (R7)

Deadline first, budget later. A deadline is mechanical: the remaining time
is carried on the send, the child's executor timeout is capped at
`min(own, inherited)`, and the existing `BudgetExhausted` failure class
reports it. It also composes with G1-B, since a lease is a renewable
deadline. Token or cost budgets need agreement on units, which the A2A
specification does not supply. Attenuable grants (`docs/handoff.md`, B1)
stay speculative until a deadline exists to attenuate.

### G6 — Shared-channel limits (already measured)

`docs/swarm-scale-findings.md` covers the coordination-channel shape: one
channel peaks at four writers, sharding by context recovers throughput, and
the single-writer refusal does not cross replicas. Nothing here changes
those findings.

## Where this work goes, and where it does not

`docs/handoff.md` ("What not to chase") records a decision not to turn the
SDK into a runtime: no registries, schedulers, orchestration DSLs or mesh.
The plan above respects that. G1-A and G2 are a client-side handle on
existing methods. G1-B and G5 are extensions in the established style, like
idempotency and failure class. G3 changes a default. None of them puts
scheduling or placement into the protocol crates.
`examples/swarm` is where an orchestrator lives, and it stays an example.

What would falsify the priority order: if adopters' trees are one hop deep,
G1 matters much less and G3 matters more. The order above is argued from
the two-hop runs measured here.

## Next, in order

1. **G1-A** — the delegation handle in `a2a-protocol-client`, with the swarm
   example rewritten on top of it. The example's own CI gate then proves
   the handle cascades.
2. **G3** — decide fail-closed or not. Either answer is small to implement.
3. **G2** — durable child records plus reattach, exercised by a swarm run
   that kills a supervisor partway through.
4. **G1-B and G5** — the lease and deadline extension, with a run that kills
   a supervisor and measures how quickly its children stop.
