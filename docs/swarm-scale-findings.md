<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Swarm scale — what A2A coordination does at a thousand agents

Measured 2026-09-20 at `391f0df`, by
`crates/a2a-protocol-server/tests/swarm_scale/`.

Nothing in this repository had pointed load at many agents sharing **one**
object. `benches/benches/concurrent_agents.rs` sweeps `[1, 4, 16, 64]` and
every agent in it drives its own task; `tests/soak.rs` runs eight workers for
an hour, again one task each. Both measure a server whose callers never
contend. The question this closes is the other shape — a swarm coordinating on
shared state — and the answer is not the one the concurrency benchmarks
suggest.

## The mapping under test

A2A has eleven methods and not one of them is a topic, a broadcast, or a
subscription an agent can take out for itself. What it has, since 0.13.0, is a
durable ordered per-task event log, a broadcast fan-out so many subscribers can
read one task's stream, and an SSE `id:` that *is* the stored `seq` — which
makes `Last-Event-ID` a cursor into the log.

A log, a cursor and fan-out is a channel. So the mapping measured here is:

| channel operation | A2A, unchanged |
|---|---|
| post | `SendMessage` naming the channel's `taskId` |
| tail from a cursor | `SubscribeToTask` + `Last-Event-ID` |
| the ordered record | that task's event log |
| a group of channels | a shared `contextId` |

## How to reproduce

```text
A2A_SWARM_MAX=1000 cargo test -p a2a-protocol-server --release \
  --test swarm_scale -- --ignored --nocapture --test-threads=1
```

`--release` matters: a debug build makes the load generator the bottleneck and
measures the generator. `--nocapture` matters because the tables are the
deliverable.

**Environment.** One container, 4 cores (Intel Xeon @ 2.80GHz), 15 GiB RAM,
Linux 6.18.44, rustc 1.94.1. Server configuration, printed above every table:
in-memory store capacity 8,192, event queue capacity 256, max connections
4,096, `max_context_locks` 10,000 (the default).

**What this is not.** One process, loopback, in-memory store, no proxy, no
second replica, and a load generator sharing four cores with the server it is
loading. Every throughput and latency figure is therefore an **upper bound**,
and a SQL-backed store writes to a disk this never touches. The four findings
below are structural — which calls are refused, which posts a tail receives —
and none of them depends on the box.

## Finding 1 — one channel does not scale; it peaks at four writers

1,000 agents, five posts each, all to one channel:

| agents | accepted | in-flight refusals | accepted | p50 | p95 | posts/s |
|---|---|---|---|---|---|---|
| 1 | 5 | 0 | 100.0% | 244µs | 367µs | 3,785 |
| 4 | 15 | 5 | 75.0% | 292µs | 394µs | **9,377** |
| 16 | 71 | 9 | 88.8% | 2.0ms | 3.2ms | 5,832 |
| 64 | 283 | 37 | 88.4% | 22.5ms | 30.9ms | 2,098 |
| 256 | 1,145 | 135 | 89.5% | 107.7ms | 1.07s | 640 |
| 1,000 | 4,343 | 657 | 86.9% | **1.31s** | **2.57s** | 522 |

Throughput peaks at four concurrent writers and then falls monotonically — 18×
down from the peak at a thousand agents, with a median post taking 1.3 seconds
and a p95 of 2.6.

What does *not* happen is as informative. The refusal rate sits between 10%
and 13% from 16 agents upward and does not climb with N. The channel is not
rejecting its way out of the load, it is **queueing**: `commit_task` takes a
per-context lock and holds it across the find-decide-save sequence, so
concurrent posts serialize there and pay in latency rather than in errors.

The refusals that do occur are the single-writer rule:
`admission::reject_in_flight_send` refuses a send to a task whose executor is
still running, because the alternative is a second executor racing the first on
store writes. It is correct, and for a channel it means concurrent posters are
turned away rather than queued behind each other.

## Finding 2 — a context holds exactly one addressable channel, and it moves

