<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Agent cards and signing

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). Each section names what breaks and what to change; the [upgrade guide](../../upgrading.md) has the policy and how to check your own code.

## `build()` refuses an agent card the spec does not allow

Spec §5.7: a required array MUST contain at least one element.
`RequestHandlerBuilder::build` now runs `AgentCard::validate`, which checks
`defaultInputModes`, `defaultOutputModes`, `skills`, and every skill's `tags`,
and the card's `description` and `version`, which `a2a.proto` marks REQUIRED,
as well as the name and interfaces it checked before. It also refuses an empty
REQUIRED string of an interface (`url`, `protocolBinding`, `protocolVersion`),
of the provider (`organization`, `url`), of a skill (`id`, `name`,
`description`) and of a signature (`protected`, `signature`). A card missing
any of them used to be served as it was. Add a description and a skill:

```rust
# use a2a_protocol_sdk::prelude::*;
# let url = "http://localhost:3000";
let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc(url))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
# assert!(card.validate().is_ok());
```

`AgentCard::new` now starts both mode lists at `["text/plain"]`, so a card
built with it only needs the description and the skill. `build()` also runs
`AgentCard::validate_security_schemes`, which refuses a security scheme whose
REQUIRED string is empty, such as an API key with no `name` (and an OAuth or
OpenID Connect URL that is not a URL; see
[that subsection](#a-card-whose-oauth-or-openid-connect-urls-are-not-urls-is-refused)).
A `DynamicAgentCardHandler` whose producer
returns an invalid card answers `500`, `HotReloadAgentCardHandler` refuses
to reload one and keeps serving the previous card, and
`StaticAgentCardHandler::new` returns `InvalidParams` for one.

## `build()` requires an agent card

Spec §8.1: an A2A server MUST make an agent card available.
`RequestHandlerBuilder::build` now refuses a handler without one. Before, it
built, answered `/.well-known/agent-card.json` with 404, and skipped every
check a card would drive: streaming and push-notification capabilities, input
modes, required extensions. Give it the card your agent publishes:

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
    .with_streaming(true);
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card)
    .build()?;
# drop(handler);
# Ok(())
# }
```

Declare what the agent does: with a card, a streaming call to an agent that
does not declare `streaming` is refused, as the spec requires. A handler whose
card is published by something else, or one that is not a public A2A server
(a test), keeps the old behaviour with `.without_agent_card()`.

## A signed agent card declares the budget extension

The builder adds `https://a2a-rust.com/extensions/budget/v1` to every card,
as it adds the failure taxonomy, and a signed card cannot be edited without
breaking its signature, so `build()` refuses a signed card that does not
declare it. Add it to `capabilities.extensions` (`required: false`) before
signing. An unsigned card needs nothing.

## Cards and push configs that would leak or drop a credential are refused

`build()` and every card handler refuse a card with a credential in one of its
URLs (user information, or a query parameter named like a secret such as
`token` or `api_key`); move it out of the card. Creating a push config whose
`authentication` names a scheme but carries no credentials is refused with
`InvalidParams` on `authentication.credentials`; leave `authentication` out
for an unauthenticated webhook.

## A card whose OAuth or OpenID Connect URLs are not URLs is refused

a2a.proto says each OAuth flow URL "MUST be in the form of a URL", and that
`oauth2_metadata_url` requires TLS. `AgentCard::validate_security_schemes`,
which `build()` and every card handler run (the static, dynamic and hot-reload
handlers, and per-caller extended cards), now refuses a flow URL, refresh URL
or `openIdConnectUrl` that is not `scheme://authority...`, and an
`oauth2MetadataUrl` that is not `https`. The deprecated implicit and password
flows' URLs are checked only when given, and must be `https` (the proto: "The
OAuth2 standard requires the use of TLS"). A card that carried a relative URL
built before and does not now; write the absolute one. A client runs the same
check on a card it receives (see
[The client refuses what `a2a.proto` does not allow](./client.md#the-client-refuses-what-a2aproto-does-not-allow)).

```text
// 0.14 built; 0.15 refuses
{"oauth2SecurityScheme": {"flows": {"clientCredentials": {"tokenUrl": "/oauth/token", "scopes": {}}}}}
```

```json
{"oauth2SecurityScheme": {"flows": {"clientCredentials": {"tokenUrl": "https://idp.example/oauth/token", "scopes": {}}}}}
```

## `GetExtendedAgentCard` with no extended card configured

A card that declares `capabilities.extendedAgentCard` and a handler built
without `with_extended_agent_card` now answers `GetExtendedAgentCard` with
`ExtendedAgentCardNotConfiguredError` (`-32007`), as §3.1.11 and §13.3
require; it used to serve the public card in the extended card's place. To
serve the same content as before, pass the public card to
`with_extended_agent_card` too:

```rust
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::RequestHandlerBuilder;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
    .with_capabilities(AgentCapabilities::none().with_extended_agent_card(true));
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card.clone())
    .with_extended_agent_card(card)
    .allow_unauthenticated_extended_card()
    .build()
    .unwrap();
# let _ = handler;
# }
```

`allow_unauthenticated_extended_card()` is there because the extended card
here is the public card, so nothing secret is served. Spec §13.3 requires the
extended card endpoint to be authenticated, and by default a handler whose
card declares `extendedAgentCard` refuses to serve it unless an interceptor
authenticates; an extended card with more content than the public one needs
an authenticating interceptor instead.

A handler built with `without_agent_card()` and no extended card answers
`UnsupportedOperationError` (`-32004`) where it answered `-32007`: nothing on
it declares the capability. Both are HTTP `400`.

## `sign_agent_card` is deprecated, and refuses `None`

It takes `key_id: Option<&str>`, and in 0.14 `None` wrote a protected
header without `kid`, which spec §8.4.2 requires; it now returns
`InvalidParams` for `None`. It is removed in 0.16. Call
`sign_card_es256(card, key, key_id)`, whose `key_id` is a `&str`; with a key
id it signs what `sign_agent_card` did, apart from the two changes in the
next subsection. For Ed25519, `sign_card_ed25519`.

## A card signed by 0.14 may no longer verify

Spec §8.4.1 rule 1: the signed bytes are the card in protobuf field presence,
which leaves out a field holding its default value unless the field is
`REQUIRED` or declared `optional`. Canonicalization now does that. It drops an
empty `capabilities.extensions`, an extension's `required: false` or empty
`description`, an interface's empty `tenant`, and empty
`securityRequirements`; before, they were signed, so a card carrying one
canonicalized differently from the specification's own worked example. A
signature made by 0.14 over such a card therefore does not verify here, and
one made here does not verify under 0.14. A card with none of these fields is
unaffected. The protected header also carries `typ: "JOSE"` now (§8.4.2). The
server never signs the card it serves, so sign it again where you do, with
`sign_card_es256` or `sign_card_ed25519`, and serve the new signature:

```text
// 0.14: the signature covers the bytes 0.14 canonicalized
let signature = sign_agent_card(&card, pkcs8, Some("my-key"))?;
card.signatures = Some(vec![signature]);
```

```rust
use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::signing::sign_card_es256;

fn resign(card: &mut AgentCard, pkcs8: &[u8]) -> A2aResult<()> {
    let signature = sign_card_es256(card, pkcs8, "my-key")?;
    card.signatures = Some(vec![signature]);
    Ok(())
}
```
