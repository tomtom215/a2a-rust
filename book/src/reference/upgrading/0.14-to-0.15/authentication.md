<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Authentication and caller isolation

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). Each section names what breaks and what to change; the [upgrade guide](../../upgrading.md) has the policy and how to check your own code.

## An authenticator needs a scheme declared in the public card

Spec §13.3: agents "MUST authenticate requests using one of the schemes
declared in the public `AgentCard.securitySchemes`". `build()` now refuses an
authenticating interceptor (bundled or custom) behind a public card that
declares no `securitySchemes`, and a scheme declared on the extended card but
not, the same, on the public one. Declare the scheme your interceptor checks
in the public card: an `httpAuthSecurityScheme` with scheme `Bearer` for
`BearerTokenAuthInterceptor` or `JwtAuthInterceptor`, an
`apiKeySecurityScheme` naming the header for `ApiKeyAuthInterceptor`, an
`mtlsSecurityScheme` for `MutualTlsAuthInterceptor`
([Authentication](../../../building-agents/authentication/api-keys.md)
shows one), and repeat any extended-card scheme in the public card.

## A card that demands authentication needs an interceptor that enforces it

Spec §3.3.2: "Servers MUST reject requests with invalid or missing
authentication credentials"; §7.4 and §13.3 add that a server authenticates
every request by its declared requirements, with one of the schemes the public
card declares. `build()` now refuses two configurations that built before,
with `InvalidParams` naming the fix:

- A card, or a skill, whose `securityRequirements` ask for a credential when no
  registered interceptor authenticates. Such a handler served every request
  without the credential its card demanded. Register an authenticating
  interceptor (`BearerTokenAuthInterceptor`, `ApiKeyAuthInterceptor`,
  `JwtAuthInterceptor`, or your own that returns `true` from `authenticates()`),
  or, when a gateway, mesh or sidecar in front of the process checks the
  credential, say so with `RequestHandlerBuilder::authenticated_upstream()`.
  A requirement list that contains an empty requirement (`{}`, optional
  authentication) asks for nothing, and needs neither.
- An authenticating interceptor that checks none of the schemes the public card
  declares, so a client following the card could never pass it. The new
  provided method `ServerInterceptor::checks_scheme` takes a `&SecurityScheme`
  and answers `Option<bool>`: it is how an interceptor says which it checks.
  The bundled authenticators answer it, and a custom one that keeps the
  default `None` is trusted to check one of them.

[The card and the interceptors must agree](../../../building-agents/authentication.md#the-card-and-the-interceptors-must-agree)
has the rules in full. Before, then after, for a card that asks for a bearer
token that a gateway checks:

```text
// 0.14: built, and served every request without checking the token
RequestHandlerBuilder::new(agent).with_agent_card(card_with_bearer_requirements).build()?;
```

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let card: AgentCard = serde_json::from_value(serde_json::json!({
    "name": "my-agent", "description": "An example agent", "version": "1.0.0",
    "supportedInterfaces": [{
        "url": "http://localhost:3000",
        "protocolBinding": "JSONRPC",
        "protocolVersion": "1.0"
    }],
    "capabilities": {},
    "defaultInputModes": ["text/plain"],
    "defaultOutputModes": ["text/plain"],
    "skills": [{ "id": "echo", "name": "Echo", "description": "Echoes text", "tags": ["echo"] }],
    "securitySchemes": { "bearer": { "httpAuthSecurityScheme": { "scheme": "Bearer" } } },
    "securityRequirements": [{ "schemes": { "bearer": {} } }]
}))?;
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card)
    .authenticated_upstream() // the gateway in front checks the token
    .build()?;
# drop(handler);
# Ok(())
# }
```

## An authenticating interceptor isolates tasks by caller

Spec §13.1: results are scoped to the caller's authorization boundaries. A
handler with an authenticating interceptor now gives each caller it
identifies (a labelled token or key, a JWT's `sub`, or a custom interceptor's
`set_caller_identity`) a partition of its own, and another caller's task is
`TaskNotFound`. Callers it does not name keep sharing the tenant's records, so
a deployment on unlabelled tokens or keys sees no change in what each caller
can reach; its default stores do become the tenant-aware in-memory ones.
Their capacity (`with_task_store_config`, 10,000 tasks by default) now applies
to each caller's partition, up to 1,000 partitions; supply a
`TenantAwareInMemoryTaskStore` with your own `TenantStoreConfig` to bound the
total differently.

Two configurations that built before now fail `build()`, each naming its
remedy: an authenticating interceptor with a task or push-config store you
supplied that does not partition by tenant (use the `TenantAware*` stores),
and an approval gate that wants an approver other than the requester,
including the one `Profile::Auditable` installs (or let requesters approve
with `ApprovalGate::allowing_self_approval()`). An explicit
`isolate_tasks_by_caller()` without an authenticating interceptor still fails,
as before. Where callers are meant to see each other's tasks, say so:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
#         .with_security_schemes(std::collections::HashMap::from([(
#             "bearer".to_owned(),
#             a2a_protocol_types::security::SecurityScheme::Http(
#                 a2a_protocol_types::security::HttpAuthSecurityScheme {
#                     scheme: "Bearer".to_owned(),
#                     description: None,
#                     bearer_format: None,
#                 },
#             ),
#         )]))
# }
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::InMemoryTaskStore;
# struct MyAgent;
# a2a_protocol_sdk::server::agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card())
    .with_task_store(InMemoryTaskStore::new())
    .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
        ("token-a", "alice"),
        ("token-b", "bob"),
    ]))
    .share_tasks_across_callers()
    .build()?;
# drop(handler);
# Ok(())
# }
```

