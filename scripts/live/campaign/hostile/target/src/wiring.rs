// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Builds the handler from the environment (see `main.rs` for the table).

use std::sync::Arc;
use a2a_protocol_server::push::{HttpPushSender, InMemoryPushConfigStore};
use a2a_protocol_server::store::encrypted::{Aes256GcmCipher, EncryptedTaskStore};
use a2a_protocol_server::{
    ApiKeyAuthInterceptor, AuthFailureThrottle, AuthFailureThrottleConfig,
    BearerTokenAuthInterceptor, BearerTokenTenantResolver, HeaderTenantResolver, InMemoryTaskStore,
    PathSegmentTenantResolver, PostgresPushConfigStore, PostgresRateLimitCounter, PostgresTaskStore,
    RequestHandler, RequestHandlerBuilder, SqlitePushConfigStore, SqliteTaskStore, SteeringConfig,
    TenantAwareInMemoryPushConfigStore, TenantAwareInMemoryTaskStore,
    TenantAwarePostgresPushConfigStore, TenantAwarePostgresTaskStore,
    TenantAwareSqlitePushConfigStore, TenantAwareSqliteTaskStore,
};
use a2a_protocol_server::auth::{Jwks, JwtAuthInterceptor, JwtValidator};
use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::{
    ApiKeyLocation, ApiKeySecurityScheme, HttpAuthSecurityScheme, SecurityScheme,
};
use std::collections::HashMap;

use crate::exec::Exec;
use crate::{env, secs};

fn card(port: &str) -> AgentCard {
    AgentCard::new(
        "Hostile Target",
        "1.0.0",
        AgentInterface::jsonrpc(format!("http://127.0.0.1:{port}")),
    )
    .with_interface(AgentInterface::rest(format!("http://127.0.0.1:{port}")))
    .with_description("the campaign's target")
    .with_skill(AgentSkill::new("s", "S", "s").with_tags(["t"]))
    .with_capabilities(
        AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true)
            .with_extended_agent_card(true),
    )
    .with_security_schemes(match env("AUTH").as_deref() {
        None | Some("none") => HashMap::new(),
        Some("apikey") => HashMap::from([(
            "key".to_owned(),
            SecurityScheme::ApiKey(ApiKeySecurityScheme::new(ApiKeyLocation::Header, "x-api-key")),
        )]),
        Some(_) => HashMap::from([(
            "bearer".to_owned(),
            SecurityScheme::Http(HttpAuthSecurityScheme::new("Bearer".to_owned())),
        )]),
    })
}

fn cipher() -> Arc<Aes256GcmCipher> {
    Aes256GcmCipher::new("k1", [7u8; 32]).expect("cipher").into_arc()
}

fn pool() -> u32 {
    env("POOL").and_then(|v| v.parse().ok()).unwrap_or(8)
}

fn pg_url() -> String {
    env("DATABASE_URL").expect("DATABASE_URL")
}

/// The tokens: `alice=tokA,bob=tokB`.
fn tokens() -> Vec<(String, String)> {
    env("TOKENS")
        .unwrap_or_else(|| "alice=tokA,bob=tokB".into())
        .split(',')
        .filter_map(|p| p.split_once('='))
        .map(|(l, t)| (t.to_owned(), l.to_owned()))
        .collect()
}

