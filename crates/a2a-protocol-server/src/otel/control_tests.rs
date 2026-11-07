// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The control plane's instruments and the opt-in dimensions (audit H11):
//! bounded label values, off by default, an allow-list with `_OTHER`.

use std::sync::Arc;
use std::time::Duration;

use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};

use super::OtelMetrics;
use crate::metrics::{Metrics, RpcCall, control};
use crate::store::tenant::TenantContext;

fn metrics() -> (OtelMetrics, Arc<ManualReader>, SdkMeterProvider) {
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(super::test_reader::SharedReader(Arc::clone(&reader)))
        .build();
    let metrics = OtelMetrics::from_meter(&provider.meter("a2a.server"));
    (metrics, reader, provider)
}

/// Each point of `name`, as (sorted `k=v` attributes, value).
fn points(reader: &Arc<ManualReader>, name: &str) -> Vec<(String, f64)> {
    let exported = super::test_reader::collect(reader);
    let mut out = Vec::new();
    for metric in exported
        .scope_metrics()
        .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
        .filter(|m| m.name() == name)
    {
        let attrs = |a: &mut dyn Iterator<Item = &opentelemetry::KeyValue>| {
            let mut kv: Vec<String> = a.map(|kv| format!("{}={}", kv.key, kv.value)).collect();
            kv.sort();
            kv.join(",")
        };
        match metric.data() {
            AggregatedMetrics::U64(MetricData::Sum(sum)) => {
                for p in sum.data_points() {
                    out.push((
                        attrs(&mut p.attributes()),
                        f64::from(u32::try_from(p.value()).unwrap()),
                    ));
                }
            }
            AggregatedMetrics::F64(MetricData::Histogram(h)) => {
                for p in h.data_points() {
                    out.push((
                        attrs(&mut p.attributes()),
                        f64::from(u32::try_from(p.count()).unwrap()),
                    ));
                }
            }
            other => panic!("{name}: {other:?}"),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn kv(attrs: &str, value: f64) -> (String, f64) {
    (attrs.to_owned(), value)
}

#[tokio::test]
async fn a_refusal_is_counted_by_cause_with_no_tenant_label_by_default() {
    let (m, reader, _p) = metrics();
    TenantContext::scope("acme", async {
        m.on_refusal(control::refusal::TENANT_CAP);
    })
    .await;
    m.on_refusal(control::refusal::STREAM_CAP);
    m.on_refusal(control::refusal::STREAM_CAP);
    assert_eq!(
        points(&reader, "a2a.server.refusals"),
        [kv("cause=stream_cap", 2.0), kv("cause=tenant_cap", 1.0)]
    );
}

/// An allowed tenant by name, any other as `_OTHER`, none without a tenant.
#[tokio::test]
async fn the_tenant_dimension_names_only_the_allowed_tenants() {
    let (m, reader, _p) = metrics();
    let m = m.with_tenant_dimension(["acme"]);
    for tenant in ["acme", "zeta", "omega"] {
        TenantContext::scope(tenant, async {
            m.on_refusal(control::refusal::TENANT_CAP);
        })
        .await;
    }
    m.on_refusal(control::refusal::TENANT_CAP);
    assert_eq!(
        points(&reader, "a2a.server.refusals"),
        [
            kv("a2a.tenant=_OTHER,cause=tenant_cap", 2.0),
            kv("a2a.tenant=acme,cause=tenant_cap", 1.0),
            kv("cause=tenant_cap", 1.0),
        ]
    );
}

#[test]
fn the_rpc_and_executor_metrics_take_the_dimensions() {
    let (m, reader, _p) = metrics();
    let m = m.with_tenant_dimension(["acme"]).with_agent_name("billing");
    for tenant in [Some("acme"), Some("zeta"), None] {
        m.on_rpc_call(&RpcCall {
            system: "jsonrpc",
            method: Some("m"),
            duration: Duration::from_millis(1),
            status_code: None,
            error_type: None,
            tenant,
        });
    }
    assert_eq!(
        points(&reader, "rpc.server.call.duration"),
        [
            kv(
                "a2a.tenant=_OTHER,gen_ai.agent.name=billing,rpc.method=m,rpc.system.name=jsonrpc",
                1.0
            ),
            kv(
                "a2a.tenant=acme,gen_ai.agent.name=billing,rpc.method=m,rpc.system.name=jsonrpc",
                1.0
            ),
            kv(
                "gen_ai.agent.name=billing,rpc.method=m,rpc.system.name=jsonrpc",
                1.0
            ),
        ]
    );
    m.on_executor_run("completed", Duration::from_millis(1));
    assert_eq!(
        points(&reader, "a2a.server.executor.duration"),
        [kv("gen_ai.agent.name=billing,outcome=completed", 1.0)]
    );
}

/// Without the dimensions, the call's tenant is not a label.
#[test]
fn the_rpc_metric_has_no_tenant_label_by_default() {
    let (m, reader, _p) = metrics();
    m.on_rpc_call(&RpcCall {
        system: "grpc",
        method: Some("m"),
        duration: Duration::from_millis(1),
        status_code: Some("OK"),
        error_type: None,
        tenant: Some("acme"),
    });
    assert_eq!(
        points(&reader, "rpc.server.call.duration"),
        [kv(
            "rpc.method=m,rpc.status_code=OK,rpc.system.name=grpc",
            1.0
        )]
    );
}

#[test]
fn lease_halt_and_claim_events_are_counted() {
    let (m, reader, _p) = metrics();
    m.on_lease(control::lease::LAPSED);
    m.on_halt(control::halt_scope::ALL, control::halt_action::HALT, 3);
    m.on_halt(control::halt_scope::ALL, control::halt_action::RESUME, 0);
    m.on_halt(control::halt_scope::TENANT, control::halt_action::HALT, 0);
    m.on_claim(control::claim::REAPED);
    assert_eq!(
        points(&reader, "a2a.server.lease.events"),
        [kv("event=lapsed", 1.0)]
    );
    assert_eq!(
        points(&reader, "a2a.server.halts"),
        [
            kv("action=halt,scope=all", 1.0),
            kv("action=halt,scope=tenant", 1.0),
            kv("action=resume,scope=all", 1.0),
        ]
    );
    assert_eq!(
        points(&reader, "a2a.server.halt.stopped_tasks"),
        [kv("scope=all", 3.0)]
    );
    assert_eq!(
        points(&reader, "a2a.server.claim.events"),
        [kv("event=reaped", 1.0)]
    );
}

/// A subscribe stream's wakes are counted by cause, and the tenant and agent
/// dimensions (off here) are not on it.
#[test]
fn subscribe_wakes_are_counted_by_cause() {
    let (m, reader, _p) = metrics();
    m.on_subscribe_wake(crate::metrics::subscribe_wake::NOTIFICATION);
    m.on_subscribe_wake(crate::metrics::subscribe_wake::NOTIFICATION);
    m.on_subscribe_wake(crate::metrics::subscribe_wake::RESYNC);
    m.on_subscribe_wake(crate::metrics::subscribe_wake::TIMEOUT);
    assert_eq!(
        points(&reader, "a2a.server.subscribe.wakes"),
        [
            kv("cause=notification", 2.0),
            kv("cause=resync", 1.0),
            kv("cause=timeout", 1.0),
        ]
    );
}

/// `Arc`, `Tee` and the global forwarder pass every new hook on: until 0.12
/// a missing forward in the `Arc` impl dropped hooks silently.
#[test]
fn the_wrappers_forward_the_control_hooks() {
    #[derive(Default)]
    struct Count(std::sync::atomic::AtomicUsize);
    impl Metrics for Count {
        fn on_refusal(&self, _: &str) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        fn on_lease(&self, _: &str) {
            self.0.fetch_add(10, std::sync::atomic::Ordering::SeqCst);
        }
        fn on_halt(&self, _: &str, _: &str, _: usize) {
            self.0.fetch_add(100, std::sync::atomic::Ordering::SeqCst);
        }
        fn on_claim(&self, _: &str) {
            self.0.fetch_add(1000, std::sync::atomic::Ordering::SeqCst);
        }
        fn on_subscribe_wake(&self, _: &str) {
            self.0.fetch_add(10000, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let fire = |m: &dyn Metrics| {
        m.on_refusal("x");
        m.on_lease("x");
        m.on_halt("x", "y", 0);
        m.on_claim("x");
        m.on_subscribe_wake("x");
    };
    let a = Arc::new(Count::default());
    fire(&Arc::clone(&a));
    assert_eq!(a.0.load(std::sync::atomic::Ordering::SeqCst), 11111);
    let (first, second) = (Arc::new(Count::default()), Arc::new(Count::default()));
    fire(&crate::metrics::Tee {
        first: Arc::clone(&first),
        second: Arc::clone(&second),
    });
    assert_eq!(first.0.load(std::sync::atomic::Ordering::SeqCst), 11111);
    assert_eq!(second.0.load(std::sync::atomic::Ordering::SeqCst), 11111);
}

/// The executor's run is labelled with the tenant it ran for.
#[tokio::test]
async fn the_executor_metric_takes_the_tenant_it_ran_for() {
    let (m, reader, _p) = metrics();
    let m = m.with_tenant_dimension(["acme"]);
    TenantContext::scope("acme", async {
        m.on_executor_run("completed", Duration::from_millis(1));
    })
    .await;
    assert_eq!(
        points(&reader, "a2a.server.executor.duration"),
        [kv("a2a.tenant=acme,outcome=completed", 1.0)]
    );
}
