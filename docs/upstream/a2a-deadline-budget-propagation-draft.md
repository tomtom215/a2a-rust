<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Draft: propagating a deadline and a budget to delegated tasks

**Status: draft, not sent.** Intended for a discussion in `a2aproject/A2A`.
The maintainer decides whether and where to post it.

## Why

An agent that delegates part of its task to another agent has no way, in
A2A v1.0, to tell the other agent how long it may take or how much it may
spend. Two failures follow, and both cascade:

1. **A child outlives its parent's deadline.** A parent with ten minutes
   left starts a child that runs for an hour. The parent gives up, and the
   child goes on consuming resources for a result nobody will read.
2. **Fan-out overspends.** A parent allowed 100,000 tokens gives each of
   ten children 100,000, because nothing records what it has already given
   away.

OWASP's agentic list names this class (ASI08, Cascading Failures). RPC
systems settled the deadline half long ago: gRPC sends the remaining time
on every call (`grpc-timeout`), and every hop subtracts what it used.

a2a-rust ships this as a declared extension, in the style the spec already
allows (§3.3.4): values in `metadata`, the URI in `extensions`. It changes
nothing in the core protocol, and a peer that does not implement it is
served as before. ADR 0023 in a2a-rust records the design and its
alternatives.

## The extension: `https://a2a-rust.com/extensions/budget/v1`

**Grant.** On the message that starts or continues a task,
`metadata["a2a-rust.com/budget"]` holds

```json
{ "remainingMs": 30000, "meters": { "tokens": 50000 } }
```

and `extensions` names the URI. Either part may be absent, but not both.

- `remainingMs` is relative, as `grpc-timeout` is, so clock skew between
  agents does not move it. The receiver's deadline is its admission time
  plus `remainingMs`. The one-way transit time is the error; a sender keeps
  back a small reserve to cover it.
- A meter is a name (1 to 64 characters of `[a-z0-9._-]`, at most 16 per
  budget) and a whole number. Its unit is whatever the two agents agree the
  name means, for example `tokens` or `usd_micros`. The extension enforces
  arithmetic, not meaning, so A2A is not asked to standardise a cost model.

**Receipt.** The receiver:

- refuses a malformed budget, or one with no time left, as invalid params,
  before any work;
- bounds the turn by the smaller of its own timeout and the caller's
  deadline, and never extends the turn past its own timeout;
- ends a turn the caller's deadline stops as `TASK_STATE_FAILED`. In
  a2a-rust it also carries the failure class `budget-exhausted` from a
  companion extension, so the caller can tell "out of budget" from "broke".

**Status.** Every status update of the turn carries what is left and what
was spent, under the same key in `TaskStatusUpdateEvent.metadata`:

```json
{ "remainingMs": 12000, "meters": { "tokens": 41000 }, "spent": { "tokens": 9000 } }
```

A caller watching the stream sees how much is left before expensive work
starts, and a delegating parent reads `spent` from the terminal update.

**Delegating further.** A parent grants a child at most what it has left,
less a reserve, and *reserves* each meter at grant time. When the child
settles, the parent is charged the child's reported spend in place of the
reservation. A child that reports nothing is charged the whole
reservation, which is the conservative direction. A meter the parent has
but the request omits goes to the child as 0, so a child cannot spend
outside the parent's budget by omission.

## What we are asking

1. Is a deadline on `SendMessage`, relative and per hop, wanted in the
   core protocol, for example as a field on `SendMessageConfiguration`? The
   deadline half is mechanical and unit-free, and it is the half every
   implementation could adopt.
2. Should the meters stay an extension? We think so: units need agreement
   that the protocol cannot supply.
3. Is the status carrier right? We chose status-update metadata because
   status-update metadata is not merged into the stored task, and changing
   that would change every store. A `GetTask` poller therefore does not
   see the figure.

## Known weaknesses

- Accounting is only as honest as the child's report. A child that
  under-reports gets a larger refund.
- The receiver's admission time, not the time the bytes arrived, anchors
  the deadline. The child gets that difference on top of what was granted.
- A peer without the extension ignores the budget. Only a protocol-level
  deadline would close that.

## Evidence

a2a-rust 0.15.0 (unreleased) implements this and runs it two hops deep
against real servers (`crates/a2a-protocol-client/tests/delegation_budget_tests.rs`).
The grandchild's deadline falls inside the parent's, and the parent is
charged what the child reported spending. The wire shape is pinned by a
golden contract test (`crates/a2a-protocol-types/tests/golden/uri_contracts/budget.json`).
