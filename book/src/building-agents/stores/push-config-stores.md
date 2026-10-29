<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Push-config stores

A push-config store holds the webhook configurations clients register for their tasks. The SQL variants, and sealing the secrets a config carries, are covered in [Push config storage](../push-notifications/storage.md).

## The `PushConfigStore` trait

The `PushConfigStore` trait manages push notification configurations:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::types::error::A2aResult;
# use a2a_protocol_sdk::types::push::TaskPushNotificationConfig;
pub trait PushConfigStore: Send + Sync + 'static {
    fn set<'a>(&'a self, config: TaskPushNotificationConfig)
        -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>>;

    fn get<'a>(&'a self, task_id: &'a str, id: &'a str)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>>;

    fn list<'a>(&'a self, task_id: &'a str)
        -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>>;

    fn delete<'a>(&'a self, task_id: &'a str, id: &'a str)
        -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    // Optional: fn isolates_tenants(&self) -> bool, false by default (see
    // Tenant-aware stores).

    // Optional: total stored configs, counted against the handler's global
    // push-config ceiling. The default implementation returns `None`
    // (only the per-task cap is then enforced), so existing custom
    // implementations keep compiling unchanged.
    fn count(&self)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<usize>>> + Send + '_>> {
        Box::pin(async { Ok(None) })
    }
}
# // The real trait requires exactly these four; this impl of it compiles.
# struct Probe;
# impl a2a_protocol_sdk::server::PushConfigStore for Probe {
#     fn set<'a>(&'a self, c: TaskPushNotificationConfig) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> { Box::pin(async { Ok(c) }) }
#     fn get<'a>(&'a self, _: &'a str, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>> { Box::pin(async { Ok(None) }) }
#     fn list<'a>(&'a self, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>> { Box::pin(async { Ok(vec![]) }) }
#     fn delete<'a>(&'a self, _: &'a str, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
# }
```

## `InMemoryPushConfigStore`

The default implementation stores configs in a `HashMap` with a secondary index for efficient per-task counting:

```rust,no_run
use a2a_protocol_sdk::server::InMemoryPushConfigStore;

let store = InMemoryPushConfigStore::new();
```

Features:
- Config IDs (a UUID is assigned when the config names none)
- Per-task config limits (prevents abuse) — uses a secondary index (`task_counts`) for O(1) per-task count lookups instead of scanning all keys
- Global config limit (`max_total_configs`, default 100,000) — prevents unbounded memory growth across all tasks
- Thread-safe access
