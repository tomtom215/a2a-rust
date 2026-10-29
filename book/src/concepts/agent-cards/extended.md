<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Extended and per-caller cards

An agent can expose a richer card via `GetExtendedAgentCard` for authenticated
clients. This requires setting `capabilities.extended_agent_card = true`:

```rust
# use a2a_protocol_sdk::prelude::*;
let capabilities = AgentCapabilities::none()
    .with_extended_agent_card(true);
```

Give the handler the card to serve with `with_extended_agent_card`: the public
card plus what only clients you know should see, such as a further skill.

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
use a2a_protocol_sdk::server::{BearerTokenAuthInterceptor, RequestHandlerBuilder};

let public = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
    .with_capabilities(AgentCapabilities::none().with_extended_agent_card(true))
    // Spec §13.3: requests are authenticated with a scheme the public card
    // declares, here the one the bearer interceptor checks.
    .with_security_schemes(std::collections::HashMap::from([(
        "bearer".to_owned(),
        a2a_protocol_types::security::SecurityScheme::Http(
            a2a_protocol_types::security::HttpAuthSecurityScheme {
                scheme: "Bearer".to_owned(),
                description: None,
                bearer_format: None,
            },
        ),
    )]));
let extended = public
    .clone()
    .with_skill(AgentSkill::new("billing", "Billing", "Account details").with_tags(["billing"]));

let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(public)
    .with_extended_agent_card(extended)
    .with_interceptor(BearerTokenAuthInterceptor::new(["s3cret"]))
    .build()
    .unwrap();
# let _ = handler;
# }
```

`build()` checks the extended card as it checks the public one, and advertises
the same extensions on it, since a client that fetches it uses its capabilities
from then on.

The answers follow §3.1.11 and §13.3:

| Public card | `with_extended_agent_card` | `GetExtendedAgentCard` answers |
|---|---|---|
| does not declare `extendedAgentCard` | — | `UnsupportedOperationError` (and `build()` refuses an extended card it would never serve) |
| declares it | set | the extended card |
| declares it | not set | `ExtendedAgentCardNotConfiguredError` |
| none (`without_agent_card()`) | set | the extended card |
| none (`without_agent_card()`) | not set | `UnsupportedOperationError` |

Until 0.15 a declared card with no extended card served the public card in its
place. To keep serving that content, pass the public card to
`with_extended_agent_card` as well.

The operation must be authenticated (§13.3): without an authenticating
interceptor (e.g. `BearerTokenAuthInterceptor`, `JwtAuthInterceptor`) the server
refuses it with `InvalidRequest`, unless you opt in with
`RequestHandlerBuilder::allow_unauthenticated_extended_card()`.

## A card per caller

§13.3 lets an agent "return different extended card content based on the
authenticated client's identity or authorization level". An
`ExtendedCardProvider` chooses the card for each request. It runs after the
interceptor chain has authenticated the caller, so
`CallContext::caller_identity()` names them (`auth::caller_id` builds the
identity to compare against). It is given the card set with
`with_extended_agent_card`, which `build()` then requires, and may return it
changed, or refuse the caller with an error.

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
use std::future::Future;
use std::pin::Pin;

use a2a_protocol_sdk::server::agent_card::ExtendedCardProvider;
use a2a_protocol_sdk::server::auth::caller_id;
use a2a_protocol_sdk::server::{
    BearerTokenAuthInterceptor, CallContext, RequestHandlerBuilder, ServerResult,
};

/// Only the caller labelled `finance` sees the billing skill.
struct BillingForFinance;

impl ExtendedCardProvider for BillingForFinance {
    fn card_for<'a>(
        &'a self,
        ctx: &'a CallContext,
        configured: &'a AgentCard,
    ) -> Pin<Box<dyn Future<Output = ServerResult<AgentCard>> + Send + 'a>> {
        let mut card = configured.clone();
        if ctx.caller_identity() != Some(caller_id::bearer("finance").as_str()) {
            card.skills.retain(|skill| skill.id != "billing");
        }
        Box::pin(async move { Ok(card) })
    }
}

let public = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
    .with_capabilities(AgentCapabilities::none().with_extended_agent_card(true))
    // Spec §13.3: requests are authenticated with a scheme the public card
    // declares, here the one the bearer interceptor checks.
    .with_security_schemes(std::collections::HashMap::from([(
        "bearer".to_owned(),
        a2a_protocol_types::security::SecurityScheme::Http(
            a2a_protocol_types::security::HttpAuthSecurityScheme {
                scheme: "Bearer".to_owned(),
                description: None,
                bearer_format: None,
            },
        ),
    )]));
let extended = public
    .clone()
    .with_skill(AgentSkill::new("billing", "Billing", "Account details").with_tags(["billing"]));

let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(public)
    .with_extended_agent_card(extended)
    .with_extended_card_provider(BillingForFinance)
    .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
        ("f-token", "finance"),
        ("s-token", "support"),
    ]))
    .build()
    .unwrap();
# let _ = handler;
# }
```

The card a provider returns is checked as the configured one is: one that is
not valid, or that declares push notifications with no push sender wired, is
not served, and the request gets an internal error saying why. The extensions
this server advertises are added to it if the provider left them out. Over
HTTP+JSON each caller's card has its own `ETag`, and the response is
`Cache-Control: private, no-cache`, so a client that revalidates gets its
own card.