pub async fn handler() -> RequestHandler {
    let port = env("PORT").unwrap_or_default();
    let mut b = RequestHandlerBuilder::new(Exec)
        .with_agent_card(card(&port))
        .with_extended_agent_card(card(&port));
    let store = env("STORE").unwrap_or_else(|| "mem".into());
    let mut push_pg = false;
    b = match store.as_str() {
        "mem" => b.with_task_store(InMemoryTaskStore::new()),
        "tmem" => b
            .with_task_store(TenantAwareInMemoryTaskStore::new())
            .with_push_config_store(TenantAwareInMemoryPushConfigStore::new()),
        "sqlite" => {
            let url = env("SQLITE_URL").unwrap_or_else(|| "sqlite::memory:".into());
            b.with_task_store(SqliteTaskStore::with_migrations(&url).await.expect("sqlite"))
                .with_push_config_store(SqlitePushConfigStore::new(&url).await.expect("push"))
        }
        "tsqlite" => {
            let url = env("SQLITE_URL").unwrap_or_else(|| "sqlite::memory:".into());
            b.with_task_store(TenantAwareSqliteTaskStore::new(&url).await.expect("tsqlite"))
                .with_push_config_store(
                    TenantAwareSqlitePushConfigStore::new(&url).await.expect("push"),
                )
        }
        "pg" | "enc-pg" => {
            push_pg = true;
            let s = PostgresTaskStore::new_with_pool_size(&pg_url(), pool()).await.expect("pg");
            let s = match env("LOG_NOTIFY").as_deref() {
                Some("direct") => s.with_log_notifications(
                    a2a_protocol_server::store::LogListen::Direct(pg_url()),
                ),
                Some("pool") => {
                    s.with_log_notifications(a2a_protocol_server::store::LogListen::Pool)
                }
                _ => s,
            };
            let pc = PostgresPushConfigStore::new_with_pool_size(&pg_url(), pool().min(4)).await.expect("push");
            if store == "enc-pg" {
                b.with_task_store(EncryptedTaskStore::new(s, cipher())).with_push_config_store(pc)
            } else {
                b.with_task_store(s).with_push_config_store(pc)
            }
        }
        "tpg" => {
            push_pg = true;
            let s = TenantAwarePostgresTaskStore::new_with_pool_size(&pg_url(), pool())
                .await
                .expect("tpg");
            b.with_task_store(s).with_push_config_store(
                TenantAwarePostgresPushConfigStore::new(&pg_url()).await.expect("push"),
            )
        }
        "enc-mem" => b
            .with_task_store(EncryptedTaskStore::new(InMemoryTaskStore::new(), cipher()))
            .with_push_config_store(InMemoryPushConfigStore::new()),
        other => panic!("unknown STORE {other}"),
    };
    let _ = push_pg;
    b = match env("PUSH").as_deref() {
        Some("off") => b,
        Some("private") => b.with_push_sender(HttpPushSender::new().allow_private_urls()),
        _ => b.with_push_sender(HttpPushSender::new()),
    };
    if let Some(n) = env("THROTTLE").and_then(|v| v.parse::<u32>().ok()) {
        let hops = env("THROTTLE_HOPS").and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut t = AuthFailureThrottle::new(
            AuthFailureThrottleConfig::default()
                .with_max_failures(n)
                .with_window_secs(env("THROTTLE_WINDOW").and_then(|v| v.parse().ok()).unwrap_or(60))
                .with_trusted_proxy_hops(hops),
        )
        .expect("throttle");
        if env("THROTTLE_PG").is_some() {
            let c = PostgresRateLimitCounter::new(&pg_url()).await.expect("counter");
            t = t.with_shared_counter(Arc::new(c));
        }
        b = b.with_interceptor(t);
    }
    b = match env("AUTH").as_deref().unwrap_or("none") {
        "bearer" => b.with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens(tokens())),
        "apikey" => b.with_interceptor(ApiKeyAuthInterceptor::with_labelled_keys(tokens())),
        "jwt" => {
            let v = JwtValidator::new()
                .with_issuer(env("JWT_ISS").unwrap_or_else(|| "iss".into()))
                .with_audience(env("JWT_AUD").unwrap_or_else(|| "aud".into()))
                .try_with_hs256_secret(env("JWT_SECRET").expect("JWT_SECRET").into_bytes())
                .expect("secret");
            match env("JWKS_URL") {
                Some(u) => b.with_interceptor(JwtAuthInterceptor::from_jwks_url(v, u)),
                None => b.with_interceptor(JwtAuthInterceptor::new(v, Jwks::from_json(
                    br#"{"keys":[]}"#,
                ).expect("jwks"))),
            }
        }
        _ => b.allow_unauthenticated_extended_card(),
    };
    b = match env("TENANT").as_deref().unwrap_or("none") {
        "path" => b.with_tenant_resolver(PathSegmentTenantResolver::new(0)),
        "header" => b.with_tenant_resolver(HeaderTenantResolver::default()),
        "token" => b.with_tenant_resolver(BearerTokenTenantResolver::with_mapper(|t| {
            Some(format!("t-{t}"))
        })),
        _ => b,
    };
    if env("ISOLATE").is_some() {
        b = b.isolate_tasks_by_caller();
    }
    if env("STEER").is_some() {
        let mut c = SteeringConfig::new();
        if let Some(n) = env("STEER_CAP").and_then(|v| v.parse().ok()) {
            c = c.with_inbox_capacity(n);
        }
        if let Some(n) = env("STEER_MAX").and_then(|v| v.parse().ok()) {
            c = c.with_max_message_size(n);
        }
        b = b.with_steering(c);
    }
    if let Some(t) = secs("EXEC_TIMEOUT_SECS") {
        b = b.with_executor_timeout(t);
    }
    if let Some(n) = env("QUEUE_CAP").and_then(|v| v.parse().ok()) {
        b = b.with_event_queue_capacity(n);
    }
    if let (Some(ttl), Some(renew)) = (
        env("CLAIM_TTL_MS").and_then(|v| v.parse::<u64>().ok()),
        env("CLAIM_RENEW_MS").and_then(|v| v.parse::<u64>().ok()),
    ) {
        b = b.with_turn_claim_timing(
            std::time::Duration::from_millis(ttl),
            std::time::Duration::from_millis(renew),
        );
    }
    if env("NAME").is_some() {
        b = b.with_replica_name(env("NAME").unwrap_or_default());
    }
    b.build().expect("handler")
}

/// With `PARKED_MS` and `PURGE_MS`, a second store on the same database that
/// runs `purge_expired` with `parked_max_age` every `PURGE_MS`, as an
/// operator's retention job would. PostgreSQL and tenant-aware PostgreSQL only.
pub async fn spawn_purger() {
    use a2a_protocol_server::store::retention::RetentionPolicy;
    let (Some(parked), Some(every)) = (
        env("PARKED_MS").and_then(|v| v.parse::<u64>().ok()),
        env("PURGE_MS").and_then(|v| v.parse::<u64>().ok()),
    ) else {
        return;
    };
    let policy = RetentionPolicy::new(std::time::Duration::from_secs(3600))
        .with_parked_max_age(std::time::Duration::from_millis(parked));
    let tenant = env("STORE").unwrap_or_default() == "tpg";
    let plain = PostgresTaskStore::new_with_pool_size(&pg_url(), 2)
        .await
        .expect("purger store");
    let tenanted = TenantAwarePostgresTaskStore::new_with_pool_size(&pg_url(), 2)
        .await
        .expect("purger store");
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(every)).await;
            let r = if tenant {
                tenanted.purge_expired(&policy).await
            } else {
                plain.purge_expired(&policy).await
            };
            match r {
                Ok(rep) => eprintln!("[purge] {rep:?}"),
                Err(e) => eprintln!("[purge] error: {e}"),
            }
        }
    });
}
