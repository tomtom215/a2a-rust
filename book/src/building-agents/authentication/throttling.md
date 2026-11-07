<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Throttling guessed credentials

Slow down a client that keeps presenting wrong credentials, without telling it which guess was right.

## Registering the throttle

Spec §13.4 asks agents to rate-limit against brute force. `RateLimitInterceptor`
cannot do it: registered after the authenticator, it never sees a refused
credential; registered before, it cannot tell one unidentified caller from
another. `AuthFailureThrottle` counts, per peer, the calls an authenticator
refused as unauthenticated, and once a peer reaches `max_failures` in a window
it refuses that peer's calls as rate-limited until the window ends, the right
credential included, so a guesser cannot tell a hit from a miss. Register it
**before** the authenticator:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_security_schemes(std::collections::HashMap::from([(
#             "bearer".to_owned(),
#             a2a_protocol_types::security::SecurityScheme::Http(
#                 a2a_protocol_types::security::HttpAuthSecurityScheme::new("Bearer"),
#             ),
#         )]))
# }
# use a2a_protocol_sdk::prelude::agent_executor;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::{
    AuthFailureThrottle, AuthFailureThrottleConfig, BearerTokenAuthInterceptor,
    RequestHandlerBuilder,
};

let throttle = AuthFailureThrottle::new(
    AuthFailureThrottleConfig::default()
        .with_max_failures(10) // refused credentials per peer per window
        .with_window_secs(300),
)?;
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card())
    .with_interceptor(throttle)
    .with_interceptor(BearerTokenAuthInterceptor::new(["service-token-1"]))
    .build()?;
# Ok(())
# }
```

A throttled call answers `429` with `Retry-After` on HTTP+JSON, `200` with a
`-32603` body on JSON-RPC, and `RESOURCE_EXHAUSTED` on gRPC; every binding
carries a `RetryInfo` detail giving the seconds until the window ends.

## Several replicas

By default the counts live in the memory of the replica that saw the failures.
Behind a balancer that spreads one peer's requests over N replicas, each replica
sees about 1/N of that peer's failures, so the peer gets up to N × `max_failures`
guesses per window before every replica refuses it. Two ways to close that:

- **Share the count.** `with_shared_counter` takes the same `RateLimitCounter`
  the rate limiter takes (`PostgresRateLimitCounter` ships; see
  [Rate limiting across replicas](../../deployment/horizontal-scaling/rate-limiting.md)):

  ```rust,no_run
  use std::sync::Arc;
  use a2a_protocol_server::{AuthFailureThrottle, AuthFailureThrottleConfig, RateLimitCounter};

  # fn build(counter: Arc<dyn RateLimitCounter>) -> Result<(), Box<dyn std::error::Error>> {
  let throttle = AuthFailureThrottle::new(AuthFailureThrottleConfig::default())?
      .with_shared_counter(counter);
  # drop(throttle);
  # Ok(())
  # }
  ```

  Each refused credential is also counted in the counter, under the key
  `authfail:` and the peer, and the throttle keeps the larger of its own count
  and the total the counter reports. The counter is **not** read on a call's way
  in, so a call with a valid credential costs no round trip; only a refused
  credential does, and a call a replica refuses as throttled does not. The price
  is that a replica learns the fleet has reached the limit when one of its own
  calls fails: after the limit is reached, a peer gets at most one more refused
  credential for each replica it has not yet failed at, plus the calls already
  in flight, where without the counter it gets `max_failures` from each. If the
  counter cannot be reached the failure is counted on this replica only and a
  warning is logged, which is the per-process behaviour above. A peer beyond
  `max_peers` on a replica is not counted anywhere. The throttle and the rate
  limiter can share one `PostgresRateLimitCounter` and its table even though
  their windows differ (300 s and 60 s by default): each row records the width
  it was counted in, and a sweep removes only its own width's rows.
- **Size it for the fleet.** Divide the number of guesses you want by N and set
  `max_failures` to that, or throttle in the proxy in front, which sees every
  request.

## Who counts as a peer

A peer is the IP address of the connection (`CallContext::peer_addr`), which
every binding records when the SDK serves it. The axum router has it only when
served with `into_make_service_with_connect_info::<SocketAddr>()`. Behind
reverse proxies, set `with_trusted_proxy_hops` to the number of proxies so the
client address they append to `x-forwarded-for` is counted instead, and name
their address ranges with `with_trusted_proxies` so that a connection from
anywhere else cannot choose the address that is counted by sending the header
itself (hops with no ranges log a warning when the throttle is built). A proxy
that writes RFC 7239 `Forwarded` rather than `x-forwarded-for` is read with
`with_forwarded_header(ForwardedHeader::Forwarded)`. An IPv6 peer is counted by its `/64`, as the rate limiter keys it: a
host is routinely given a whole `/64` and could otherwise rotate addresses
without reaching the limit. A call with no known address is not throttled.

## What to weigh

- Everyone behind one address (a NAT, an untrusted proxy) shares a count, so a
  peer that keeps failing locks the others there out for the window.
- A failure is counted when its call ends, so calls already in flight when a
  peer reaches the limit are still checked.
- At most `max_peers` peers are tracked (10,000 by default). When the table is
  full, peers whose window has passed are dropped, and a new peer that still
  does not fit is not counted.