Probed directly rather than read off the source
(`fan_in::how_a_context_addresses_its_channels`):

| probe | result |
|---|---|
| post into the context naming **no** task | **Accepted** — and it landed on a *brand new* task |
| post naming the original task | **Refused**, `message task_id does not match task found for context` |
| post naming the task the untargeted post created | Accepted |
| post naming a task from another context | Refused, same message |
| post naming the original task again | **Refused** — permanently |

Two rules combine. `resolve_task_id` returns a fresh uuid whenever the message
names no task — unconditionally, whether or not the context already has a live
one. And `helpers::find_task_by_context` resolves a context to the first
non-terminal task the store lists, which for a §3.1.4-conformant store is the
**most recently updated** one.

So a context is not a folder holding many channels. It is a single slot whose
occupant is whichever channel was written to last, and **one participant that
omits a `taskId` silently forks the channel and locks every other participant
out of the original** — with a 400 that names a mismatch rather than anything a
caller would recognise as "someone displaced your channel".

Note what is *not* a hazard here. `CONTEXT_LOOKUP_PAGE_SIZE` is 10, so the
lookup sees only ten tasks — harmless precisely because the list is
recency-ordered and the live one is therefore first. It would become a live
hazard under a store that ordered differently.

Two source-level observations fell out of this and are for the maintainer, not
for the swarm question:

* The comment above that branch of `resolve_task_id` says "If the found stored
  task is terminal, a new task will be created on this context". The code does
  not consult `stored_task` at all on that path. The comment describes a
  condition that is not checked, and the measured behaviour is the
  unconditional one.
* Whether a message carrying only a `contextId` *should* join the context's
  live task rather than fork a new one is a spec question this experiment does
  not answer. It only establishes what the implementation does.

### Fixed — and the spec question turned out to be the wrong question

Both observations above are now resolved, and the second one resolved in a way
that moved the defect. The spec settles the fork, and permits it:

> Clients **MAY** use `contextId` without `taskId` to start a new task within
> an existing conversation context.
>
> — §3.4.3, `docs/implementation/v1.0.0-specification-complete.md:651`

So the fork was never the bug. The bug was the *lockout that followed it*. The
same section permits the continuation the server was refusing, and mandates
exactly one rejection, which is not this one:

> Clients **MAY** use `taskId` (with or without `contextId`) to continue or
> refine a specific task
>
> Agents **MUST** reject messages containing mismatching `contextId` and
> `taskId` (i.e., the provided `contextId` is different from that of the
> referenced `Task`).
>
> — §3.4.3, lines 650 and 653

When participant B posted with the original `taskId`, that task existed, was
non-terminal, and its `contextId` *did* match the one supplied — so line 653's
rule did not apply — yet `resolve_task_id` rejected it anyway, purely because
`find_task_by_context` had handed it a different task from the same context.
§3.4.1 says a `contextId` "logically groups multiple `Task` objects"; the
implementation treated it as holding one.

`resolve_task_id` now resolves against the task the message actually names: it
accepts any live task in the same context, and refuses only a task from a
different one. Re-running the same probe:

| probe | before | after |
|---|---|---|
| post naming **no** task | Accepted, forked | Accepted, forked (§3.4.3 permits it) |
| post naming the original task | **Refused** | **Accepted** |
| post naming the task the untargeted post created | Accepted | Accepted |
| post naming a task from another context | Refused | Refused (§3.4.3 requires it) |
| post naming the original task again | **Refused, permanently** | **Accepted** |

One existing unit test changed its expectation, which is worth stating plainly
rather than burying. `task_id_mismatch_returns_invalid_params` named a task
that *did not exist at all* and asserted `InvalidParams`. §3.4.2 requires
`TaskNotFound` for that, and the handler already returned `TaskNotFound` for
the identical input when the context happened to be empty — the only thing
deciding between the two answers was whether some unrelated task existed
nearby. It is now `task_id_naming_no_existing_task_returns_task_not_found`,
and three tests were added beside it for the cases nothing covered: a
cross-context `taskId` (still `InvalidParams`), a live sibling in the same
context (now accepted), and a terminal sibling (still `UnsupportedOperation`,
now judged on the task actually named rather than on the canonical one).