## A caller is named by how it authenticated

The bundled interceptors record `bearer:<label>`, `api-key:<label>` and
`jwt:<issuer>:<sub>` where they recorded the bare label or `sub`, so one `sub`
from two issuers is two callers. Two things follow here, and a third is the
next subsection.

An approver allowlist, or anything else that compares against an identity,
names the new form. `auth::caller_id` builds it:

```rust
use a2a_protocol_server::approval::ApprovalGate;
use a2a_protocol_server::auth::caller_id;

let gate = ApprovalGate::new().with_approvers([
    caller_id::bearer("alice"),
    caller_id::jwt(Some("https://idp.example"), "bob"),
]);
# let _ = gate;
```

Records a SQLite or PostgreSQL tenant-aware store keeps under caller
isolation are keyed by the old name. Move them once, before serving with
0.15, or each caller's tasks and webhooks are not found. The old key does not
say how the caller authenticated, so you give the map; the helpers cover a
deployment that used one authenticator. `migrate_caller_keys` exists on the
SQLite and PostgreSQL stores (the `sqlite` or `postgres` feature); the
in-memory stores keep nothing across a restart and need no migration:

```rust,no_run
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::store::caller_keys;
use a2a_protocol_server::store::tenant_sqlite_store::TenantAwareSqliteTaskStore;

let tasks = TenantAwareSqliteTaskStore::new("sqlite:tasks.db").await?;
let moved = tasks.migrate_caller_keys(caller_keys::bearer_labels()).await?;
println!("{} partitions, {} rows", moved.partitions, moved.rows);
# Ok(())
# }
```

Run it on the push-config store too, with its cipher set: a sealed config's
context names its partition, and the migration seals it again under the new
one. For several authenticators, pass your own `Fn(&str) -> Option<String>`
returning the new identity of each old name, or `None` to leave it.

## A JWT with no `sub` is refused under caller isolation

A JWT with no `sub` authenticates and names nobody. Under the default caller
isolation it used to be served the tenant's shared records, which handed an
anonymous token every unattributed task. It is now refused with
`PermissionDenied`, unless callers share tasks
(`RequestHandlerBuilder::share_tasks_across_callers()`), which keeps the shared
records for it. A JWT issuer that omits `sub` needs one added, or the handler
needs `share_tasks_across_callers()` as in
[An authenticating interceptor isolates tasks by caller](#an-authenticating-interceptor-isolates-tasks-by-caller).

## `JwtValidator` refuses an HS256 secret shorter than 32 bytes

RFC 7518 §3.2 requires an HS256 key at least as long as the hash output, so
`with_hs256_secret` (the `auth-jwt` feature) now takes a secret of at least
`MIN_HS256_SECRET_LEN` (32) bytes. It took any length, down to one byte, and a
guessable secret let anyone who guessed it mint tokens the server accepted. A
shorter one **fails closed**: the call logs an error and stores nothing, so
every HS256 token is rejected. A deployment configured with a short secret
therefore stops accepting HS256 tokens on upgrade, with a log line to say why.
Move to a secret of at least 32 bytes, and use `try_with_hs256_secret`, which
returns the error so the process can refuse to start. RS256 and ES256 keys are
not affected.

```text
// 0.14: any length was stored
let validator = JwtValidator::new().with_hs256_secret(b"hunter2".to_vec());
```

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::auth::jwt::JwtValidator;

assert!(JwtValidator::new().try_with_hs256_secret(b"hunter2".to_vec()).is_err());

// Load yours from a secret store; it must be at least 32 bytes.
let secret = b"0123456789abcdef0123456789abcdef".to_vec();
let validator = JwtValidator::new().try_with_hs256_secret(secret)?;
# let _ = validator;
# Ok(())
# }
```
