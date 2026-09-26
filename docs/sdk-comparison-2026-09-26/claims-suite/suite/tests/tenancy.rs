// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claims: Multi-tenancy isolation, TenantResolver strategies (header, bearer,
//! path), PerTenantConfig limits. Ports 7600-7649.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::{
    BearerTokenTenantResolver, HeaderTenantResolver, PathSegmentTenantResolver, PerTenantConfig,
    TenantAwareInMemoryTaskStore, TenantLimits,
};
use claims_suite::common::*;
use serde_json::{json, Value};

fn port() -> u16 {
    port_in(7600, 50)
}

fn task_id_from(body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or_else(|_| panic!("not json: {body}"));
    v["result"]["task"]["id"]
        .as_str()
        .or_else(|| v["result"]["id"].as_str())
        .unwrap_or_else(|| panic!("no task id in {body}"))
        .to_owned()
}

async fn start(b: RequestHandlerBuilder) -> String {
    let p = port();
    let h = Arc::new(b.build().unwrap());
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    format!("http://127.0.0.1:{p}")
}

async fn cross_read(url: &str, hdr_a: (&str, &str), hdr_b: (&str, &str), label: &str) -> (bool, bool, bool) {
    let (s, _, b) = jsonrpc_raw(url, "SendMessage", send_params_json("hi"), &[hdr_a]).await;
    assert_eq!(s, 200, "{label} send: {b}");
    let tid = task_id_from(&b);
    let (_, _, a_get) = jsonrpc_raw(url, "GetTask", json!({"id": tid}), &[hdr_a]).await;
    let (_, _, b_get) = jsonrpc_raw(url, "GetTask", json!({"id": tid}), &[hdr_b]).await;
    let (_, _, b_list) = jsonrpc_raw(url, "ListTasks", json!({}), &[hdr_b]).await;
    let a_ok = a_get.contains("\"result\"");
    let b_can_read = b_get.contains("\"result\"");
    let b_lists = b_list.contains(&tid);
    println!("{label}: owner_get_ok={a_ok} other_tenant_get_ok={b_can_read} other_tenant_list_contains={b_lists}");
    println!("  other tenant GetTask body: {}", &b_get[..b_get.len().min(220)]);
    (a_ok, b_can_read, b_lists)
}

#[tokio::test(flavor = "multi_thread")]
async fn header_resolver_with_tenant_aware_store_isolates() {
    let (agent, _p) = CtlAgent::new();
    let url = start(
        RequestHandlerBuilder::new(agent)
            .with_tenant_resolver(HeaderTenantResolver::new("x-tenant-id"))
            .with_task_store(TenantAwareInMemoryTaskStore::new()),
    )
    .await;
    let (a, b, l) = cross_read(&url, ("x-tenant-id", "A"), ("x-tenant-id", "B"), "header+TenantAwareInMemory").await;
    assert!(a && !b && !l);

    // Client-supplied params.tenant that disagrees with resolver is refused.
    let (_, _, body) = jsonrpc_raw(&url, "ListTasks", json!({"tenant":"A"}), &[("x-tenant-id", "B")]).await;
    println!("B naming tenant A in params: {}", &body[..body.len().min(200)]);
    assert!(body.contains("\"error\""));
}

/// What a user gets if they set a resolver but keep the default store.
#[tokio::test(flavor = "multi_thread")]
async fn header_resolver_with_default_store() {
    let (agent, _p) = CtlAgent::new();
    let url = start(
        RequestHandlerBuilder::new(agent).with_tenant_resolver(HeaderTenantResolver::new("x-tenant-id")),
    )
    .await;
    let (a, b, l) = cross_read(&url, ("x-tenant-id", "A"), ("x-tenant-id", "B"), "header+DEFAULT InMemoryTaskStore").await;
    println!("RESULT default-store isolation: owner={a} cross_get={b} cross_list={l}");
}

#[tokio::test(flavor = "multi_thread")]
async fn bearer_resolver_isolates() {
    let (agent, _p) = CtlAgent::new();
    let url = start(
        RequestHandlerBuilder::new(agent)
            .with_tenant_resolver(BearerTokenTenantResolver::with_mapper(|tok| {
                tok.strip_prefix("tok-").map(str::to_owned)
            }))
            .with_task_store(TenantAwareInMemoryTaskStore::new()),
    )
    .await;
    let (a, b, l) = cross_read(&url, ("authorization", "Bearer tok-A"), ("authorization", "Bearer tok-B"), "bearer+TenantAwareInMemory").await;
    assert!(a && !b && !l);
}

