// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What [`Profile::MultiTenant`](crate::profile::Profile::MultiTenant) asks of a
//! handler that serves several tenants (adversarial review, 2026-11-12,
//! "multi-tenant production policy must be explicit").

use std::sync::Arc;

use super::{CallerIsolation, RequestHandlerBuilder, TenantIsolation};
use crate::error::{ServerError, ServerResult};
use crate::tenant_resolver::TenantResolver;

fn refuse(why: &str) -> ServerResult<()> {
    Err(ServerError::Config(format!(
        "the multi-tenant profile {why}"
    )))
}

impl RequestHandlerBuilder {
    /// Sets the tenant resolver and **asserts that it is authorized**: it names
    /// a tenant only from something the server has verified, or a gateway sets
    /// and strips from client traffic, never from what the client chose.
    /// [`Profile::MultiTenant`](crate::profile::Profile::MultiTenant) refuses a
    /// resolver given through [`with_tenant_resolver`](Self::with_tenant_resolver)
    /// alone, since a tenant the caller can choose isolates nothing (see the
    /// [`tenant_resolver`](crate::tenant_resolver) module's security section).
    ///
    /// The assertion is yours: nothing here can check that a gateway strips a
    /// header or that a JWT's signature was verified before the claim was read.
    /// It is a signed statement in the configuration, and the profile refuses a
    /// configuration that does not make it.
    #[must_use]
    pub fn with_authorized_tenant_resolver(mut self, resolver: impl TenantResolver) -> Self {
        self.tenant_resolver = Some(Arc::new(resolver));
        self.tenant_resolver_authorized = true;
        self
    }

    /// What the profile asks, checked before `build()` consumes the stores.
    pub(super) fn check_multi_tenant(&self) -> ServerResult<()> {
        if !self.interceptors.has_authenticator() {
            return refuse(
                "needs an authenticating interceptor: otherwise no request names a caller, \
                 and no tenant can be authorized for one",
            );
        }
        if self.tenant_resolver.is_none() || !self.tenant_resolver_authorized {
            return refuse(
                "needs a tenant resolver the deployment asserts is authorized \
                 (with_authorized_tenant_resolver): a tenant the client chose isolates nothing",
            );
        }
        if self.caller_isolation == Some(CallerIsolation::Shared) && self.task_store.is_none() {
            return refuse(
                "was told to share tasks across callers within a tenant, which needs a \
                 tenant-aware task store you supply: the default keeps one set of records \
                 for every tenant",
            );
        }
        // What `build()` will use: a store given, or the default, which
        // partitions by tenant because callers are isolated (sharing without a
        // store of its own was refused above).
        let task_partitions = self
            .task_store
            .as_ref()
            .is_none_or(|s| s.isolates_tenants());
        // An unset push-config store takes the default, which partitions when the
        // task store does, so only one that was given can fail on its own.
        let push_partitions = self
            .push_config_store
            .as_ref()
            .is_none_or(|s| s.isolates_tenants());
        if self.tenant_isolation == TenantIsolation::SharedAccepted
            || !(task_partitions && push_partitions)
        {
            return refuse(
                "needs a task store and a push-config store that partition by tenant (the \
                 TenantAware* stores) and does not accept stores that share records across \
                 tenants (accept_unisolated_tenants)",
            );
        }
        let Some(config) = &self.tenant_config else {
            return refuse(
                "needs finite per-tenant budgets (with_tenant_config): a tenant with no \
                 concurrency limit can take every executor slot from the others",
            );
        };
        let unbounded = std::iter::once(("the default", &config.default)).chain(
            config
                .overrides
                .iter()
                .map(|(tenant, limits)| (tenant.as_str(), limits)),
        );
        for (whose, limits) in unbounded {
            if limits.max_concurrent_tasks.is_none() {
                return Err(ServerError::Config(format!(
                    "the multi-tenant profile needs a concurrency limit for every tenant \
                     (TenantLimits::with_max_concurrent_tasks), and {whose} has none"
                )));
            }
        }
        Ok(())
    }
}
