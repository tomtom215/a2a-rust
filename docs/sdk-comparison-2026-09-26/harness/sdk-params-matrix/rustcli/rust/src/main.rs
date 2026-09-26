// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use a2a_protocol_sdk::client::discovery::resolve_agent_card;
use a2a_protocol_sdk::prelude::*;
#[tokio::main]
async fn main() {
    let base = std::env::args().nth(1).unwrap();
    let card = resolve_agent_card(&base).await.unwrap();
    let client = ClientBuilder::from_card_preferring(&card, &["JSONRPC".to_string()]).unwrap().build().unwrap();
    match client.get_extended_agent_card().await {
        Ok(c) => println!("got card: {}", c.name),
        Err(e) => println!("error: {e}"),
    }
}