/// PathSegmentTenantResolver with the REST dispatcher's documented tenant
/// routes (/tenants/{t}/... and /{t}/...).
#[tokio::test(flavor = "multi_thread")]
async fn path_resolver_over_rest() {
    let (agent, probe) = CtlAgent::new();
    let p = port();
    let h = Arc::new(
        RequestHandlerBuilder::new(agent)
            .with_tenant_resolver(PathSegmentTenantResolver::new(1)) // /tenants/{acme}/...
            .with_task_store(TenantAwareInMemoryTaskStore::new())
            .build()
            .unwrap(),
    );
    serve_with_addr(format!("127.0.0.1:{p}"), RestDispatcher::new(h)).await.unwrap();
    let c = reqwest::Client::new();
    let body = json!({"message":{"messageId": uid(), "role":"ROLE_USER","parts":[{"text":"hi"}]}});
    let r = c
        .post(format!("http://127.0.0.1:{p}/tenants/acme/message:send"))
        .header("content-type", "application/json")
        .header("A2A-Version", "1.0")
        .json(&body)
        .send()
        .await
        .unwrap();
    let s = r.status().as_u16();
    let t = r.text().await.unwrap();
    println!("PathSegmentTenantResolver(1) POST /tenants/acme/message:send -> {s} {}", &t[..t.len().min(300)]);
    println!("executor saw tenants: {:?}", probe.tenants.lock().unwrap());
    // Also: HeaderTenantResolver + same explicit path works?
    assert_eq!(s, 200, "path-resolved tenant request should succeed");
}

/// Same REST tenant path with no resolver at all (the params.tenant route).
#[tokio::test(flavor = "multi_thread")]
async fn rest_tenant_path_without_resolver() {
    let (agent, probe) = CtlAgent::new();
    let p = port();
    let h = Arc::new(
        RequestHandlerBuilder::new(agent)
            .with_task_store(TenantAwareInMemoryTaskStore::new())
            .build()
            .unwrap(),
    );
    serve_with_addr(format!("127.0.0.1:{p}"), RestDispatcher::new(h)).await.unwrap();
    let c = reqwest::Client::new();
    let body = json!({"message":{"messageId": uid(), "role":"ROLE_USER","parts":[{"text":"hi"}]}});
    let r = c
        .post(format!("http://127.0.0.1:{p}/tenants/acme/message:send"))
        .header("content-type", "application/json")
        .header("A2A-Version", "1.0")
        .json(&body)
        .send()
        .await
        .unwrap();
    let s = r.status().as_u16();
    let t: Value = r.json().await.unwrap();
    let tid = t["task"]["id"].as_str().or(t["id"].as_str()).unwrap().to_owned();
    let other = c
        .get(format!("http://127.0.0.1:{p}/tenants/evil/tasks/{tid}"))
        .header("A2A-Version", "1.0")
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    let own = c
        .get(format!("http://127.0.0.1:{p}/tenants/acme/tasks/{tid}"))
        .header("A2A-Version", "1.0")
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    println!("no resolver: send={s} tenants_seen={:?} own_get={own} other_tenant_get={other}", probe.tenants.lock().unwrap());
    assert_eq!(own, 200);
    assert_ne!(other, 200);
}

/// PerTenantConfig max_concurrent_tasks = 1 for tenant A: a second concurrent
/// send from A is refused while B is unaffected.
#[tokio::test(flavor = "multi_thread")]
async fn per_tenant_max_concurrent_tasks() {
    let (agent, _p) = CtlAgent::new();
    let cfg = PerTenantConfig::builder()
        .default_limits(TenantLimits::builder().max_concurrent_tasks(1).build())
        .build();
    let url = start(
        RequestHandlerBuilder::new(agent)
            .with_tenant_resolver(HeaderTenantResolver::new("x-tenant-id"))
            .with_task_store(TenantAwareInMemoryTaskStore::new())
            .with_tenant_config(cfg),
    )
    .await;
    let u2 = url.clone();
    let first = tokio::spawn(async move {
        jsonrpc_raw(&u2, "SendMessage", send_params_json("sleep:1500"), &[("x-tenant-id", "A")]).await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (_, _, second_a) = jsonrpc_raw(&url, "SendMessage", send_params_json("hi"), &[("x-tenant-id", "A")]).await;
    let (_, _, b) = jsonrpc_raw(&url, "SendMessage", send_params_json("hi"), &[("x-tenant-id", "B")]).await;
    let (_, _, first_body) = first.await.unwrap();
    println!("A second concurrent: {}", &second_a[..second_a.len().min(200)]);
    println!("B concurrent: {}", &b[..b.len().min(120)]);
    assert!(second_a.contains("\"error\""));
    assert!(b.contains("\"result\""));
    assert!(first_body.contains("\"result\""));
}