The stale comment is gone too, replaced by what the code does and the spec
line that permits it.

## Finding 3 — sharding by context recovers it, and the knee is early

The same 1,000 agents, five posts each, spread round-robin over K channels in K
contexts:

| contexts | agents/channel | accepted | posts/s | p50 |
|---|---|---|---|---|
| 1 | 1000 | 87.7% | 512 | 1.23s |
| 4 | 250 | 99.9% | 728 | 1.05s |
| 16 | 62 | 100.0% | 2,054 | 173ms |
| 64 | 16 | 100.0% | 2,592 | 67ms |
| 256 | 4 | 97.2% | 3,278 | 68ms |
| 1,000 | 1 | 100.0% | 3,299 | 52ms |

Sharding recovers 6.4× the throughput and 24× the median latency, and the
single-writer refusal disappears entirely from K=4 onward. Returns flatten
sharply after K=64: going from 64 contexts to 1,000 buys 27% more throughput
for 16× the channels.

**The design number.** 16 agents per channel (K=64) captures 79% of the
available throughput and all of the acceptance. Somewhere between 4 and 16
writers per channel is the operating point, which is the same place Finding 1's
curve peaks.

The shard key has to be the **context**, not the task. Finding 2 is why: extra
tasks inside one context are not extra channels, they are a lockout.

## Finding 4 — a tail is live only if turns outlast the reattach poll

A task's event queue lives exactly as long as one executor invocation. A
`SubscribeToTask` stream that is between queues does not block on the next one
— `handler::lifecycle::subscribe`'s reattach hook **polls** for it every
`subscribe_reattach_interval`, 250ms by default. So a tail's liveness is a race
between the turn and the poll.

Both arms, 40 and 20 posts, tails attached before the first post:

| turn dwell | tails | posts | tails that received every post | gaps |
|---|---|---|---|---|
| 0ms | 1 … 1,000 | 40 | **none, at any M** | 0 |
| 300ms | 1 … 256 | 20 | **all of them** | 0 |
| 300ms | 1,000 | 20 | all but one (18 of 20 at the window edge) | 0 |

A channel whose turns park instantly delivers essentially **nothing** to a live
tail, at every subscriber count from 1 to 1,000: no tail at any M received all
40 posts, and the only row where anything arrived at all was M=256, where 13 of
256 tails caught a single position each. Make the turn outlast the poll
interval and every tail receives every post with zero gaps — including a
thousand concurrent tails on one channel, which is the fan-out working exactly
as designed.

This is not a defect for the workload A2A was built for, where a turn is an LLM
call and lasts seconds. It is decisive for a coordination channel, where a post
is an append and a turn is microseconds: such a channel is not tailable live at
all. Its readers have to poll `Last-Event-ID` instead, which works — a replay from
position 0 returned 42 positions, contiguous from 1 to 42 with zero gaps: the
40 posts plus the two that opened and settled the channel.

### Loss is announced, never silent

A separate arm emits 1,024 events inside **one** turn — four times the
broadcast capacity — with every tail attached for the whole of it:

| tails | emitted | positions seen (non-empty tails) | tails that got nothing | gaps | streams ended by `streamLagged` |
|---|---|---|---|---|---|
| 1 | 1,024 | — | 1 | 0 | 1 of 1 |
| 4 | 1,024 | 43 | 3 | 0 | 4 of 4 |
| 16 | 1,024 | 1 and 64 | 14 | 0 | 16 of 16 |
| 64 | 1,024 | 21 | 63 | 0 | 64 of 64 |

