<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Push config storage

Where push configs are kept, and how to keep the secrets they carry sealed at rest. The `PushConfigStore` trait itself is described in [Push-config stores](../stores/push-config-stores.md).

## Choosing a store

The default `InMemoryPushConfigStore` (the tenant-aware in-memory store when the task store isolates tenants) keeps configs in memory with per-task limits. For production use `SqlitePushConfigStore` or `PostgresPushConfigStore` (features `sqlite` / `postgres`; tenant-aware forms for multi-tenant servers), or implement `PushConfigStore` for your own database:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true).with_push_notifications(true))
# }
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# use a2a_protocol_sdk::types::push::TaskPushNotificationConfig;
use a2a_protocol_sdk::server::PushConfigStore;

struct MyPushConfigStore { /* ... */ }

impl PushConfigStore for MyPushConfigStore {
    // Implement set, get, list, delete...
#     fn set<'a>(&'a self, _: TaskPushNotificationConfig) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> { unimplemented!() }
#     fn get<'a>(&'a self, _: &'a str, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>> { unimplemented!() }
#     fn list<'a>(&'a self, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>> { unimplemented!() }
#     fn delete<'a>(&'a self, _: &'a str, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { unimplemented!() }
}

# fn f(executor: MyAgent) -> ServerResult<RequestHandler> {
RequestHandlerBuilder::new(executor)
    .with_agent_card(card())
    .with_push_config_store(MyPushConfigStore { /* ... */ })
    .with_push_sender(a2a_protocol_sdk::server::HttpPushSender::new())
    // (SQLite and PostgreSQL push-config stores ship with the crate —
    //  SqlitePushConfigStore / PostgresPushConfigStore.)
    .build()
# }
```

## Sealing stored configs

A push config carries secrets: the client's `token`, and the `credentials` the
agent presents to the webhook. The SQL push-config stores keep each config as
JSON, so without more they hold both in the clear (spec §13.2: agents SHOULD
store them securely). Give a store a `PushConfigCipher` and it writes each
config sealed, as `a2a-sealed:v1:<base64>`, and opens it on read:

```rust,no_run
# use std::future::Future;
# use std::pin::Pin;
# use std::sync::Arc;
use a2a_protocol_server::push::{PushConfigCipher, SqlitePushConfigStore};
use a2a_protocol_types::error::A2aResult;

/// An AEAD (AES-256-GCM, ChaCha20-Poly1305) whose key your KMS holds,
/// authenticating `context` as associated data.
struct KmsCipher;

impl PushConfigCipher for KmsCipher {
    fn seal<'a>(
        &'a self,
        plaintext: Vec<u8>,
        context: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<u8>>> + Send + 'a>> {
        Box::pin(async move { aead_seal(plaintext, context) })
    }

    fn open<'a>(
        &'a self,
        sealed: Vec<u8>,
        context: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<u8>>> + Send + 'a>> {
        Box::pin(async move { aead_open(sealed, context) })
    }
}
# fn aead_seal(p: Vec<u8>, _: &[u8]) -> A2aResult<Vec<u8>> { Ok(p) }
# fn aead_open(s: Vec<u8>, _: &[u8]) -> A2aResult<Vec<u8>> { Ok(s) }

# async fn f() -> Result<(), Box<dyn std::error::Error>> {
let store = SqlitePushConfigStore::new("sqlite:push.db?mode=rwc")
    .await?
    .with_cipher(Arc::new(KmsCipher));
# Ok(())
# }
```

The SDK ships no cipher, by design; bring a vetted AEAD. The server crate's
tests use AES-256-GCM from `ring` with a random 96-bit nonce per seal
(`tests/support/aes_gcm.rs`), which is safe for up to 2^32 seals under one key.

- **The context binds a config to its row**: the JSON array
  `["a2a-push-config/v1", partition, task_id, id]`, with `partition` `""` in a
  store without tenants. The cipher must authenticate it, or anyone who can
  write the table could move a sealed config to another task and receive that
  task's updates.
- **Fails closed**: with a cipher, a row that is not sealed, or one the cipher
  cannot open, is an error rather than a config. Without one, a sealed row is
  an error.
- **Migrating a table written before**: `accept_unsealed_rows()` also reads
  plain rows; each is sealed when it is next written. A config that is never
  rewritten stays plain, so find them (`data NOT LIKE 'a2a-sealed:%'` in
  SQLite, `jsonb_typeof(data) <> 'string'` in Postgres) and re-create them
  before dropping the flag.
- The task and config ids stay in their own columns, unsealed, so lookups and
  deletes work as before.
