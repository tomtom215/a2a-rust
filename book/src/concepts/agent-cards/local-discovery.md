<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Finding agents on the local network

`a2a-protocol-discovery` announces agents and finds them with multicast DNS
service discovery (RFC 6762, RFC 6763), so agents on one LAN, or in one
compose project, need not be given each other's URLs. It is a separate crate,
not published yet ([ADR 0029](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0029-local-network-discovery.md)).

An agent announces the port it serves on; the announcement lasts until the
`Announcer` is dropped, when a goodbye removes it from every browser at once:

```rust,no_run
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_discovery::{Advertisement, Announcer, Scheme};

let _announcer = Announcer::announce(
    &Advertisement::new("echo-1", 8080)          // instance name, unique on the link
        .with_scheme(Scheme::Https)             // default: http
        .with_card_path("/.well-known/agent-card.json"), // the default
)?;
# Ok(())
# }
```

A client takes a snapshot, or follows changes as they happen:

```rust,no_run
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
use std::time::Duration;
use a2a_protocol_discovery::{discover, Browser, DiscoveryEvent};

for agent in discover(Duration::from_secs(3)).await? {
    let card = agent.fetch_card(Default::default()).await?;
    println!("{} at {:?}", card.name, agent.card_urls());
}

let mut browser = Browser::start()?;
while let Some(event) = browser.next().await {
    match event {
        DiscoveryEvent::Found(agent) => println!("+ {}", agent.instance),
        DiscoveryEvent::Lost(name) => println!("- {name}"),
        _ => {}
    }
}
# Ok(())
# }
```

Answers arrive as queries repeat, so a window under a second can miss agents
that are there. `cargo run -p a2a-protocol-discovery --example browse` lists
what is announced, with each card's name and skills.

## Trust

mDNS has no authentication: anything on the link can announce any name, or
withdraw one. An announcement says where a card claims to be. When what you
find decides what an agent may do, require its card to be signed by a key you
trust: `CardFetchOptions::with_signature_keys` (feature `signing`), passed to
`fetch_card`. Card URLs leave out loopback addresses, which would name the
browsing host itself, and IPv6 link-local ones.

## Limits

- One link: multicast does not cross routers, and rarely a Kubernetes pod
  network, where DNS and Services already find agents.
- No registry, catalogue or capability search.
- The push sender refuses `.local` names and private addresses by default, so
  a discovered agent gets webhooks only once the sender allows private URLs.
