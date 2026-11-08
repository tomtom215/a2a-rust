<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Hostile campaign, wave 2 — area HOSTILE

Adversarial testing of a2a-rust against `main` fast-forwarded to `a746210f`
(`test(stores): the in-memory page-token tests get their own file`), this
worktree's branch `worktree-agent-acea2d724feafed58` (tracking
`claude/affectionate-gauss-5yn4f5`). Target: hostile peers, hostile clients,
auth and tenant bypass, SSRF, replica chaos on PostgreSQL. Everything below is
re-runnable from the scripts in `scripts/live/campaign/hostile/`; this file
only reports outcomes and points at the evidence.

**This is not a release-readiness verdict.** It is one area's worth of
adversarial input against one build of the server; sections below name what
was not attacked and why.

## Setup

```
export A2A_HOSTILE_TARGET_DIR=<your CARGO_TARGET_DIR>   # build output only, not committed
CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=$A2A_HOSTILE_TARGET_DIR cargo build \
  --manifest-path scripts/live/campaign/hostile/target/Cargo.toml
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=$A2A_HOSTILE_TARGET_DIR cargo build \
  --manifest-path scripts/live/campaign/hostile/client/Cargo.toml   # only for h12
export A2A_HOSTILE_BIN=$A2A_HOSTILE_TARGET_DIR/debug/a2a-hostile-target
export A2A_HOSTILE_CLIENT=$A2A_HOSTILE_TARGET_DIR/debug/a2a-hostile-client   # only for h12
export A2A_LIVE_PG=postgres://postgres:postgres@127.0.0.1:5432   # or your own
python3 -I scripts/live/campaign/hostile/<script>.py [args]
```

Every script aims only at listeners it starts on `127.0.0.1`. PostgreSQL
scripts create and drop their own `a2a_camp_*` databases. `target/` is one
configurable binary standing in for every deployment shape the campaign
attacks (store, auth, tenancy, steering, push guard — see its `main.rs` doc
comment for the full environment-variable table); `exec.rs` is the
misbehaving executor whose message text selects the behaviour (panic after N
events, no terminal state, blocks forever, steers, and so on — see its doc
comment).

## Findings