Every tail was cut off — most of them before a single event reached it — and
**every one was told**. A receiver that falls
behind gets `broadcast::error::RecvError::Lagged`, which
`InMemoryQueueReader::read` turns into `A2aError::stream_lagged`, which
`streaming::sse` writes as an `event: error` frame before closing the stream.
Zero gaps across every row: what a tail received was a contiguous prefix,
however short, and then an announced end. No subscriber silently lost an
event.

That is the property a record needs, and it is the one thing in this whole
experiment that came out better than expected. It matters more than the
throughput numbers: a reader that can always tell it fell behind can always
recover from the log.

Two honest qualifications. A single tail lagged too, so this is a **producer
speed** result, not a subscriber-count one — the burst is an unthrottled
in-process loop and the SSE writer cannot match it at any M. And the
`streaming::event_queue::in_memory` module documentation says a lagging
consumer "receives `Lagged(n)` and skips missed events — this is acceptable for
SSE delivery". The consumer does not skip and resume; its stream ends. The
behaviour is the better of the two, and the documentation describes the other
one.

## Finding 5 — the transport has 11x headroom the handler never uses

Everything above is about contention. This one is not, and it is the one that
decides whether "fast" is a claim this project can make.

Every row below is measured twice at the same concurrency: `GET /health`,
which takes the same socket, the same hyper connection handling and the same
dispatcher routing and then returns a fixed body without touching the handler,
the store or the executor; and `POST /message:send` to a channel **only that
agent uses**, so nothing in the handler contends. The first bounds what this
box and this load generator cost. The second is the workload.

| agents | `/health` per s | `/health` p50 | posts per s | post p50 | handler's share | accepted |
|---|---|---|---|---|---|---|
| 1 | 11,064 | 21µs | 3,685 | 206µs | 90% | 100.0% |
| 4 | 30,270 | 85µs | 5,250 | 708µs | 88% | 98.8% |
| 16 | 52,799 | 199µs | 4,671 | 3.2ms | 94% | 99.1% |
| 64 | 32,556 | 734µs | 4,410 | 12.8ms | 94% | 98.5% |
| 256 | 4,984 | 1.8ms | 3,691 | 42.6ms | 96% | 94.9% |
| 1,000 | 17,240 | 5.8ms | 3,669 | 202.5ms | 97% | 100.0% |

The HTTP stack answers 52,799 requests a second on this box while an
uncontended send answers 4,671. At a concurrency of one the split is 21µs of
transport against 206µs of request — **about 90% of a send is work behind the
dispatcher**, and that share only grows with load.

Posts also do not scale with concurrency. They sit between 3,669 and 5,250 per
second from one agent to a thousand, which is the signature of a per-request
cost that is simply large, not of a box that has run out of cores — the same
box does ten times that number through the same sockets.

## Finding 6 — a channel makes its own posts slower, and history is why

Concurrency held at one, so these are service times rather than queueing.
One channel, 1,400 sequential posts, timed individually:

| posts | p50 | p95 | reply size |
|---|---|---|---|
| 0–100 | 385µs | 656µs | 152 B |
| 200–300 | 1,009µs | 1,264µs | 152 B |
| 500–600 | 1,774µs | 2,852µs | 152 B |
| 900–1,000 | 2,634µs | 3,986µs | 152 B |
| 1,000–1,100 | 2,678µs | 4,997µs | 152 B |
| 1,300–1,400 | 2,495µs | 3,800µs | 152 B |

Service time grows **6.5x** over the run and then flattens. The reply is a
constant 152 bytes throughout, so none of it is payload: the response to a
send carries the task's id, context and status and not its history.

Two things grow per post on one channel — the task's `history`, capped at
`MAX_TASK_HISTORY_MESSAGES` (1,024), and its event log, which is not capped
the same way. The curve turning over between 1,000 and 1,100 posts points at
the first. A controlled run settles it: with that constant lowered from 1,024
to 64 and nothing else changed, the plateau falls from about 2,500µs to about
540µs and the growth from 6.5x to 1.3x.

