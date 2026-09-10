# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Pairs each timeout with the bound that encloses it, and fails when the inner
one is larger — or the same one is applied twice — with no recorded reason.

This package is the body of `scripts/check_timeout_nesting.py`; that file is
the entry point and puts `scripts/lib` on `sys.path`. The split exists for
`scripts/check_file_lengths.sh`, which counts every tracked `.py` file against
a 500-line ratchet, helpers included.

What problem this solves
------------------------
Addendum 8 of docs/v0.9.0-post-release-review.md names a class with eight
shipped instances: **a documented, configurable bound that a wider bound
silently preempts.** Two shapes account for all of them:

* *The budget applied twice.* `shutdown_with_timeout(t)` drained to
  `now + t` and then handed the cleanup hook a fresh `t`; measured 2x. The
  discovery fetch bounded headers at 30 s and then the body at 30 s again;
  measured 55 s. The client's JSON-RPC transport had fixed exactly this and
  written down why (`transport/jsonrpc.rs`, "single deadline across header
  fetch AND body read"), and the fix recurred twice more after being written
  down.
* *The inner bound the outer never lets run.* `HttpPushSender` promises three
  attempts at 30 s with backoff — 98 s with the DNS bound — and the handler
  wraps every `send` in `push_delivery_timeout`, 5 s. Measured: one attempt,
  cut at 5.001 s. `max_attempts` and `backoff` were configuration that could
  not take effect.

A fix is not a lesson until something checks for the next instance. This is
that check: B14 in the review's backlog.

How the pieces fit
------------------
Over every non-test `.rs` file under `crates/*/src` and `bindings/*/src`
(`sites.sources`), with comments, strings and `#[cfg(test)]` items blanked
(`scrub`), the check finds every bound (`sites`), resolves each bound
expression as far as the text allows (`resolve`), and then runs four
pairings: nested, deadline and twice (`pairs`) and the hand-modelled push
schedule (`push`). `report` applies the two justification mechanisms — a
`// timeout-nesting: <reason>` comment and
`scripts/timeout_nesting_allowlist.txt` — and prints the gate's verdict or
the `--explain` listing. `entities` holds the data model these share and
`model` composes them into one `Model`.

What it cannot see, stated so that a clean run is read for what it is:

* **Bounds across function calls.** The push pairing is hand-modelled
  precisely because the enclosing `timeout` and the enclosed retry loop are
  two functions apart. Any other such pair is invisible until someone adds
  it here.
* **A knob renamed at a boundary, or with several defaults.** The default is
  found by field name; a field that two structs both declare with different
  defaults is reported as ambiguous and not compared.
* **Arithmetic.** `idle.saturating_sub(idle_for)` is a bound this script
  lists as unresolved and never compares.
* **Sleeps.** `tokio::time::sleep` is listed under `--explain` as a timer,
  not paired: keep-alives and backoffs are not budgets.

Exit codes: 0 clean, 1 at least one unjustified pair (or a stale allowlist
line), 2 the tree could not be read.
"""

from __future__ import annotations

import pathlib

# scripts/lib/timeout_nesting/__init__.py -> the checkout root.
REPO = pathlib.Path(__file__).resolve().parents[3]
ALLOWLIST = REPO / "scripts" / "timeout_nesting_allowlist.txt"

SENDER_RS = "crates/a2a-protocol-server/src/push/sender.rs"
LIMITS_RS = "crates/a2a-protocol-server/src/handler/limits.rs"
