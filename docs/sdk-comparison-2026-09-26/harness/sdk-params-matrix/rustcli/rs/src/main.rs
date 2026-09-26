// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use a2a::*;
use a2a_client::A2AClientFactory;
use a2a_client::agent_card::AgentCardResolver;
#[tokio::main]
async fn main() {
    let base = std::env::args().nth(1).unwrap();
    let card = AgentCardResolver::new(None).resolve(&base).await.unwrap();
    let factory = A2AClientFactory::builder().preferred_bindings(vec!["JSONRPC".to_string()]).build();
    let (client, _iface) = factory.create_from_card_with_interface(&card).await.unwrap();
    match client.get_extended_agent_card(&GetExtendedAgentCardRequest { tenant: None }).await {
        Ok(c) => println!("got card: {}", c.name),
        Err(e) => println!("error: {e}"),
    }
}