| history cap | p50 at posts 0–100 | p50 at posts 1,300–1,400 | growth |
|---|---|---|---|
| 1,024 (default) | 385µs | 2,495µs | 6.5x |
| 64 (probe only) | 419µs | 547µs | 1.3x |

**History length is the driver**, at roughly 2µs of service time per retained
message per post. The 64 figure comes from a temporary edit to
`handler::messaging::decisions::MAX_TASK_HISTORY_MESSAGES`, made to run this
experiment and reverted; the constant in the tree is 1,024.

The store is not implicated. Filling it with other channels in other contexts
leaves one channel's own cost flat — 538µs, 485µs, 671µs and 538µs at 0, 100,
1,000 and 4,000 other tasks — so `InMemoryTaskStore`'s `context_index` is
doing its job and the cost is per-channel, not global.

Reading the send path for where O(history) work happens finds at least four
places a continuation touches the whole history: `find_task_by_context` calls
`TaskStore::list`, which clones every task it collects; `messaging::create`
clones the stored history to append one message to it; `TaskStore::save`
stores a clone of the resulting task; and the background state machine saves
again on each status event. That attribution is **read from the source and
consistent with the controlled result above, but not independently profiled** —
no profiler is available in this container.

## Fix 1 — `TaskStore::save_status_delta`, and what it did and did not do

The first change made off the back of finding 6. `save` takes a whole `Task`,
and a `Task` carries its history, so every status transition copied whatever
the conversation had accumulated. `TaskStore::save_status_delta` is an
additive trait method — default implementation delegates to `save`, so no
existing store breaks — that the in-memory store overrides to edit the stored
status in place and re-key its indexes, touching neither history nor the
event log. It follows `save_artifact_delta`, which already exists for the same
reason on the artifact path.

**Where it pays.** A turn emitting `n` status events on a channel holding `h`
messages was `n * h` work. One turn of 512 events, concurrency one:

| history | turn with `save` | turn with `save_status_delta` |
|---|---|---|
| 1 | 1,706µs | 1,044µs |
| 200 | 18,609µs | 1,280µs |
| 600 | 54,301µs | 2,248µs |

The ratio at 600 messages is 24x, but the shape matters more than the ratio:
with `save` the turn grows with the channel's age and with the delta it does
not.

These are back-to-back runs of the final code. An earlier measurement of the
same arm reported 31x; that run predates the eviction-pacing fix below, which
adds a store-length read, a counter bump and an occasional sweep to every
delta. 31x was real for the code that existed then and is not the number to
quote for what shipped.

**Where it does not.** On the ageing probe — turns that emit one event each —
it is within noise: the plateau moved from about 2,495µs to about 2,477µs.
That is not a disappointment, it is the measurement working. A single-event
turn pays this cost once, against four other O(history) copies that dominate
it.

Those four were then measured directly rather than guessed at, by timing the
stages of `commit_task` in a temporary build (reverted; `git diff` over `src/`
carries none of it). Per send, at the history cap:

| stage | at history 1 | at history 1,024 |
|---|---|---|
| `find_task_by_context` | 3µs | ~280µs |
| `build_initial_task` | 4µs | ~300µs |
| `persist_initial_task` | 12µs | ~440µs |

All three scale with history, and together they are roughly 40–50% of a
2,400µs request. The rest is spread across the executor spawn, the event-log
append, the snapshot read for the response, and HTTP.

This is why the remaining work is structural rather than more delta methods:
the unit passed through the send path is a `Task`, and a `Task` carries its
history, so every stage that touches one pays for the conversation's length.

### Fix 2 — the send path stops carrying the conversation

`TaskStore::save_appending_history` takes the snapshot plus **only the
messages this turn added**, and appends them store-side. `task.history` is
ignored by it, deliberately: reusing that field for the tail — the shape
`save_artifact_delta` uses — would have put the caller back in possession of
the whole conversation, which is the thing being removed. `build_initial_task`
now builds a one-message history instead of cloning the stored one forward,
and the store appends it.

