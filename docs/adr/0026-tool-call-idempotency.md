<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0026: Tool calls and delegations carry idempotency keys; a repeated call runs once

**Date:** 2026-10-22
**Status:** Accepted
**Author:** Tom F.

---

## Context

A tool with a side effect (restart a service, send a message, charge a
card) does its work each time it is called. Two things call it twice for
what was meant once:

- **The model repeats itself.** In the tool-calling study's finished cells
  (`scripts/tool_calling_study/`, read 2026-10-22), a run repeated a call
  with the same tool and the same arguments in 0 to 16 of a cell's 60 runs:
  16 for Qwen3.5-0.8B `q4_0` with thinking on through this SDK, 10 for the
  same model asked directly. The study's tools are lookups, so nothing
  happened twice; a tool with an effect would have done its work twice in
  each of those runs.
- **The transport retries.** A tool that calls a backend over a network has
  the ambiguous failure `book/src/client/idempotency.md` describes for
  sends: the request may have landed. Backends that deduplicate take a key
  (an `Idempotency-Key` header, a unique request id); the tool had nothing
  stable to send.

The maintainer's 2026-10-21 decision asks for idempotency keys for tool
side effects by default. The tool loop (ADR 0022) is unreleased, so its
`Tool` trait can change without a deprecation.

## Decision

- **Every call carries a key.** `Tool::call` takes a `ToolInvocation`, its
  arguments and `idempotency_key`: HMAC-SHA256 under the loop's secret of
  the run's scope, the tool's name and the arguments in canonical form
  (object fields sorted, no whitespace), each field prefixed with its
  length as 8 bytes big-endian, rendered as 64 lowercase hex digits
  (`tool_loop/idempotency.rs`).
  - **The scope** is the task id and the message id under
    `ToolLoopExecutor`, so another message to the same task is new work with
    new keys; `ToolLoop::run` and `run_within` draw a random scope per run;
    `run_scoped` takes one from the caller.
  - **The secret** is 32 bytes drawn per loop by default (two version-4
    UUIDs, 244 random bits from the OS generator).
    `ToolLoop::with_idempotency_secret` sets one, which replicas share so
    the same call in the same scope has the same key on each.
  - **Keyed, not a plain hash,** because the book's rule for send keys
    holds here too: a key derived from what a caller can see (a task id, a
    tool's arguments) can be computed by that caller.
- **A repeated call runs once.** Within one run, a call whose key matches a
  call that succeeded is not run again: the model is sent `not run again:
  an identical call already ran in this task and returned: <result>`, and
  no tool span is opened, since nothing ran. A call that failed
  (`ToolError::Failed`) is run again, with the same key, so a backend that
  did the work before the failure can recognise the retry.
- **Side-effecting unless declared otherwise.** `Tool::read_only` defaults
  to `false`. A read-only tool runs every time, so a model can poll it for a
  change. Declaring a tool read-only is the opt-out, and the safe mistake is
  the default: a lookup left undeclared answers a repeated question from
  the first answer; an effect wrongly declared read-only would act twice.

- **Delegation keys its sends.** A delegated child is a side effect too.
  `Delegation::start` gives the child's message a fresh key (two version-4
  UUIDs, 64 hex digits) when it has none and `A2aClient::honours_idempotency`
  says the agent's card advertised the extension; a caller's key is kept,
  and `Delegation::idempotency_key` returns it. Nothing is added otherwise:
  this SDK's server refuses a keyed send its store cannot honour, so a key
  sent on a guess could break delegation to it. Random, not derived, as the
  book requires of send keys, which are the handle to a task.

Alternatives not taken:

- *A key and nothing else.* It leaves a repeated call to every backend to
  deduplicate, and most tools wrap something that does not.
- *A durable store of results, shared across replicas.* It would answer a
  call made again after a crash. The handler does not run the same message
  of a task twice (a resent message with a send key is answered from the
  task it created, and a task this executor ran is terminal), so there is no
  second run inside this SDK for it to serve. A deployment that runs work
  again itself shares the secret, and the backend sees the same key.
- *An ordinal per call (`call-3`) as the key.* A model that calls tools in
  another order on a second attempt would give a different operation the
  key of the first, and a backend that answers a reused key with its stored
  result would return the wrong one.

## Consequences

- A tool implementation changes from `call(arguments)` to
  `call(invocation)`, reading `invocation.arguments`; one that has no side
  effects adds `fn read_only(&self) -> bool { true }`.
- `tool-loop` now depends on `ring` for the HMAC. `ring` is already in the
  graph with the default `tls-rustls`.
- Arguments that differ only in how a number is spelled (`1` and `1.0`) are
  different calls, and both run.
- Not covered:
  - a call repeated in a later run (another message, a run outside the
    executor) is not answered from the earlier one; that is the backend's
    key to honour;
  - a message resent without a send key is a new task, with new keys, and
    its tools run again. The send key (`book/src/client/idempotency.md`)
    is what makes the resend the same work.
- A delegation's key helps a client with a retry policy (it retries the
  keyed send) and a parent holding the handle. A parent that resends after
  `start` fails sets its own key first; `from_card` sets no retry policy.
- Tests that fail without it (`tool_loop/run_tests/idempotency.rs`,
  `tool_loop/idempotency.rs`), each checked against a hand mutant on
  2026-10-22: no replay fails 2, a key ignoring the scope 3, the executor
  drawing a fresh scope 1, `Delegation` adding no key 1
  (`a2a-protocol-client/tests/delegation_idempotency_tests.rs`, where the
  same message resent with its key reached the same child and the worker
  ran once). The known-answer key was computed with Python's
  `hmac`, not this code. Removing the field sort fails no test in the
  server's own build, where `serde_json`'s map is sorted already; under
  `--features serde_json/preserve_order`, which `genai` 0.7 turns on in any
  build that includes it, it fails 2.
