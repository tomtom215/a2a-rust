<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# In-memory task store

`InMemoryTaskStore` is the default task store: nothing to set up, gone when the process exits. Use it for development, tests, and agents whose tasks need not outlive the process. This page covers its limits, eviction and the `TaskStoreConfig` that tunes it. For one partition per tenant or caller, see [Tenant-aware stores](./tenants.md).

The default implementation with optional TTL and capacity limits:

```rust,no_run
use a2a_protocol_sdk::server::{InMemoryTaskStore, TaskStoreConfig};
use std::time::Duration;

// Default: 1hr TTL, 10k capacity
let store = InMemoryTaskStore::new();

// With custom limits
let store = InMemoryTaskStore::with_config(
    TaskStoreConfig::default()
        .with_task_ttl(Some(Duration::from_secs(7200))) // 2 hour TTL
        .with_max_capacity(Some(50_000)),
);
```

Features:
- Thread-safe (`RwLock` — concurrent readers, exclusive writers)
- Pre-allocated `HashMap::with_capacity(max_capacity)` — eliminates resize-induced latency spikes under load
- O(1) `get()` via `HashMap`; `save()` and `delete()` also maintain the ordered indexes, so they are O(log n)
- **O(log n + page\_size) `list()` with secondary indexes**:
  - `BTreeMap<(i64, u64), TaskId>` order index keyed by the task's status
    timestamp (Unix milliseconds) and a monotonic per-write sequence that
    breaks ties — iterated in reverse to return tasks most-recently-updated
    first (spec §3.1.4: sorted by status timestamp, descending), with no
    O(n log n) per-call sort
  - `HashMap<String, BTreeMap<(i64, u64), TaskId>>` context index — O(log m + page\_size) filtered queries
  - The pair is a collision-free pagination cursor (the page token is
    `millis:seq`); a write that does not change the status timestamp, such as
    an artifact append, keeps the task's place in the list
- Automatic TTL eviction on write (every `eviction_interval` writes, or when you
  call `run_eviction()`); a read never evicts, so `get` can still return a
  terminal task past its TTL until the next sweep (maintains all indexes)
- Capacity eviction (oldest terminal tasks first; falls back to non-terminal tasks when needed) when limit exceeded — `save` does not return with the store over capacity. Measured with 16 concurrent writers against a capacity of 100: a concurrent reader saw a peak of 102 and a final count of exactly 100, so the transient overshoot is bounded by the number of writes in flight
- Cursor-based pagination, most-recently-updated first
- Filtering by `context_id` (index-accelerated) and `status`

## `TaskStoreConfig` fields

**`TaskStoreConfig` configures `InMemoryTaskStore` only.** The SQL stores take
a connection URL, not a config — `max_capacity`, `task_ttl` and
`eviction_interval` have no meaning for them (the database holds the rows, and
retention is `purge_expired`; see
[Retention, Erasure and Audit](./retention.md)), and each carries
its own page-size cap via
`with_max_page_size`. Until 2026-08-19 that cap was a hardcoded `1000` that
happened to equal this table's default, so setting `max_page_size` to something
tighter changed the in-memory store and nothing else: measured with the cap at
10 and a client asking for 100, the in-memory store returned 10 and SQLite
returned all 60 rows it held.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_capacity` | `Option<usize>` | `Some(10_000)` | Max tasks in store; oldest evicted when exceeded |
| `task_ttl` | `Option<Duration>` | `Some(1 hour)` | TTL for terminal-state tasks; `None` disables eviction |
| `eviction_interval` | `u64` | 64 | Writes between automatic eviction sweeps (amortizes the sweep's cost) — see the note below |
| `max_page_size` | `u32` | 1000 | Maximum allowed page size for list queries |
| `max_events_per_task` | `Option<usize>` | `Some(512)` | Events one task's log keeps for stream resumption; `None` removes the bound |
| `max_held_events_factor` | `Option<usize>` | `Some(8)` | Times `max_events_per_task` a log may hold while an attached stream still owes its oldest events |
| `idempotency_key_ttl` | `Option<Duration>` | `Some(24 h)` | How long a client's idempotency key is honoured; `None` keeps keys for the life of the process |
| `max_idempotency_keys` | `Option<usize>` | `Some(100_000)` | Keys held at once; a send with a new key past it is refused as temporarily unavailable (HTTP `503`; gRPC `RESOURCE_EXHAUSTED`) |

### `eviction_interval` is a tail-latency knob

The TTL sweep is not spawned: it is awaited inside `save`, and it holds the
store's write lock for its whole duration, so the write that triggers it pays
for it and every concurrent writer waits.

The sweep walks only the tasks last written at least `task_ttl` ago (an index
ordered by write time) and every held idempotency key (`idempotency_key_ttl`),
so what it costs grows with the expired tasks and the held keys, not with the
store size. The interval keeps it off most writes. The cost of one sweep has
not been measured on this code.

If p99.9 write latency matters to you, raise `eviction_interval` and drive
cleanup yourself with `run_eviction()` on your own schedule, so the stall lands
where you chose it. The capacity pass is separate: it runs on any write that
leaves the store over `max_capacity` and removes only the overflow, oldest
first, rather than scanning for it.