Re-running the ageing probe back to back on one box, 1,400 sequential posts,
concurrency one, in-memory store:

| posts | before | after |
|---|---|---|
| 0–100 | 1,250µs | 1,288µs |
| 1,300–1,400 | 3,962µs | 2,748µs |
| growth across the run | 3.2x | 2.1x |

31% off the per-send cost at the history cap, and nothing at the start — which
is the honest shape of the change. A fresh channel has no accumulated history
to avoid copying, so there is nothing there to win.

**It does not make a send O(1), and the reason is now measured rather than
reasoned.** Re-instrumenting the three stages in a temporary build (reverted;
`git diff` over `src/` carries none of it) and reading the first 50 posts of
the run against the last 50:

| stage | fresh channel | at the history cap |
|---|---|---|
| `find_task_by_context` | 34µs | **511µs** |
| `build_initial_task` | 4µs | 7µs |
| `persist_initial_task` | 30µs | 42µs |

Both stages this change targeted are now flat: `build_initial_task` went from
~300µs to 7µs and `persist_initial_task` from ~440µs to 42µs. What is left
is concentrated almost entirely in one place. `find_task_by_context` costs
**511µs at the cap, 15x its own cost on a fresh channel and roughly 73x
`build_initial_task`** — it lists and clones up to ten whole tasks, history
included, to pick one.

That settles the question the previous attribution left open. It was read from
source and said "which of the four clones dominates is unmeasured"; it is
`find_task_by_context`'s, and not narrowly. It is also the one blocked on an
API decision rather than on effort: its result becomes `RequestContext`'s
`stored_task`, a `pub` field documented as the executor's view of the previous
turn, so serving it without history changes what every user-written executor
sees.

The background processor's re-read at `background/mod.rs:84` survives too. It
is why the refactor did not corrupt anything — it is the reason a continuation
still accumulates history — and being off the request's critical path it costs
throughput rather than latency.

Two regressions this could have shipped, both found by predicting the failure
and writing the test rather than by the suite going green:

* **A continuation truncating the conversation.** 2,037 tests passed with the
  change in place, and *none of them* covered a second turn keeping what the
  first wrote. `a_continuation_keeps_what_earlier_turns_wrote` covers it now.
  The truncation turned out not to happen, for the `background/mod.rs:84`
  reason above — but nothing in the suite knew that.
* **`historyLength` returning one message.** This one was real. The send
  response is shaped from the send path's task, which now holds only this
  turn's message, so a client asking for ten got `["user-1"]`.
  `hydrate_response_history` reads the stored conversation, and only when
  history was actually asked for — `historyLength` defaults to omitting it, so
  the common send pays nothing.

### All three stores now override it, not just the in-memory one

The figures above are in-memory, which is what every arm of this experiment
drives. That left a gap worth naming, because for a while it was real: the
delta was overridden **only** in `InMemoryTaskStore`, so a SQLite or Postgres
deployment got nothing from it. No regression — the trait's default delegates
to `save` — but no win either, and neither this report nor the trait docs said
so. A reader running Postgres would reasonably have read the ratio above as
applying to them.

Both SQL stores now override it. Measured on this box, 100 status events on a
channel holding 200 messages, medians of five runs:

| store | `save` | `save_status_delta` | ratio |
|---|---|---|---|
| `InMemoryTaskStore` | 8,499µs | 639µs | 13.3x |
| `SqliteTaskStore` | 122,071µs | 37,216µs | 3.3x |
| `PostgresTaskStore` | 287,287µs | 120,104µs | 2.4x |

The SQL ratios are smaller for the same reason the artifact delta's are: both
keep one JSON document per row and still rewrite the row internally, so what
the delta removes is the Rust-side serialization of the whole task — history
included — and its transfer as a bind parameter, not the write itself.

