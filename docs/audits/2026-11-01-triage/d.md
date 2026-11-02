<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Slice d: resilience rows (D-03 … D-25), checked against main at 4a8243c

Baseline confirmed with `git rev-parse HEAD` (4a8243c66df5ae07a54a69168ab419ac609b06e9). Everything below comes from reading the code. Nothing was built, run or probed. Tests were read, not run.

## Counts (19 rows)

| Verdict | Rows |
|---|---|
| confirmed (7) | D-07, D-08, D-09, D-16, D-19, D-22, D-23 |
| partly_fixed (4) | D-03, D-12, D-13, D-21 |
| already_fixed (1) | D-04 |
| needs_decision (5) | D-05, D-10, D-11, D-18, D-20 |
| not_a_defect (2) | D-24, D-25 |

New findings: 3 (NEW-d-1 medium, NEW-d-2 low, NEW-d-3 low).

## The five most important

1. **D-03 and NEW-d-1, executor panic (medium).** Commit ee0a4c4 made a panic store `Failed` and push it on both send paths (`processor.rs:100-139`, `sync_collector.rs:489-511`). The tests pin it and would fail without the fix. Two parts are still open:
   - the panic's `Failed` is not written to the event log;
   - a `SendStreamingMessage` stream ends with no terminal frame (`respond.rs:178-181` attaches no reattach hook, and `in_memory.rs:1604-1610` ends the stream).

   Reading the event loop turned up something worse (NEW-d-1). The processor's `biased` select reads the persistence channel first and breaks on `None` (`background/mod.rs:185-203`). The CleanupGuard's spawned `destroy` drops the last writer, which closes that channel. If `destroy` runs before the processor is woken, `executor_panicked` never runs. On a current_thread runtime tokio queues FIFO, so `destroy` runs first and this is likely every time. The task stays `Working`, and no webhook is told. No end-to-end test covers it. The fix is XS.
2. **D-07, server WebSocket wedge (medium), with NEW-d-2 on the client side.** `send_json` has no timeout on the lock or the send. The busy reply and the binary-frame reply are awaited in `read_loop`, outside the select that watches `closing` and the idle budget. If a peer stops reading, it pins the loop, and shutdown abandons the connection after `drain_timeout`. On the client, `write_tx.send(..).await` runs before any timeout starts. Calls therefore hang without bound against a peer that is not reading. This SDK's client never stops reading (`route_frame` only uses `try_send`), so the trigger has to be another peer.
3. **D-21, rolling deploy (medium; half fixed by E-F2).** Turns are no longer `Canceled`, and sends are refused with a retry delay. Every turn still running after 5 s still ends `Failed`/transient on every deploy, on all three listeners. The work is lost, and delegation children are cancelled (the shutdown test asserts this). No hand-off exists. The reaper also fails orphaned turns, so a crash is no longer the better outcome either. The fix is a design decision. Two doc comments still say `Canceled` (`serve/graceful/config.rs:48-51`, `in_flight.rs` ~197).
4. **D-05, push durability (medium, needs_decision).** Delivery is still a few in-memory attempts with no delivery id. A 404 or 410 config is kept and paid for on every later event. Lanes now also drop deliveries under backlog. Recommendation: add an `A2A-Delivery-Id` header and document the semantics now, add a dead-config policy next, and write an ADR for an outbox.
5. **D-08 and D-20, liveness defaults (medium/low).** The WebSocket client sends no ping and sets no TCP keepalive, and a timeout does not mark the connection dead. The server's WebSocket idle timeout and gRPC keepalive are off by default, and tests pin them as off. With defaults, neither end of a WebSocket ever detects a half-open peer. For D-20 I recommend turning the ping-based bounds on by default.

## Other notes

- **D-13**: the reserved ranges and the trailing-dot bypass are fixed and edge-tested (8bdf383). `https` is still unpinned (`pinning.rs:76-84`). Pinning it with a resolver that returns the validated address would also remove one `getaddrinfo` from D-22.
- **D-22**: the new PushGate bounds concurrent deliveries. It releases its permit when a delivery times out, while the `getaddrinfo` thread keeps running, so leaked blocking threads still pile up. NEW-d-3: the per-destination bound is keyed by host name, so wildcard DNS defeats it.
- **D-16**: the fix has to map to `ServerError::Overloaded`. `not_retry_safe` (`messaging/mod.rs:106`) deliberately strips the unavailable flag from errors on the send path.
- **D-19**: anyone can reach it without authenticating when audit is on and the deployment uses either a header tenant resolver or tenant-aware stores with no resolver. `isolatable()` refuses only control characters.
- **D-23**: a third site the report missed, `CommitGuard::drop` (`commit_guard.rs:182`).
- **D-09**: the TLS sub-claim is wrong for this build. tokio-tungstenite 0.30 reports rustls failures as `Io`, which the client retries. The connect timeout, an incomplete handshake and a handshake `Retry-After` are still mishandled.
- **D-04**: fixed and tested through `deliver_push_bg`. The blocking path's call site has no test of its own.
- **D-12**: the 50 s idle timeout was fixed in 57f034e, before the README's last edit (B-07), but no test pins it. Max lifetime, a per-host cap and HTTP/2 keepalive are left to proposed ADR 0032.

## Not checked

- No test was run, so every "would fail without the fix" judgement comes from reading the assertions. That includes NEW-d-1: the scheduling argument is read from tokio-1.54.1's harness and current_thread sources and is not reproduced. A two-line `#[tokio::test]` settles it.
- The live probes listed in d.json were not run: D-08 (blackholed flow), D-13 (rebinding DNS), D-22 (blackholed resolver), D-21 hand-off.
- D-11: I did not re-read the delegation resume tests (59e6ff0, 26ca4f1).
- D-24: the slicing sample covers the request-path hits from a scan whose script is not kept in the repository, not all 47.
