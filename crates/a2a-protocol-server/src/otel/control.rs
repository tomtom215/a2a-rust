// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The control plane's instruments, and the opt-in tenant and agent
//! dimensions (audit H11; ADR 0038 §4).
//!
//! Every label value here is bounded: a constant from
//! [`crate::metrics::control`], a tenant from the operator's allow-list or
//! `_OTHER`, an agent name the operator configured. No task, context,
//! root, job or message id is ever a label (ADR 0034 §4): those belong on
//! spans, where a per-job view is a trace query, not a series.

use std::sync::Arc;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Meter};

/// What a tenant outside the allow-list is recorded as.
pub const OTHER_TENANT: &str = "_OTHER";

/// The control plane's counters.
pub(super) struct Control {
    pub(super) refusals: Counter<u64>,
    pub(super) lease_events: Counter<u64>,
    pub(super) halts: Counter<u64>,
    pub(super) halt_stopped: Counter<u64>,
    pub(super) claim_events: Counter<u64>,
    pub(super) subscribe_wakes: Counter<u64>,
}

impl Control {
    pub(super) fn build(meter: &Meter) -> Self {
        Self {
            refusals: meter
                .u64_counter("a2a.server.refusals")
                .with_description("Requests refused before any work, by cause")
                .with_unit("{request}")
                .build(),
            lease_events: meter
                .u64_counter("a2a.server.lease.events")
                .with_description("Task lease events, by event")
                .with_unit("{event}")
                .build(),
            halts: meter
                .u64_counter("a2a.server.halts")
                .with_description("Halts and resumes applied, by scope and action")
                .with_unit("{action}")
                .build(),
            halt_stopped: meter
                .u64_counter("a2a.server.halt.stopped_tasks")
                .with_description("Running tasks a halt stopped, by scope")
                .with_unit("{task}")
                .build(),
            claim_events: meter
                .u64_counter("a2a.server.claim.events")
                .with_description("Turn-claim events, by event")
                .with_unit("{event}")
                .build(),
            subscribe_wakes: meter
                .u64_counter("a2a.server.subscribe.wakes")
                .with_description(
                    "Times a subscribe stream stopped waiting on a task's event log, by cause",
                )
                .with_unit("{wake}")
                .build(),
        }
    }
}

/// Which optional dimensions the metrics carry. Both off by default, so the
/// series an existing dashboard reads do not change.
#[derive(Clone, Debug, Default)]
pub(super) struct Dimensions {
    /// The tenants recorded by name; any other non-empty tenant is
    /// [`OTHER_TENANT`]. `None`: no `a2a.tenant` label at all.
    pub(super) tenants: Option<Arc<[String]>>,
    /// The `gen_ai.agent.name` every metric carries, when set: the agent
    /// this handler serves, one value per `OtelMetrics`.
    pub(super) agent: Option<Arc<str>>,
}

impl Dimensions {
    /// The `a2a.tenant` value for `tenant`, when the dimension is on and
    /// there is a tenant.
    pub(super) fn tenant(&self, tenant: Option<&str>) -> Option<KeyValue> {
        let allowed = self.tenants.as_ref()?;
        let tenant = tenant.filter(|t| !t.is_empty())?;
        let value = if allowed.iter().any(|a| a == tenant) {
            tenant.to_owned()
        } else {
            OTHER_TENANT.to_owned()
        };
        Some(KeyValue::new("a2a.tenant", value))
    }

    /// Adds the dimensions that are on to `attributes`.
    pub(super) fn extend(&self, attributes: &mut Vec<KeyValue>, tenant: Option<&str>) {
        attributes.extend(self.tenant(tenant));
        if let Some(agent) = &self.agent {
            attributes.push(KeyValue::new("gen_ai.agent.name", agent.to_string()));
        }
    }
}

impl Control {
    pub(super) fn refusal(&self, dims: &Dimensions, cause: &str) {
        let mut attributes = vec![KeyValue::new("cause", cause.to_owned())];
        let tenant = crate::store::tenant::TenantContext::current();
        dims.extend(&mut attributes, Some(&tenant));
        self.refusals.add(1, &attributes);
    }

    pub(super) fn lease(&self, dims: &Dimensions, event: &str) {
        let mut attributes = vec![KeyValue::new("event", event.to_owned())];
        dims.extend(&mut attributes, None);
        self.lease_events.add(1, &attributes);
    }

    pub(super) fn halt(&self, dims: &Dimensions, scope: &str, action: &str, stopped: usize) {
        let mut attributes = vec![
            KeyValue::new("scope", scope.to_owned()),
            KeyValue::new("action", action.to_owned()),
        ];
        dims.extend(&mut attributes, None);
        self.halts.add(1, &attributes);
        if stopped > 0 {
            attributes.retain(|kv| kv.key.as_str() != "action");
            self.halt_stopped
                .add(u64::try_from(stopped).unwrap_or(u64::MAX), &attributes);
        }
    }

    pub(super) fn claim(&self, dims: &Dimensions, event: &str) {
        let mut attributes = vec![KeyValue::new("event", event.to_owned())];
        dims.extend(&mut attributes, None);
        self.claim_events.add(1, &attributes);
    }

    pub(super) fn subscribe_wake(&self, dims: &Dimensions, cause: &str) {
        let mut attributes = vec![KeyValue::new("cause", cause.to_owned())];
        dims.extend(&mut attributes, None);
        self.subscribe_wakes.add(1, &attributes);
    }
}

impl super::OtelMetrics {
    /// Labels `rpc.server.call.duration`, `a2a.server.executor.duration` and
    /// `a2a.server.refusals` with `a2a.tenant`: the tenant's name when it is
    /// one of `allowed`, `_OTHER` for any other tenant, and
    /// no label for a request with none. Off by default; the series grow by
    /// at most `allowed.len() + 1` per existing series, which is the bound an
    /// operator chooses here (audit H11).
    #[must_use]
    pub fn with_tenant_dimension<S: Into<String>>(
        mut self,
        allowed: impl IntoIterator<Item = S>,
    ) -> Self {
        self.dims.tenants = Some(allowed.into_iter().map(Into::into).collect());
        self
    }

    /// Labels every `a2a.server.*` instrument this records, and
    /// `rpc.server.call.duration`, with `gen_ai.agent.name` = `name`: for a
    /// process serving several agents, one `OtelMetrics` per handler. Off by
    /// default.
    #[must_use]
    pub fn with_agent_name(mut self, name: impl Into<String>) -> Self {
        self.dims.agent = Some(Arc::from(name.into()));
        self
    }
}
