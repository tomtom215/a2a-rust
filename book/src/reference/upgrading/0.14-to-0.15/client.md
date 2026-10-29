<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Client

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). Each section names what breaks and what to change; the [upgrade guide](../../upgrading.md) has the policy and how to check your own code.

## The client refuses what `a2a.proto` does not allow

Spec §5.7: implementations SHOULD reject a message missing a field the proto
marks REQUIRED. The client now holds what it receives to the rules the server
holds its own executors to, on every binding. A task with an empty `id`, a
status in `TASK_STATE_UNSPECIFIED`, a message without a `messageId`, `role` or
parts, an artifact without an `artifactId` or parts, a status or artifact
update without its `taskId` or `contextId`, and a push config without a `url`
or with an `authentication` that has no `scheme`, fail the call with
`ClientError::InvalidAgentResponse`, which names the call and the field; on a
stream, the event ends the stream with that error. They used to be returned as
if valid. Over gRPC such a field absent on the wire (proto3's `""` or `0`), or
a card's absent `capabilities`, is refused by the conversion where it used to
read as its zero value ([Wire format and bindings](./wire-and-bindings.md#the-grpc-conversion-refuses-an-absent-required-field)),
and any answer the conversion refuses is now `InvalidAgentResponse` rather
than `Transport`. Match `InvalidAgentResponse` to tell a non-conformant agent
from a transport fault; `ClientError` is `#[non_exhaustive]`, so the new
variant breaks no `match`.

An agent card fetched by `resolve_agent_card`, `fetch_card_from_url` or
`CachingCardResolver`, or returned by `get_extended_agent_card`, must pass
`AgentCard::validate` and `AgentCard::validate_security_schemes`, the checks
`build()` runs ([Agent cards and signing](./agent-cards.md#build-refuses-an-agent-card-the-spec-does-not-allow)); the deprecated implicit and password OAuth flows'
URLs must also be `https` (the proto: "The OAuth2 standard requires the use of
TLS"). A refused extended card is not adopted. `ClientBuilder::from_card` does
not validate the card it is given, but no longer selects an interface whose
`protocolVersion` is empty.

Two requests change on the wire: `list_tasks` sends `status_timestamp_after`
in UTC with a `Z` (a value with an offset is rewritten; one that is not ISO
8601 is refused, unsent, with `InvalidParams`), and an inline push config's
`task_id` is no longer sent with `SendMessage`, as the proto asks.

## `ClientBuilder::from_card` follows the card's interface order

Spec §8.3.2: a client MUST select the first interface it supports, preferring
earlier entries in the card's list. `from_card` used to prefer JSON-RPC
wherever the card listed it; it now takes the card's first interface for
protocol 1.x and falls back down the list past one it cannot build. A card
listing `HTTP+JSON` before `JSONRPC` now gets HTTP+JSON. To keep the old
choice, rank the bindings yourself:

```rust
# use a2a_protocol_sdk::prelude::*;
# fn main() -> Result<(), ClientError> {
# let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::rest("http://localhost:3001"))
#     .with_description("An example agent")
#     .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
let client = ClientBuilder::from_card_preferring(&card, &["JSONRPC".into()])?.build()?;
# drop(client);
# Ok(())
# }
```

`ClientConfig::preferred_bindings` and its setter are deprecated: nothing
reads them. Use `from_card_preferring` to rank, and
`ClientBuilder::chosen_interface` to see what was chosen.

## A client built from a card checks the card's capabilities

Spec §3.3.4: a client SHOULD check the agent card before an operation that
needs an optional capability. A client from `ClientBuilder::from_card` (or
`A2aClient::from_card`) now does: streaming and resubscription need
`capabilities.streaming`, the push-config methods and a send carrying an
inline push config need `pushNotifications`, and `get_extended_agent_card`
needs `extendedAgentCard`, each set to `true`. Otherwise the call returns
`ClientError::CapabilityNotDeclared` and nothing is sent.

A conformant agent refuses the same calls, so code that already handled
`UnsupportedOperationError` or `PushNotificationNotSupportedError` from such
an agent should handle this error the same way. For an agent that streams or
takes push configs without declaring it, opt out:

```rust
# use a2a_protocol_sdk::prelude::*;
# fn main() -> Result<(), ClientError> {
# let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3001"))
#     .with_description("An example agent")
#     .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
let client = ClientBuilder::from_card(&card)?
    .without_capability_checks()
    .build()?;
# drop(client);
# Ok(())
# }
```

The CLI's equivalent is `--no-capability-check`. A client built for a bare
endpoint with `ClientBuilder::new` has no card and checks nothing.

## A client built from a card refuses a required extension it did not ask for

a2a.proto, `AgentExtension.required`: "If true, the client must understand and
comply with the extension's requirements". When the card marks an extension
`required: true` and `with_extensions` does not ask for its URI,
`ClientBuilder::build` now fails with `ClientError::Protocol` carrying code
`ErrorCode::ExtensionSupportRequired`, whose message names the extension; it
used to build, and the agent refused every call. Ask for the extension, or
call `skip_required_extension_check()` when an interceptor of yours sends the
`A2A-Extensions` header instead. A client built with `ClientBuilder::new` has
no card and is unaffected.

```rust
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_types::extensions::AgentExtension;
# fn main() -> Result<(), ClientError> {
const CONSENT: &str = "https://ext.example/consent/v1";
let mut consent = AgentExtension::new(CONSENT);
consent.required = Some(true);
let mut card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3001"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
card.capabilities.extensions = Some(vec![consent]);

let client = ClientBuilder::from_card(&card)?
    .with_extensions([CONSENT])
    .build()?;
# drop(client);
# Ok(())
# }
```

## `CachingCardResolver` trusts the card's `max-age`

Spec §8.6.2: clients SHOULD honor RFC 9111 when fetching agent cards.
`CachingCardResolver::resolve` used to send a request on every call; it now
returns the stored card without one while the card is fresh, for its
`Cache-Control: max-age` (or `Expires`). A card the agent changes is seen
once the stored one goes stale, which against this SDK's servers is up to
`agent_card_max_age` (default 3600 s) later. To see a change at once, as
before, build the resolver with `.with_always_revalidate(true)`, or lower
the server's `max-age`. `with_options` now also starts the resolver with an
empty cache, so a clone given other options no longer shares its original's
stored or extended card.

## The REST client sends `application/a2a+json`

The HTTP+JSON client labels a request body `application/a2a+json`, as §11.1
asks, and a non-streaming call asks for
`application/a2a+json, application/json` (a streaming call still asks for
`text/event-stream`). A GET or DELETE has no body and now no `Content-Type`.
a2a-java 1.3.0.Final and older refuse a request labelled anything but
`application/json` (`ContentTypeNotSupportedError`); 1.4.0.Final accepts it. A
client that must reach an older a2a-java server uses its JSON-RPC interface.

## `ClientRequest` gains `query_params`

An API key that the agent's card places in the query string is sent there
now, so a request reaches the transports with query parameters as well as
headers. `ClientRequest` has a new public field, `query_params`, and a struct
literal that names every field no longer compiles. Build the request with
`ClientRequest::new`, which leaves it empty:

```rust
use a2a_protocol_client::ClientRequest;

let mut req = ClientRequest::new("GetTask", serde_json::json!({ "id": "t-1" }));
assert!(req.query_params.is_empty());
req.query_params.push(("api_key".into(), "k".into()));
```

A custom `Transport` still compiles: the new `send_request_with_query` and
`send_streaming_request_with_query` methods have defaults, which send a call
with no query parameters as before and refuse one with them
(`ClientError::InvalidAuthScheme`). Override both if your transport has a URL
to put them in. gRPC, WebSocket and the SLIMRPC binding refuse such a call.
