<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0053: Five renames through deprecation, and a naming rule for new APIs

**Date:** 2026-11-05
**Status:** Accepted (the maintainer's instruction of 2026-11-04: every open
plan item goes into 0.16.0 with the best engineering option, open to a veto in
review). Built in 0.16.0.
**Author:** Tom F.

---

## Context

Audit G-15 ("naming inconsistencies"), re-read at the 0.16.0 integration head
(`558be5d3`). Every name the audit listed was still present:

| Where | Today | What is wrong with it |
|---|---|---|
| `RequestHandler` | `on_resubscribe` | The wire method is `SubscribeToTask` (spec §9.4), and the client's method is `subscribe_to_task`; "resubscribe" is a v0.3 name. |
| `RequestHandler` | `on_set_push_config` | The wire method is `CreateTaskPushNotificationConfig`; the client says `set_push_config`; the dispatchers' own code calls it "create". |
| `A2aClient` | `set_push_config` | As above, on the client. |
| `A2aRouter` | the type's name | It serves HTTP+JSON only (with `with_websocket`, also the WebSocket upgrade); "A2a" names no binding, while its siblings are `JsonRpcDispatcher` and `RestDispatcher`. |
| `RateLimitConfig` | `with_window_secs(u64)` | Every other duration in the crate's configuration is a `Duration`. |
| `TenantLimitsBuilder` | `max_concurrent_tasks`, `executor_timeout`, `event_queue_capacity`, `rate_limit_rps` | Bare setters on a builder, while `TenantLimits` itself has `with_*` for the same four. |

The audit also listed inconsistencies that are not on this list: `with_config`
meaning a setter on `Server` and a constructor elsewhere, the three auth
interceptor names, `CallInterceptor` vs `ServerInterceptor`, and the mixed
`with_`/`without_`/`require_`/`allow_`/bare vocabulary of
`RequestHandlerBuilder`.

`STABILITY.md` §3: a rename is a new name plus `#[deprecated]` on the old one,
removed no earlier than the next minor.

## Options

1. **A short list, through deprecation.** The six rows above get a new name in
   0.16.0; the old one stays, `#[deprecated(since = "0.16.0")]`, calling the
   new one, and is removed no earlier than 0.17.0.
2. **Normalise the whole vocabulary** (the rows above plus every builder verb
   and the interceptor names). Dozens of deprecations, every example and book
   page touched, for names that are unusual but not misleading.
3. **Change nothing; write the rule down** for APIs added from now on.

## Decision

Option 1 for the six rows, and option 3's rule for everything new. Option 2 is
rejected: a deprecation costs every adopter a migration, and the remaining
names do not send anyone to the wrong method.

### The new names

| Old (deprecated in 0.16.0, removed no earlier than 0.17.0) | New |
|---|---|
| `RequestHandler::on_resubscribe` | `RequestHandler::on_subscribe_to_task` |
| `RequestHandler::on_set_push_config` | `RequestHandler::on_create_push_config` |
| `A2aClient::set_push_config` | `A2aClient::create_push_config` |
| `RateLimitConfig::with_window_secs(u64)` | `RateLimitConfig::with_window(Duration)` |
| `TenantLimitsBuilder::max_concurrent_tasks` | `TenantLimitsBuilder::with_max_concurrent_tasks` |
| `TenantLimitsBuilder::executor_timeout` | `TenantLimitsBuilder::with_executor_timeout` |
| `TenantLimitsBuilder::event_queue_capacity` | `TenantLimitsBuilder::with_event_queue_capacity` |
| `TenantLimitsBuilder::rate_limit_rps` | `TenantLimitsBuilder::with_rate_limit_rps` |

`A2aRouter` is not deprecated: a type alias, **`RestRouter`**, is the name the
documentation uses. `RestRouter` rather than `HttpJsonRouter` because this
crate already names the HTTP+JSON binding "REST" everywhere a Rust identifier
is involved (`RestDispatcher`, `AgentInterface::rest`), and an alias that
matched the spec's spelling would be the one name in the crate that did not
match the rest. An alias costs nothing to keep, so `A2aRouter` stays
undeprecated: existing code keeps compiling without a warning, and the two
names are the same type.

`with_window` keeps the field it sets, `window_secs: u64` (a public field;
changing its type would be a break with no deprecation path). The window is
counted in whole seconds, so a `Duration` with a fractional second is rounded
**up**: a window of 1.5 s is 2 s, and no non-zero duration becomes the
refused zero window. `Duration::ZERO` stays zero, which
`RateLimitInterceptor::new` refuses, as it refuses `with_window_secs(0)`.

The SDK prelude re-exports types, not methods, so none of its names change.

### The rule for new APIs (written into `CONTRIBUTING.md`)

* A setter on a builder or a configuration type is `with_<field>`; the one that
  turns a default-on behaviour off is `without_<field>`.
* A `RequestHandler` entry point is `on_` plus the wire method's name in
  `snake_case` (`SendMessage` → `on_send_message`); a client method is the wire
  method's name in `snake_case`.
* A duration is a `std::time::Duration`, never an integer of seconds or
  milliseconds.
* A rename goes through deprecation (`STABILITY.md` §3): the new name, then
  `#[deprecated(since = "X.Y.0", note = "use `new`; removed no earlier than X.(Y+1).0")]`
  on the old one, which calls the new one, and the old one's own test under
  `#[allow(deprecated)]` (with the removal version in the comment) as its only
  caller in the workspace.

## Consequences

* Eight deprecated methods, each removed no earlier than 0.17.0. Each keeps a
  test that calls it and checks it still behaves as the new name does; nothing
  else in the workspace calls it.
* The book, the READMEs and the examples use the new names and `RestRouter`.
* Not built: the rest of the vocabulary (option 2), and a gate that every
  `#[deprecated]` names a removal version. The repository has no such gate
  (`grep -rn deprecated scripts/` finds only the inert-bounds gate), and the
  0.15.0 deprecations do not name one, so a gate written now would fail on
  items this ADR does not own; the maintainer can add it with them.