Two columns are as much part of "what `save` would have left" as the document
is, and each has a test that fails when it is missed: the `state` column that
`list` filters on, and `updated_at`, which carries the §3.1.4 ordering key. A
delta that updates the document alone leaves the row correct and both
unfilterable and mis-ordered.

The SQLite override additionally must **not** delete its artifact journal,
though its `save` does. `save` deletes those rows because it has just
rewritten the document with every part in it; a status delta has not, so they
are still the only record of the appended parts. Copying `save` there would
have silently dropped them, which is why that divergence has a test of its own.

### Two things the delta broke, found by auditing it rather than by a test

Both were caught by reading the change back against the code it touched, not
by the suite, which is worth saying because the suite was green for both.

**A failed history save stopped repairing itself.** `process_event_bg`'s own
doc states the rule: "when a save fails, the in-memory `last_task` is reverted
to its previous state so it stays consistent with what's actually persisted."
Every branch obeys it except the agent-`Message` branch, which logged and
carried on — leaving the snapshot holding a message the store had refused.
That divergence used to be repaired by accident, because the *next* status
transition wrote the whole task and carried the orphaned message with it. A
status delta writes only the status, so the accident is gone.

The fix is the branch obeying its own contract: the refused message is popped
back off. Reinstating the accident was the alternative and is worse — the
store is the record, and a snapshot ahead of it is exactly the "phantom state"
the contract names.

Exposure today is nil, which is why no test caught it: the only store that
overrides `save_status_delta` is the in-memory one, whose `save` cannot fail,
and the SQL stores take the default that delegates to `save`. It would have
become live the moment a third-party store — the trait is unsealed and meant
for them — overrode the delta with a fallible backend.

**The TTL sweep lost its pacing.** `should_evict` advances a write counter and
fires the TTL pass every `eviction_interval` writes; both of this store's
memory bounds run off it. Only `save` and `insert_if_absent` advanced it, so
every transition the delta replaced became invisible to eviction — a
deployment whose writes are mostly transitions would have swept expired tasks
more and more rarely the better this method worked. The delta now advances the
counter and runs the sweep exactly as `save` does, which is what the second
measurement above already includes.

`save_artifact_delta` has the same gap and is **not** fixed here: closing it
changes the cadence of an already-shipped path and needs its own measurement
against the artifact benchmarks. It is worth doing and is not this change.

## What this means for building a coordination channel on A2A

* **Shard by context, at roughly 4–16 writers per channel.** Not by task:
  Finding 2 makes extra tasks in a context a lockout rather than a shard.
* **Never post without a `taskId`.** One untargeted post displaces the
  channel for everybody else, permanently, with an error message that does not
  say so.
* **Do not expect a live tail.** Readers of an append-rate channel poll with
  `Last-Event-ID`; the live stream only works when turns are long.
* **Loss is detectable, so build on that.** `streamLagged` is the signal that
  a reader must resynchronise, and it is delivered before the stream closes.
* **Budget about 3,300 posts/s for 1,000 agents on four cores**, sharded, with
  an in-memory store — and treat that as a ceiling, not a target.

## What is still unmeasured

* ~~The attribution in finding 6 is read from the source and supported by the
  history-cap experiment, but no profiler ran; which of the four clones
  dominates is unmeasured.~~ **Measured.** See the stage table under Fix 2:
  `find_task_by_context` dominates at 511µs against `build_initial_task`'s
  7µs and `persist_initial_task`'s 42µs at the history cap. Done by timing
  the stages directly rather than with a sampling profiler, which on an async
  runtime attributes to the executor rather than to the call that scheduled
  the work.
* Every figure here uses the in-memory store. The SQL stores write to a disk
  this experiment never touches; their append throughput is unknown.
* One replica. Nothing here says what a shared PostgreSQL store does when two
  servers write to the same channel.
* Agent-card fetch under swarm load, `ListTasks` pagination as a context fills,
  and push-notification config CRUD as a coordination path are all untouched.
* The `grpc` and `websocket` bindings. Only REST was driven.
