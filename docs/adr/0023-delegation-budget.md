<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0023: A budget travels down the delegation chain

**Date:** 2026-10-21
**Status:** Accepted
**Author:** Tom F.

---

## Context

The control map's ASI08 row (OWASP Agentic Security, "Cascading Failures")
stays `partial` for G5 in `docs/swarm-orchestration.md`: nothing carries a
deadline or a spending limit from a parent to the agents it delegates to. A
parent with ten minutes left can start a child that runs for an hour, and a
parent allowed 100,000 tokens can give each of ten children 100,000. A child
also cannot see how much it has left, so it cannot plan to fit.

A2A v1.0.1 has no deadline or budget field. Any budget is therefore an
extension, in the style of the lease (ADR 0020), idempotency, failure-class
and approval extensions. The maintainer chose a deadline plus named meters
on 2026-10-21 (`docs/handoff.md`).

## Options

1. **Deadline only.** The swarm document's "deadline first". It is
   mechanical, but leaves the fan-out overspend open, and that overspend is
   the cascade the row is about.
2. **Deadline plus named meters, with the units left to the caller.** A meter
   is a name and a whole number, such as `tokens` or `usd_micros`. The SDK
   enforces arithmetic, not meaning. Agents that agree on a name agree on
   its unit, and the specification is not asked to.
3. **A standard cost model** (tokens, money, tool calls). This needs the
   unit agreement that A2A does not supply, and it would be wrong for agents
   that bill in other units.

## Decision

Option 2.

- **Wire.** A message carries
  `{"remainingMs": n, "meters": {"tokens": 50000}}` in `Message.metadata`
  under `a2a-rust.com/budget`, and declares
  `https://a2a-rust.com/extensions/budget/v1` in `Message.extensions`
  (`a2a_protocol_types::budget::Budget`). Both parts are optional, and a
  budget with neither is malformed.
  - The time is *relative*, as gRPC's `grpc-timeout` is, so that clock skew
    between agents does not move it. The one-way transit time is the error,
    and the sender's reserve covers it (below).
  - A meter name is 1 to 64 characters of `[a-z0-9._-]`, with at most 16
    meters. Values are `u64`.
- **Receipt.** A server always honours a budget, and its card declares the
  extension, not as required. There is no switch, because a caller's
  deadline that is silently ignored is the gap itself.
  - Before any work, the handler refuses a malformed budget as
    `InvalidParams`. It refuses a `remainingMs` of 0 as failure class
    `budget-exhausted`.
  - The deadline is the receipt time plus `remainingMs`. The turn's executor
    timeout becomes `min(handler or tenant timeout, time to the deadline)`.
  - A turn stopped by the inherited deadline ends `failed`, with failure
    class `budget-exhausted`. That separates it from a lease lapse
    (`canceled`) and from the server's own timeout.
- **Scope.** A budget bounds one turn, meaning the execution started by one
  `SendMessage`. A continuing message brings its own budget. Budgets are
  per process, need no store, and do not cross replicas: the turn runs on
  one replica.
- **The executor's view.** `RequestContext::budget()` returns the turn's
  `BudgetHandle`, or `None` when the message carried no budget.
  - `remaining_time()` and `remaining(meter)` are the "how much more can I
    spend" signal.
  - `charge(meter, amount)` records real spend. It saturates at zero and
    returns `Err(BudgetExhausted)` once the meter is overdrawn, with the
    overdraft recorded. Spend that already happened is never refused,
    because refusing it would lose the record.
  - A meter the budget does not name is unlimited, and charging it is a
    no-op. A budget limits only what it names.
  - Charges are lock-free (an atomic compare-and-swap per meter), so
    concurrent tool calls in one turn cannot overspend by a race.
- **Delegating.** `BudgetHandle::grant(request)` carves a child's budget out
  of the parent's.
  - The child's time is the parent's remaining time minus a reserve. The
    reserve defaults to 5% of the remaining time, at least 100 ms and at
    most 5 s, and covers transit and the parent's own wrap-up.
  - Each meter is the smaller of the amount requested and the amount left,
    and it is *reserved*, which means deducted from the parent at grant
    time. Ten children cannot each be given the whole.
  - A parent's meter that the request does not name goes to the child as
    0, so a child cannot spend outside the parent's budget by omission. A
    meter only the request names passes through, because the parent does
    not limit it.
  - `BudgetHandle` lives in `a2a_protocol_types::budget`, so the server's
    `RequestContext` and the client's `Delegation` share one account
    without the server depending on the client. Its atomics use a
    compare-exchange loop: `fetch_update` is deprecated on current
    toolchains, and its replacement is newer than the MSRV.
  - A grant when no time is left, or for an exhausted meter, is
    `Err(BudgetExhausted)`, and no child is started.
  - `Delegation::start_budgeted` attaches the grant. When the child settles,
    the parent is refunded the unspent part, granted minus spent. If the
    child reports no spend, nothing is refunded, which is the conservative
    direction.
- **Reporting spend.** On every status update, the server writes
  `{"remainingMs", "meters", "spent"}` under `a2a-rust.com/budget` in the
  task's `metadata`. A parent, a dashboard or the caller can therefore see
  the remaining budget while the task runs, and the delegation reads
  `spent` to refund.
- **Tool loop.** When the context has a budget, `ToolLoop` charges the
  `tokens` meter with each model reply's reported usage and stops with
  `budget-exhausted` when it is spent. It also stops at the deadline. A
  model that reports no usage charges nothing, and the docs say so.

## Consequences

- A child of a parent with a deadline cannot outlive that deadline by more
  than the transit time. A parent cannot grant more of a meter than it has
  left, across any number of children. These are the two cascades ASI08
  names that a lease does not cover.
- What stays open on ASI08:
  - A peer that does not implement the extension ignores the budget, as
    with the lease.
  - Units mean only what the agents agree on.
  - A child that under-reports its spend gets a larger refund. Accounting
    is only as honest as the child, so a parent that cannot trust its child
    should not rely on refunds.
- Upstream: deadline propagation is a candidate A2A proposal, drafted in
  `docs/upstream/` and not filed, as the maintainer's rule requires.
- Tests that must fail without the change:
  - the types round-trip and the validation bounds;
  - the deadline capping the executor timeout, and the failure class;
  - refusal before work;
  - lock-free charging under concurrent charges;
  - reservation across ten children;
  - the refund on settle;
  - the remaining figure on the task;
  - the tool loop stopping on tokens;
  - a two-hop end-to-end run in which the grandchild's deadline is inside
    the parent's.
