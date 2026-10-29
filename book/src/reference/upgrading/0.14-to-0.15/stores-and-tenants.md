<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Stores and tenants

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). Each section names what breaks and what to change; the [upgrade guide](../../upgrading.md) has the policy and how to check your own code.

## `save_appending_history` takes a `HistoryLimit`

A `TaskStore` that overrides `save_appending_history` takes
`limit: HistoryLimit` where it took `max_history: usize`, and trims with
`limit.trim(&mut history)`, which bounds bytes as well as messages (release
audit F2). A caller that passed a count passes
`HistoryLimit::new(count, usize::MAX)` for the old behaviour; the handler
passes `TASK_HISTORY_LIMIT`, 1,024 messages and 16 MiB.

## `TenantLimits::max_stored_tasks` is gone

The field, `TenantLimits::with_max_stored_tasks` and
`TenantLimitsBuilder::max_stored_tasks` are removed. Nothing ever read them
(see [0.9 → 0.10](../0.9-to-0.10.md#tenantlimitsmax_stored_tasks-is-deprecated)); a build that
used them stops compiling where it used to warn. Give the tenant its own
`TaskStoreConfig` with `TenantAwareInMemoryTaskStore::with_tenant_override`
instead.

## A tenant needs stores that isolate tenants

A request that resolves to a non-empty tenant, named by the client or derived
by a `TenantResolver`, is now refused with `UnsupportedOperation` (`-32004` on
JSON-RPC, `400` on HTTP+JSON) unless both the task store and the push-config
store answer `true` from the new `isolates_tenants()`. The default stores
ignore the tenant, so such a request used to be served from records every
tenant shared: on 0.14.1 tenant B could get, list, subscribe to and cancel
tenant A's task. The six `TenantAware*` stores answer `true`, the default
stores `false`, and the built-in wrappers what the store they wrap answers.
Requests without a tenant are unaffected. Configure a `TenantAware*` task
store; an unset push-config store then defaults to
`TenantAwareInMemoryPushConfigStore`. Where tenants only key per-tenant limits
over records you mean every tenant to share, say so with
`RequestHandlerBuilder::accept_unisolated_tenants()`. A custom `TaskStore` or
`PushConfigStore` that partitions by tenant must override `isolates_tenants()`
to return `true`, and a wrapper store must forward it.

```text
// 0.14: a request naming tenant "acme" was served from the shared records
RequestHandlerBuilder::new(agent).with_agent_card(card).build()?;
```

```rust
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_server::TenantAwareInMemoryTaskStore;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#     .with_description("An example agent")
#     .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
// Each tenant gets records of its own.
let isolated = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card.clone())
    .with_task_store(TenantAwareInMemoryTaskStore::new())
    .build()?;

// Or: tenants key limits over records every tenant shares.
let shared = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card)
    .accept_unisolated_tenants()
    .build()?;
# drop((isolated, shared));
# Ok(())
# }
```
