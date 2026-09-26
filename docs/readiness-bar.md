<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Readiness bar

What "ready for production use" would have to mean for this SDK, written
down **before** anything was measured against it, so the thresholds could not
be tuned to the numbers. Written 2026-09-24. Measurements against it are
recorded in the section at the end, each with the commit, hardware, build
profile and command that produced it; a row with no measurement is unmet, not
assumed.

This is a bar for the **server and client crates as a library** inside a
deployment someone else operates. It says nothing about a particular agent's
executor, whose cost dominates every real latency figure.

## Scope

- **Bindings:** JSON-RPC over HTTP, HTTP+JSON (REST), gRPC, WebSocket.
- **Stores:** in-memory, SQLite, PostgreSQL.
- **Topologies:** one process; two replicas sharing PostgreSQL.
- **Out of scope:** the SLIMRPC binding (never published), TLS termination
  cost, and any executor that calls a model — a model's latency is the
  model's, and is recorded separately in the real-model run.

## Load profiles

| Id | Profile | Shape |
|---|---|---|
| L1 | Steady blocking | 8 concurrent clients, each sending blocking `SendMessage` to an executor that writes one artifact and completes; runs for the stated duration. |
| L2 | Streaming | 32 concurrent `SendStreamingMessage` streams, each receiving 20 events, re-opened as they end. |
| L3 | Churn | Clients connect, send one message, and drop the connection — half before the response arrives. |
| L4 | Overload | 4× the configured concurrency limit (`max_concurrent_streams`, rate limits, `max_connections`). |
| L5 | Continuations | Sequential multi-turn conversations: each turn answers `input-required` the moment it arrives (N21's shape). |

## Service levels

A requirement is met only when it holds on every binding it names. "Loopback"
means client and server in one host over 127.0.0.1.

| Id | Requirement | Threshold |
|---|---|---|
| R1 | No unexpected errors under L1, L2, L5 | 0 errors other than the refusals the profile provokes |
| R2 | Latency does not degrade with time under L1 | last-quarter p99 ≤ 1.5 × first-quarter p99 |
| R3 | Latency of a trivial blocking send, loopback, L1 | p99 < 25 ms on the reference host |
| R4 | Overload is refused, not queued into timeouts (L4) | every refusal is the binding's overload signal (HTTP 503 / gRPC `UNAVAILABLE` / JSON-RPC error), none is a client timeout |
| R5 | A continuation sent the moment `input-required` arrives is admitted (L5) | 0 refusals |
| R6 | Graceful shutdown with streams open | every open stream receives a terminal event or a clean close within `completion_grace` + 1 s |
| R7 | A peer that disconnects mid-stream releases server resources | queue, token and connection gone within the idle bound |

## Resource ceilings

| Id | Resource | Ceiling |
|---|---|---|
| C1 | Resident memory under L1 | ≤ 1 KiB growth per completed request (the soak test's existing bound) |
| C2 | File descriptors after L1–L3 end | back to the pre-load count ± 8 within 10 s |
| C3 | Tokio tasks after L1–L3 end | back to the pre-load count within 10 s |
| C4 | Tasks left non-terminal after L1 | 0 |
| C5 | Event queues and cancellation tokens after L3 | 0 left for tasks whose clients are gone, after the idle bound |

## Failure modes that must be handled

| Id | Fault | Required behaviour |
|---|---|---|
| F1 | Push target slow or refusing | the send path is not delayed; delivery is retried within its budget and then dropped with a log line |
| F2 | PostgreSQL restarted under load | calls fail with a store error while it is down and succeed again without restarting the server |
| F3 | Replica killed mid-stream | a client resuming with `Last-Event-ID` on the other replica receives every event after that id, once |
| F4 | Client dropped mid-stream (every binding) | as R7 |
| F5 | Malformed, oversized or slow input (slowloris, partial frames, oversized headers, compression bombs) | refused within the binding's bound; no unbounded buffering |

## What would move this bar

The thresholds are judgements, and these are the judgements. R3's 25 ms is
about 150 times the 169.6 µs the benchmark page
(`book/src/reference/benchmarks.md`) records for one JSON-RPC send on the
benchmark runner — a different host, sequential rather than 8-way, and a
figure this bar did not re-measure — so that it fails only on a real
regression, not on a busy host. C1's 1 KiB is the soak test's existing bound, kept rather
than tightened. R2's 1.5× is tighter than the soak test's 3.0× because the
soak test must also pass on shared CI runners, and this bar is measured on an
idle host. A reviewer who thinks any of them wrong should say so before the
numbers are read, not after.

## Measurements

None yet at the time of writing. Each measurement added here states the
commit, the host (CPU model and count, memory), the build profile, the exact
command, and the spread across runs.
