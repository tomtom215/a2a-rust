// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Multi-tenant isolation, checked by absence and by refusal.

use std::sync::Arc;

use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::store::TenantAwareInMemoryTaskStore;
use a2a_protocol_server::tenant_resolver::HeaderTenantResolver;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::SendMessageResponse;

use super::{bind, plain_card, serve, Check, HeaderInterceptor};
use crate::agents::LogSearchExecutor;
use crate::{send_params, user_message};

const LABEL: &str = "Tenant isolation (server-resolved, cross-tenant refused)";

/// The header the deployment trusts to name the tenant.
///
/// In production this is set by a gateway that has already authenticated the
/// caller; the point of resolving server-side is that the *client* does not get
/// to choose. [`HeaderTenantResolver`] reads `x-tenant-id` by default.
const TENANT_HEADER: &str = "x-tenant-id";

/// Two tenants must not see each other's tasks, and neither may name the other.
///
/// This is checked three ways, because the two obvious ways can both pass on a
/// broken server:
///
/// 1. **Each tenant sees its own task.** A store that returned nothing to
///    everybody would pass an isolation check that only looked for absence.
/// 2. **Neither tenant sees the other's task.** A store that ignored the tenant
///    entirely would pass a check that only looked for presence.
/// 3. **A caller authenticated as `acme` that *names* `globex` is refused.**
///    Before v0.6.0 a configured [`HeaderTenantResolver`] was built but never
///    consulted, so the client-supplied `params.tenant` alone selected the
///    partition: any caller could read another tenant's tasks by naming them.
///    Checks 1 and 2 both pass against that build. Only this one fails.
pub(super) async fn isolation() -> Check {
    let (listener, url) = bind().await;

    let handler = match RequestHandlerBuilder::new(LogSearchExecutor)
        .with_agent_card(plain_card(&url, "Tenant Agent"))
        .with_task_store(TenantAwareInMemoryTaskStore::new())
        // Without this the client's `params.tenant` is trusted verbatim, which
        // is correct only for a single-tenant deployment behind an
        // authenticating gateway.
        .with_tenant_resolver(HeaderTenantResolver::default())
        .build()
    {
        Ok(handler) => Arc::new(handler),
        Err(e) => return Check::fail(LABEL, format!("building the handler: {e}")),
    };
    serve(listener, handler);

    let tenants = ["acme", "globex"];
    let clients: Vec<A2aClient> = {
        let mut built = Vec::new();
        for tenant in tenants {
            match ClientBuilder::new(&url)
                .with_interceptor(HeaderInterceptor::new(TENANT_HEADER, tenant))
                .build()
            {
                Ok(client) => built.push(client),
                Err(e) => return Check::fail(LABEL, format!("building the {tenant} client: {e}")),
            }
        }
        built
    };

    // One task per tenant, created over the wire under that tenant's header.
    let mut owned = Vec::new();
    for (tenant, client) in tenants.iter().zip(&clients) {
        match client
            .send_message(send_params(user_message("payments-api")))
            .await
        {
            Ok(SendMessageResponse::Task(task)) => owned.push((*tenant, task.id.0.clone())),
            Ok(other) => {
                return Check::fail(LABEL, format!("{tenant}: expected a Task, got {other:?}"))
            }
            Err(e) => return Check::fail(LABEL, format!("{tenant}: send failed: {e}")),
        }
    }

    // (1) and (2): each tenant's list contains its own id and no other's.
    for ((tenant, own_id), client) in owned.iter().zip(&clients) {
        let listed = match client.list_tasks(ListTasksParams::default()).await {
            Ok(page) => page,
            Err(e) => return Check::fail(LABEL, format!("listing for {tenant}: {e}")),
        };
        let seen: Vec<&str> = listed.tasks.iter().map(|t| t.id.0.as_str()).collect();
        if !seen.contains(&own_id.as_str()) {
            return Check::fail(
                LABEL,
                format!(
                    "{tenant} cannot see its own task {own_id} — the partition is not readable"
                ),
            );
        }
        for (other, other_id) in owned.iter().filter(|(t, _)| t != tenant) {
            if seen.contains(&other_id.as_str()) {
                return Check::fail(
                    LABEL,
                    format!("{tenant} can see {other}'s task {other_id} — partitions leak"),
                );
            }
        }
    }

    // (3): authenticated as the first tenant, naming the second.
    let (victim, _) = &owned[1];
    let mut smuggled = send_params(user_message("payments-api"));
    smuggled.tenant = Some((*victim).to_owned());
    match clients[0].send_message(smuggled).await {
        Ok(_) => Check::fail(
            LABEL,
            format!(
                "a caller authenticated as {} wrote into {victim} by naming it in params.tenant \
                 — the resolver is not authoritative",
                owned[0].0
            ),
        ),
        Err(_) => Check::pass(
            LABEL,
            format!(
                "{} tenants isolated; a cross-tenant params.tenant was refused",
                owned.len()
            ),
        ),
    }
}
