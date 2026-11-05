<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0048: The tool loop gets a shared inference limit and a Retry-After-aware retry inside the budget (amends ADR 0022)

**Date:** 2026-11-04
**Status:** Accepted (0.16.0; audit H17). Built behind the `tool-loop`
feature: `InferenceLimit` and `LimitedModel`, `ModelError::RateLimited`,
`ModelRetry` and `ToolLoop::with_model_retry`.
**Author:** Tom F.

---

## Context

ADR 0022 said a `ToolModel` "does not run tools, retry or loop", and the loop
never retried: any `ModelError` ended the run. Since `84e0dbe` a 429, 502, 503
or 504 ends the task `transient`, so the A2A caller may retry, but it retries
the whole task and spends again every token the run had already used.

Nothing bounded how many turns are in flight at the provider. Every running
task's loop sends its turn at once, so a replica with 200 running tool-loop
tasks sends 200 completions to a model server with, say, two slots
(`llama-server -np 2`), which answers 429 or queues them past the request
timeout. The measured live run had the model as the bottleneck
(`docs/swarm-orchestration.md`, G4).

`ModelError::Status { status, body }` carries no `Retry-After`. The enum is
`#[non_exhaustive]`; the variant is not, so a field cannot be added to it.

Options (DECISIONS.md, H17): **A** setters on `OpenAiChatModel` (one
provider only); **B** a `ToolModel` decorator with a shared semaphore, and a
retry that honours `Retry-After` from a new `ModelError::RateLimited` variant
and stops when the budget's remaining time is shorter than the wait; **C**
leave it to a gateway.

## Decision

B, split where each half has what it needs.

1. **`InferenceLimit`**, a cloneable handle over one semaphore, and
   **`LimitedModel<M>`**, a `ToolModel` that takes a permit for each turn and
   holds it for that turn only. Share one `InferenceLimit` between every loop
   that talks to the same provider. It wraps any `ToolModel`, not only the
   bundled one. A turn waiting for a permit is abandoned when the run is
   cancelled (the loop already races the turn against its token) and is
   bounded by the executor's timeout, which ADR 0023 caps at the budget.
2. **`ModelError::RateLimited { retry_after, body }`**, produced by
   `OpenAiChatModel` for HTTP 429, with the provider's `Retry-After` read as
   delta-seconds or an HTTP date. A 429 is no longer `Status { status: 429 }`
   (a 0.16.0 break, listed below). It keeps the `error.type` `"429"` and the
   caller's message, and stays `transient`.
3. **`ModelRetry`**, set with `ToolLoop::with_model_retry`, off by default:
   on `RateLimited`, the loop waits and sends the same turn again, at most
   `max_retries` times (default 3). The wait is the provider's `Retry-After`
   lengthened by up to a quarter at random (the client's rule since
   `a8bf45e`, so loops refused together do not return together), or, with
   none, `base_delay` (500 ms) doubled per retry up to `max_delay` (30 s),
   then between half and all of that at random (the client's backoff jitter). A `Retry-After` longer than `max_delay` is not waited
   for: the run fails with the 429 as before. Nor is a wait longer than the
   budget's remaining time (ADR 0023), so the retry never spends time the
   caller did not give. The wait is abandoned when the run is cancelled.
   Each retry is logged with the delay. The time waited is charged to the
   deadline because it passes; nothing else is charged, since a refused turn
   reports no tokens.
4. The permit is not held while the loop waits to retry: a 429 frees the slot
   for another task's turn.

Retry is opt-in because it changes how long a task takes to fail, which an
operator may have tuned around; the book recommends it with a shared limit.

## Consequences

* Breaking (0.16.0 window): integrators matching
  `ModelError::Status { status: 429, .. }` from `OpenAiChatModel` match
  `ModelError::RateLimited { .. }` instead. `LoopError::is_transient`,
  `error_type` and the caller's message are unchanged for a 429.
* Other statuses (502, 503, 504) are not retried in the loop; they stay
  `transient` for the caller. A 503 from `llama-server` while it loads a model
  lasts minutes, longer than a turn's retry should wait.
* ADR 0022's "it never retries" now reads "it never retries unless
  `with_model_retry` is set, and then only a rate-limited turn".

## Not built

* A limit shared across processes (a gateway's job; option C remains right
  for a fleet).
* Retrying a turn that failed mid-stream after text was already handed to the
  sink: only a refusal before any answer (`RateLimited`) is retried.
