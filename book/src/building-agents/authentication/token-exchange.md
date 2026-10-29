<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Delegating for a principal: RFC 8693 token exchange

An agent serving Alice that delegates to another agent should send neither
its own credential (it names only the agent) nor Alice's (the next agent
was never meant to accept it). It exchanges the token it was called with at
the identity provider for one issued for the next agent: `sub` is still
Alice, and `act` names the agent acting for her
([ADR 0021](../../reference/adrs.md)).

```rust,no_run
use a2a_protocol_client::token_provider::{ExchangeRequest, TokenExchange};
# async fn run(alices_token: String) -> Result<(), Box<dyn std::error::Error>> {
let idp = TokenExchange::new("https://idp.example/oauth/token", "planner-agent", "secret");
let delegated = idp
    .exchange(
        &ExchangeRequest::new(alices_token)
            .with_audience("https://worker.example")
            .with_scopes(["tasks:read"]),
    )
    .await?;
# let _ = delegated;
# Ok(())
# }
```

Send the call with `delegated.access_token`. The exchanged token belongs to
one principal, so give it to that call alone, for example on a client
built with `StaticTokenProvider`, not to a client every principal shares.
On the receiving agent, `JwtAuthInterceptor` makes Alice the caller
identity and lists the acting agents in `CallContext::actors`, nearest
first. With `audit`, every call record names both. The identity provider
decides what may be delegated, to whom, and with which scopes.

For cancelling and supervising the child task the delegated call starts, see
[Delegating to Another Agent](../../client/delegation.md).