| ID | Severity | Class | What | Status |
|---|---|---|---|---|
| W2-hostile-1 | high | hostile input → SQL | `ListTasks` page token with a `0000-01-01`-shaped `updated_at`, or a NUL in its id, reached `PostgreSQL`'s `::timestamp` cast/bind and came back `-32603` carrying the database's own message | **FIXED** (commit below) |
| W2-hostile-2 | high | hostile input → SQL | `statusTimestampAfter` (and a task's own status timestamp) with a `0000-*` year reached `::timestamptz` the same way | **FIXED** (commit below) |
| W2-hostile-3 | high | replica chaos, resilience | a `CancelTask` that lands on a replica other than the one running the turn, exactly as that replica is shutting down, falls back to a direct `tasks`-table write that never appends a terminal event to the log and never releases the turn claim: a permanent orphan claim (the reaper's own query excludes terminal tasks) and a cancellation no event-log consumer (push outbox, replay, audit) ever sees | **DESCRIBED, NOT FIXED** — design decision, see below |
| W2-hostile-4 | medium | hostile input | a `SendMessage` whose `extensions` array or `metadata` key carries a NUL (`\u0000`) succeeds on the in-memory, SQLite and tenant-in-memory stores but answers `-32603` on every PostgreSQL store (`jsonb` cannot hold a NUL): a hostile string the handler does not refuse reaches the database and comes back as an internal error, and the five stores disagree on the same input | not fixed — see below |
| W2-hostile-5 | low | observability | ten HTTP+JSON routes a `BearerTokenAuthInterceptor` protects answer an unauthenticated caller `200`/`404`/`204` instead of `401` before the method/route is resolved (unknown JSON-RPC method, malformed JSON, oversized body, a REST 404, `GET /metrics`): no data crosses the boundary, but the shape of the answer differs from the reference `401`, so an unauthenticated probe can tell a live route from a dead one, and a parse error from a method-not-found one | not fixed — see below |

Every other class attacked (below) held: the SDK answered as documented, with
a bounded refusal, no panic, no leak, and resources back near baseline.

### W2-hostile-1, W2-hostile-2: year-0000 timestamps reaching PostgreSQL's cast

**PROVEN.** Commands:

```
A2A_HOSTILE_BIN=<bin> python3 -I scripts/live/campaign/hostile/h1_inputs.py pg tpg
```

Before the fix, against `pg`: `rpc list.pageToken.year0000` → HTTP 200,
`{"error":{"code":-32603,"message":"internal error (reference …)"}}`; the
server log (`evidence/h1_pg.target.log`, pre-fix capture folded into the
session transcript) carries PostgreSQL's own `date/time field value out of
range: "0000-01-01 00:00:00"`. Likewise `rest list.statusTimestampAfter.2`
(`"0000-01-01T00:00:00Z"`) → HTTP 500. `tpg` and `enc-pg` showed the same.

Cause: `store/cursor.rs`'s `is_timestamp` and `store/mod.rs`'s
`status_timestamp_rfc3339` both delegate to
`a2a_protocol_types::parse_iso8601_to_unix_millis`, whose range starts at year
0000 (`crates/a2a-protocol-types/src/lib.rs:215`, the first year the proleptic
Gregorian calendar can name) — but `PostgreSQL`'s `timestamp`/`timestamptz`
range starts at year 0001. A token or filter value in that one-year gap passed
every check in the crate and reached `::timestamp`/`::timestamptz`, which
PostgreSQL itself refuses. This is exactly the class the `ListTasks`
page-token fix (`02ab23e4`) was closing for a different shape (a nonsense
timestamp with no real calendar date); year 0000 *is* a real calendar date to
this crate's own parser, which is why that fix did not close it.

A NUL byte in a page token's id (`store::cursor::decode`) was also unguarded:
`PostgreSQL` cannot bind a NUL in a text parameter, so
`list.pageToken.nul_id` also answered `-32603` with the database's own
message.

**Fix** (test-first, confined to `crates/a2a-protocol-server/src/store/cursor.rs`
and `crates/a2a-protocol-server/src/store/mod.rs`):

* `store/cursor.rs`: `decode` refuses an id containing `\0`; `is_timestamp`
  refuses an `updated_at` starting `"0000"`.
* `store/mod.rs`: `status_timestamp_rfc3339` refuses (returns `None`, the same
  as any other unparseable value — a list filter then matches nothing, a save
  falls back to the write wall-clock) an instant whose canonical form starts
  `"0000"`.

New unit tests (same two files, fail before the fix, pass after — verified by
hand-reverting the production code with the tests left in place and watching
them fail for the claimed reason, then restoring):

* `store::cursor::tests::an_id_with_a_nul_is_rejected`
* `store::cursor::tests::a_separator_with_no_timestamp_before_it_is_rejected`
  (two new cases: `"0000-01-01 00:00:00"`, `"0000-12-31 23:59:59.999999"`)
* `store::status_timestamp_tests::rfc3339_refuses_a_year_postgres_timestamptz_cannot_hold`

Gates run after the fix, all green:

```
cargo fmt -p a2a-protocol-server
cargo clippy -p a2a-protocol-server --all-targets --features postgres,sqlite -- -D warnings
cargo test -p a2a-protocol-server --lib --features postgres,sqlite status_timestamp cursor
A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo test -p a2a-protocol-server --test postgres_store_tests --features postgres -- --ignored --test-threads=4
  # 77 passed, 0 failed — including the two tests this exact class already had
  # (a_malformed_page_token_is_invalid_params_and_a_deleted_tasks_token_resumes,
  # task_list_status_timestamp_ordering_and_filter)
bash scripts/check_file_lengths.sh
```

Re-run of `h1_inputs.py pg tpg` after the fix: both timestamp-shaped
deviations gone; 750/752 held per store (the remaining 2 are W2-hostile-4,
below). `git diff --stat`: `cursor.rs` +40/-8, `mod.rs` +46/-6. Not run: a full
workspace `cargo test` (out of budget here; the targeted test selection above
is what the brief's fixing discipline asks for) and `cargo mutants` on this
diff (RUSTFLAGS unset, as required — not run for time; the maintainer's own
incremental mutation gate on the real PR diff is the authoritative run).

### W2-hostile-3: cross-replica cancel during a shutdown hand-off loses the terminal event and the claim

**PROVEN**, reproduced twice (`c1_chaos.py cancel_handoff`, and a standalone
probe kept long enough to rule out delayed cleanup). Not fixed: it spans the
cancel handler, the event-queue/log-append path and the claim reaper, and the
right answer needs a decision the brief says to describe and not make.

Reproduction: 3 replicas (`CLAIM_TTL_MS=3000`), 10 `hold` tasks started on
`r0`; `r0` sent `SIGTERM` (graceful shutdown) while another thread hammers
`CancelTask` for all 10 task ids against `r1` (which never held their
claims). After 8 s:

```sql
select id, state, applied_seq from tasks;                 -- all TASK_STATE_CANCELED, applied_seq = 1
select task_id, seq, payload from task_events order by 1,2;  -- ONE row per task: the initial "working" event; no terminal row
select owner, expires_at, cancel_requested from a2a_task_claims;  -- one row per task, cancel_requested = true, never deleted
```

Invariants (`chaoslib.invariants`, SQL not eyeballing): `tasks_with_no_terminal_event=10`,
`orphan_claims_on_terminal_tasks=10`. Waiting past the claim TTL and several
reaper intervals does not clear them: the reaper's own query
(`crates/a2a-protocol-server/src/store/claim/postgres.rs:56,65`,
`ABANDONED_PLAIN_SQL`/`ABANDONED_TENANT_SQL`) is
`WHERE t.state IN ('TASK_STATE_SUBMITTED', 'TASK_STATE_WORKING')`, which by
design excludes a task that is already terminal — so a claim left behind on a
terminal task is never reaped. This is permanent, not slow.

Cause, read in source: `handler/lifecycle/cancel_task.rs:108-126` signals the
in-process cancellation token only when the turn runs on *this* replica
(`runs_here`); when it does not, it builds a `TerminalTracking` over
`event_queue_manager.writer_for_cancel`, which the code's own comment
(line 114) calls "a throwaway writer" with no live queue to append through.
It then writes the terminal status with
`task_store.save_status_delta(&updated)` (line 158), and
`PostgresTaskStore::try_save_status_delta`
(`store/postgres_store/store_impl.rs:340-370`) is purely an `UPDATE tasks SET
data = …, state = …` — it has no `INSERT INTO task_events` and no
`DELETE FROM a2a_task_claims` anywhere in it. In the *plain* cross-replica
cancel case (both replicas alive, no shutdown race — reproduced separately,
`probe6.py`), this path is not the one that wins: the owning replica notices
`cancel_requested` on its own next claim renewal (every `CLAIM_RENEW_MS`),
locally cancels its own token, and finishes its turn through the normal
in-process path, which *does* append the event and release the claim — all
usually faster than the cross-replica handler's fallback write. The bug is
specifically the race against that replica's own shutdown: when it cannot get
to its next renewal before exiting, the fallback wins, and the fallback is
missing two things the normal path has.

What this means operationally: a webhook or audit consumer that relies on the
event log (the outbox pattern the brief's invariants describe — "every
terminal event reaches the log … at-least-once") never learns this
cancellation happened; and the claim table grows one row per such race,
forever, with no sweep that will ever remove it.

Options, not decided here:

1. Make the cross-replica cancel's fallback (`cancel_task.rs`'s
   `save_status_delta` branch) also append a terminal event to the log and
   release the claim directly, by calling the store's event-log append and
   `claims.store.release` explicitly rather than relying on a throwaway
   writer. Confined to that one function, but changes what "throwaway" means
   (the comment's own reasoning — no live queue to serve a stream through —
   would need re-examining: an append to the log does not require a live
   in-memory subscriber).
2. Widen the reaper's `ABANDONED_*_SQL` to also find (and release) claims
   whose task has *already* gone terminal by some other path, independent of
   whether it also backfills the missing event — a narrower, additive fix for
   the orphan-claim half only.
3. Treat this as a push/outbox-level question instead: have the outbox (or a
   periodic reconciliation) watch `tasks.updated_at`/`state` directly for
   tasks whose last logged event is not terminal, rather than assuming the
   event log is always complete.

My recommendation is (1) for the claim leak (it is the SDK's own resource,
and the reaper's terminal-exclusion is correct as a general rule — the gap is
this one write path not doing its half of the job) and a maintainer decision
on whether (1) alone is enough for the missing event, or whether (3) is
needed regardless because *some* path will always be able to skip the log
(a process that dies between the two writes, for instance).

Evidence: `evidence/c1_cancel_handoff.jsonl`
(`cancel_handoff: invariants` row), `evidence/c1_steer.jsonl` (the *working*
contrast case — a reaper-driven `ClaimLapsed` failure, same fleet shape,
*does* log its terminal event and release its claim cleanly: `steer:
invariants` shows `tasks_with_no_terminal_event: 0`). The plain
(non-shutdown) cross-replica cancel control run (`probe6.py`, not committed —
reproduce with the command in `c1_chaos.py`'s `cancel_handoff` minus the
`SIGTERM`) showed the event logged and the claim released within 1.5 s.

### W2-hostile-4: NUL in a message field reaches `jsonb` on PostgreSQL only

**PROVEN**, not fixed (a design decision — see below). Command:

```
A2A_HOSTILE_BIN=<bin> python3 -I scripts/live/campaign/hostile/h2_nul_json.py
```

A `SendMessage` whose `message.extensions[0]` or whose `message.metadata` key
contains `\u0000` succeeds (200, a normal task) on `mem`, `sqlite`, `tmem`,
`tsqlite`, `enc-mem`; on `pg`, `tpg` and `enc-pg` the identical request
answers `-32603` and **no task is created** (`tasks_created: 0`) — the
executor never runs, because the write that would have persisted the initial
task fails first. Nine of fourteen fields tested fail this way on PostgreSQL
(`text`, `part.metadata.value/key`, `part.filename`, `part.data`,
`part.url`, `message.metadata.value/key`, `message.extensions`); the
idempotency key and `role` field did not reproduce it in this run (the key's
own validator and the role enum presumably reject a NUL before the store is
reached — not separately confirmed here, say so).

Cause: `PostgreSQL`'s `jsonb` cannot hold `\u0000` inside a JSON string (a
long-standing `PostgreSQL` limitation, not a bug in this crate's SQL); nothing
in the handler's message validation (`handler/messaging/validation.rs`)
refuses a NUL in these fields the way `handler/ids.rs` already refuses one in
an *id*. Unlike W2-hostile-1/2, this is a design choice with real trade-offs,
not a one-function fix:

* reject a NUL in every message field at ingress, uniformly, for every store
  (consistent, but changes what every store accepts, including ones that
  handle it fine today);
* reject it only in the PostgreSQL-backed stores' write path (keeps other
  stores' current behaviour, but the five stores would keep disagreeing on
  what a client may send, which is itself the class this whole hunt is
  about);
* leave it, documented as a known PostgreSQL limitation.

Not fixed here per the brief ("Fixes that need a design decision … do not
make them"). Evidence: `evidence/h2_pg.jsonl`, `evidence/h2_tpg.jsonl`,
`evidence/h2_enc-pg.jsonl`, `evidence/h1_pg.jsonl` rows
`Send.extension.nul`/`Send.metadataKey.nul`.

### W2-hostile-5: unauthenticated callers get route-shaped answers, not a uniform 401

**VALIDATED** (observed; severity judged low — no data crosses the auth
boundary, only the *shape* of the refusal). Command:

```
A2A_HOSTILE_BIN=<bin> python3 -I scripts/live/campaign/hostile/h10_preauth.py
```

With `BearerTokenAuthInterceptor` and no credential, a `GetTask` with a known
method answers the reference `401 {"error":{"code":-32600,"message":
"authentication required"}}`. But: an unknown JSON-RPC method → `200`,
`-32601` "Method not found"; invalid JSON → `200`, `-32700` parse error; a
body over the 4 MiB limit → `200`, `-32700` size error; an unversioned/wrong
`A2A-Version` → `200`, `-32009`; a REST route that does not exist, or exists
but with the wrong HTTP method → `404` (not `401`); `GET /metrics` (not
served in this build) → `404`. In every one of these the auth interceptor
never ran: method dispatch, body parsing and version negotiation happen
before authentication on at least the JSON-RPC path tested. An unauthenticated
prober can therefore distinguish "this route exists" from "this route does
not", and "my JSON was broken" from "my method name was wrong", without ever
presenting a credential — a (small) information leak about the server's
surface, and arguably a spec-adjacent ordering question (the brief's own
"auth-before-params" framing). 12 of 24 cases in the probe differed from the
reference this way; `OPTIONS`, `HEAD` and the public `/health`, `/ready`,
`/.well-known/agent-card.json` routes are correctly public and excluded from
the count. Not fixed: changing dispatch order to authenticate before
parsing/routing is a design decision with its own trade-offs (it also makes
every malformed-JSON probe cost the throttle's or JWKS's per-request budget),
so this is reported, not changed. Evidence: `evidence/h10_preauth.jsonl`.

## Classes attacked, by group (count tried / held / deviation)

Full detail and raw attack records are in `evidence/<script>_<store>.jsonl`
(one JSON line per attack; a trailing `{"tally": …}` line per file). "Held"
means the SDK answered as documented: a bounded refusal with the right class
of error, no panic, no crash, no leak, resources recovering.

| Script | Class | Tried | Held | Deviation |
|---|---|---|---|---|
| `h1_inputs.py` (8 stores × 94 cases) | hostile ids/numbers/timestamps/tokens across `ListTasks`, `GetTask`, `CancelTask`, push-config CRUD, `SendMessage` fields, over JSON-RPC and REST | 6016 | 6010 | 6 (W2-hostile-4 only — figures below are the post-fix, consistent re-run; pre-fix this run is what found W2-hostile-1/2, 24 deviations across `pg`/`tpg`) |
| `h2_nul_json.py` (8 stores × 14 cases) | NUL inside JSON-string fields of a message | 112 | 91 | 21 (W2-hostile-4; `pg`/`tpg` 9 each, `enc-pg` 3) |
| `h3_steering.py` (mem, pg) | steering: non-owner, cross-tenant, flood past the inbox bound, terminal/cancelled task, racing the terminal transition, duplicate ids, undeclared extension, oversized messages | 44 | 44 | 0 |
| `h4_streams.py` (mem, 1 and 100 subscribers) | F14: panics (0/3/50 events in), no-terminal-state return, queue closed early, writes after terminal, error return, 2000-event burst, 1h block vs. the executor timeout | 22 | 21 | 1 (see below) |
| `h5_transport.py` | HTTP/1.1 framing: `Content-Length` lies (2 GB, −1, non-numeric, mismatched, duplicate), chunked abuse (overflow size, 1 MB extension, 100 000 chunks, bad trailer), 10^6-deep JSON nesting, duplicate JSON keys, invalid/overlong UTF-8, 10 000 headers, 100 KB/10 MB header values, slowloris (150 headers, 50 bodies), request smuggling shapes, `TRACE`/`CONNECT`, 5000 pipelined requests | 59 | 54 | 5 (see below) |
| `h6_h2_ws.py` (h2, ws, sse, idle) | HTTP/2 rapid reset, CONTINUATION/SETTINGS/PING/PRIORITY floods, oversized frame, zero-window; WebSocket fragmentation+ping interleave, 16 MB frame, 64×1 MB fragment bomb, ping flood, binary/unmasked/invalid-UTF-8/reserved-bit/control-frame violations, a 200 000-event stream nobody reads; SSE 50 non-readers on a 300 000-event task; 10 000 idle connections | 32 | 32 | 0 |
| `h7_zombie.py` | an executor that returns `Ok` with no terminal state: 50 subscribers held, the per-task cap (256), `CancelTask` on the zombie | 8 | 8 | 0 |
| `h8_ssrf.py` | 61 webhook URL spellings (decimal/octal/hex/short IPv4, mapped/compat/NAT64/6to4 IPv6, userinfo tricks, percent-encoding, fullwidth/ideographic digits, zone ids, `file://`/`gopher://`/`ftp://`/`ws://`/`data:`/`javascript:`, a name mapped to 127.0.0.1 in `/etc/hosts`, a redirect to a second listener), standalone and inline push config | 61 | 61 | 0 |
| `h9_auth.py` (JWT, static bearer, API key) | `alg:none` (4 spellings), wrong/empty HMAC secret, HS384/512 and RS/ES/PS/EdDSA against an HS256 validator, RSA-public-key-as-HMAC-secret, RS256 self-signed (no JWKS), `crit`/`b64:false`, kid injection (6 payloads), duplicate `alg` keys, malformed base64, expired/not-yet-valid (with and without leeway), wrong/array/no issuer or audience, header shape (case, whitespace, two headers, query string, cookie), auth-before-params, oversized credentials, static bearer/API-key edge cases | 104 | 99 | 5 (see below) |
| `h10_preauth.py` | what an unauthenticated caller can learn from the shape of the refusal | 24 | 12 | 12 (W2-hostile-5) |
| `h11_notifications.py` | JSON-RPC notifications and batches: unauthenticated/wrong-token/cross-caller `SendMessage` and `CancelTask` notifications, batches mixing a notification with a request, a 1000- and a 200-request batch | 11 | 11 | 0 |
| `h12_hostile_servers.py` | hostile servers against the client: 10 MB card, card naming another host, infinite stream, wrong-task terminal event, no-blank-line SSE, 200 MB gzip bomb, garbage-200 server, slow-drip body, redirect loop, never-ending chunked | 10 | 10 | 0 |
| `h13_parked_expiry.py` (pg) | parked-task expiry: a parked task past `parked_max_age` is deleted; a working (claimed) task is never swept; a subscriber on a swept task gets a definite end | 4 | 3 | 1 (harness timing, see below — not claimed as a finding) |
| `h14_throttle_xff.py` | `AuthFailureThrottle` with a PostgreSQL shared counter, rotating `X-Forwarded-For` with and without a trusted hop | 3 | 3 | 0 |
| `c1_chaos.py` — `kill9` | `SIGKILL` a replica holding 20 of 60 claims mid-turn | 4 | 4 | 0 |
| `c1_chaos.py` — `sigstop` | `SIGSTOP` a replica past 4× its claim TTL, then `SIGCONT` (the zombie that wakes and writes) | 2 | 2 | 0 |
| `c1_chaos.py` — `terminate` | `pg_terminate_backend` every connection to the database, 12 times over a run | 2 | 2 | 0 |
| `c1_chaos.py` — `pool` | 300 concurrent blocking sends against a pool of 2 | 3 | 3 | 0 |
| `c1_chaos.py` — `dbpause` | the database link paused 8 s mid-turn, then cut and restored | 4 | 4 | 0 |
| `c1_chaos.py` — `many30` | 30 replicas (pool 1 each) started at once on one database | 2 | 2 | 0 |
| `c1_chaos.py` — `tenk` | 10 000 tasks over 3 replicas | 2 | 2 | 0 |
| `c1_chaos.py` — `steer` | steering through a replica that does not hold the claim, then after the holder is `kill -9`'d until its claim lapses | 5 | 5 | 0 |
| `c1_chaos.py` — `cancel_handoff` | a task cancelled while its replica hands turns back on `SIGTERM` | 2 | 1 | 1 (W2-hostile-3) |

**Cross-store comparison, `h1_inputs.py`** (the brief's own instruction:
"compare the five stores' answers to the same hostile input; any difference
is a finding"). Comparing all eight stores' answers to the same 752 cases
each (`evidence/h1_<store>.jsonl`, same attack key): 44 of 752 differ, in two
groups, both read in source and resolved as *not* findings:

1. A page-token string shaped for the SQL stores' cursor format
   (`updated_at\nid`, e.g. the `leap`/`y9999`/`ancient`/`long_id` cases) is
   rejected by the in-memory stores (`-32602`) and *accepted, with no error*,
   by the SQL stores. Not a bug: the in-memory store uses a completely
   different cursor encoding (`millis:seq`,
   `store/task_store/in_memory/mod.rs:607`, gated out of the SQL stores'
   shared `store::cursor` module, itself `#[cfg(any(feature = "sqlite",
   feature = "postgres"))]`), so a string in the *other* format simply does
   not parse there. On the SQL side, accepting a syntactically valid
   `(updated_at, id)` pair the store never literally wrote is the intended
   semantics of a *position* cursor (not a capability token): the
   `is_timestamp`/`decode` guard exists to keep a malformed string from
   reaching the database as a cast or a bind, which it does; it was never a
   promise that only a server-issued token resumes a listing, and nothing in
   `store/cursor.rs` or the handler claims otherwise.
2. `tenant` with a hostile string (emoji, RTL mark, SQL-looking text, …) on
   `ListTasks` answers `-32004` (`UnsupportedOperation`) on `mem`/`sqlite`/
   `pg`/`enc-mem`/`enc-pg` and `-32602` (`InvalidParams`) on `tmem`/`tsqlite`/
   `tpg`. Not a bug: the first group does not support per-request tenant
   routing at all (any value is unsupported, regardless of content), the
   second validates the value it tries to resolve a tenant from — the
   difference tracks the stores' declared capability, not the hostile
   content.

**h5's five deviations, not claimed as SDK defects** (recorded for
completeness; each is a case my own expected-status list was too narrow, not
a verified bug): `CL declares 2 GB, sends 10 bytes` and `Expect:
100-continue then 3 GB` both answer `200` with a JSON-RPC `-32700` body-too-
large error rather than an HTTP 413/400 — correct behaviour for this
binding's style of refusal (an application-level parse error, which the brief
lists as an acceptable shape), my check only allowed the bare HTTP codes.
Two chunked-framing probes (`overflow`, `1 MB extension`) got the same
correctly-shaped `-32700`. `batch of notifications` got `204 No Content`
(correct: an all-notifications batch gets no body), which my check did not
list. None of these reached a 5xx, leaked anything, or left the process
unhealthy.

**h9's five deviations**, none a finding on re-reading the source: `token of
1 MB of dots` answers `431` (correctly refused as an oversized header), which
my check's expected-status list had excluded. `token with trailing space` and
`two spaces after Bearer` are both *accepted* — `auth/mod.rs`'s own
`extract_bearer` (`rest.trim()` after splitting on the first space,
`crates/a2a-protocol-server/src/auth/mod.rs:397-405`) deliberately tolerates
incidental whitespace around the token; the exact credential is still
required, so this is documented leniency, not a bypass, and my check wrongly
expected strict whitespace. The last two
(`auth-before-params: no credentials, unknown method …` and `… an invalid
credential with a bad method …`) are not a new finding: they are the same
class `h10_preauth.py` already reports in full as W2-hostile-5, surfacing
here too because `h9_auth.py` happens to probe one unauthenticated call with
an unknown method along the way.

**h4's one deviation**: the final "resources back near baseline" check, after
the full 22-case battery (12 cases at 1 subscriber, 10 at 100 — see the
scoping note below), found RSS at 106,792 KB against a 22,888 KB baseline
(my check's bound was 3×; file descriptors returned exactly to baseline,
10 vs 10). Not claimed as a leak: the battery opened on the order of 1,000
short-lived subscriber connections and twice drove a 2,000-event stream, and
this target (unlike `examples/deploy-agent`) does not use `jemalloc` — the
project's own documentation (`examples/deploy-agent/Cargo.toml`'s comment)
is explicit that glibc's allocator is the one that "keeps a fixed-size heap
full of freed holes" under this traffic shape, which is RSS that is not
reclaimed rather than RSS that is leaked. Descriptors returning to baseline
and the process staying healthy for the rest of the campaign (run after this
case with no restart) are the stronger signal; not re-measured with
`jemalloc` or over a longer idle period to confirm the memory is in fact
reclaimable, which would be needed to fully rule out a leak.

**h4's scoping limitation**: `noterm`/`closeq` (an executor that returns
without a terminal state) never end on their own, and with 100 concurrent
subscribers this harness's own task-id discovery (polling `ListTasks` for
the sent text while 99 more connections race to attach) hung past any
reasonable bound in testing — not diagnosed further given the time budget.
These two behaviours were driven at 1 subscriber only (where they completed
correctly, matching the documented `EOF` after `executor_drain_timeout` +
`subscribe_replay_catchup`, ~7 s) and skipped at 100; say so rather than
silently narrowing the claim. Whether many subscribers on a never-terminal
task behaves correctly was therefore not established at the higher
subscriber count that `h7_zombie.py` *did* drive successfully for a related
case (50 subscribers on a zombie task, held) — the difference being 100
concurrent subscribers racing the *open* rather than 50 already attached.

**h13's one deviation**: the subscriber-on-a-swept-task case closed cleanly
with a terminal frame, but at 10.3 s against my own 8 s check (the check loop
blocks on `recv()` with a 10 s socket timeout, so it can overrun its own
budget by construction). Not re-run with a corrected bound; recorded as a
harness artifact, not a finding.

## Attacks that failed to run (tool or environment, not a result)

* **cargo-fuzz**: nightly is not installed and could not be fetched in this
  environment (only `stable` and `1.94.0`); fuzz targets were not driven.
  Nothing in `fuzz/fuzz_targets/` was run as a substitute — the hostile-input
  matrix (`h1_inputs.py`, `h2_nul_json.py`) covers the same class (caller
  strings reaching SQL/JSON) by direct enumeration instead.
* **h2spec**: not installed; the HTTP/2 class was attacked with hand-built
  frames (`h6_h2_ws.py`'s `h2()` group) over a raw socket instead of a
  conformance harness.
* **toxiproxy**, **faketime**: not installed/used; `c1_chaos.py`'s own
  `TcpProxy` (asyncio, in `chaoslib.py`) stands in for the database-link
  pause/cut cases, and clock-skew-between-replicas was not attacked (no
  faketime, and PostgreSQL's own clock is shared by all replicas in this
  single-database setup, so a meaningful skew test needs either faketime per
  process or a second database — neither built here).
* **`many60`** (60 replicas) and a **1,000,000-key** throttle run: not run,
  time budget.
* **gRPC and WebSocket steering**: `handler/claims/steering.rs` carries any
  `SendMessage`, so the JSON-RPC path exercised should cover the dispatch
  logic, but a steering message sent *as* a gRPC or WebSocket request body was
  not separately driven (the handoff's own `docs/handoff.md` already flags
  this as untested).
* **SLIMRPC**: not attacked; this build does not enable it and the binding is
  flagged elsewhere as not yet wired into this campaign's target.
* **E-F25(b) `LISTEN`/`NOTIFY` abuse** (a dying listener, a pooler in
  transaction mode, payload injection into the channel, 200 subscribers, 5000
  appends/s): the target wires `LOG_NOTIFY=pool|direct`, but no script
  attacked it this wave — time budget. The explicit "does the failure stay
  loud and bounded under a transaction-mode pooler" question from the brief
  was not answered.
* **Clock skew between replicas**, **the table upgraded from the 0.15 shape
  with live rows** (both named under E-F15): not attacked.
* **mTLS identity from a forwarded-cert header** (`ForwardedClientCert`,
  spoofed `X-Forwarded-Client-Cert`): not attacked this wave.
* **Timing of the constant-time comparison itself**: `h9_auth.py` records
  *which* comparison method is used (`subtle::ConstantTimeEq`, per
  `auth/mod.rs:107-115`) by reading the source, not by measuring timing; no
  statistical timing-channel measurement was run.

## Commits

* `5667ac86` (`fix(stores): a page-token or statusTimestampAfter in year
  0000 is InvalidParams, not a PostgreSQL error`) — the two timestamp fixes
  above, with their tests; this directory, the evidence, and the campaign
  scripts under `scripts/live/campaign/hostile/` are the same commit, on this
  worktree's branch `worktree-agent-acea2d724feafed58`.

## Scripts and evidence

* `scripts/live/campaign/hostile/` — `lib.py`/`chaoslib.py` (shared harness),
  `target/` and `client/` (the Rust binaries the scripts drive, built outside
  the workspace like `scripts/live/replica`), `h1`–`h14` and `c1_chaos.py`
  (one script per class above), `smoke.py` (a two-line sanity check).
* `docs/audits/2026-11-07-campaign-wave2/hostile/evidence/` — one
  `<script>_<store/group>.jsonl` per run (attack, verdict, detail) and a
  `.target.log` per target process (its stderr).
