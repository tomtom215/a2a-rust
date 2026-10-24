// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `BIND-EQUIV-001` (spec §5.1): "all supported protocols MUST provide the
//! same set of operations and capabilities."
//!
//! [`sweep`] performs all eleven operations through one client, each on its
//! success path, and the agent records what reached it. Every binding must
//! complete the sweep and deliver exactly the operations in `Method::ALL`.

use std::collections::BTreeSet;

use a2a_protocol_types::method::Method;

use crate::harness::{Binding, serve};
pub use crate::sweep::*;

pub fn every_method() -> BTreeSet<String> {
    Method::ALL
        .iter()
        .map(|m| m.wire_name().to_owned())
        .collect()
}

/// BIND-EQUIV-001: every binding serves every operation, and its success
/// path, against one agent.
#[tokio::test]
async fn bind_equiv_001_every_operation_succeeds_on_every_binding() {
    let agent = serve(false, true).await;
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        if let Err(e) = sweep(&client).await {
            panic!("{binding:?}: {e}");
        }
        let reached: BTreeSet<String> = agent.take_seen().into_iter().map(|(m, _)| m).collect();
        assert_eq!(
            reached,
            every_method(),
            "{binding:?} must deliver exactly the operations in Method::ALL"
        );
    }
}

/// Drops every `false`. `AgentExtension.required` is a proto3 `bool`
/// without `optional`, so `false` and absent are one value: gRPC's encoding
/// cannot tell them apart and decodes it as absent, where the JSON bindings
/// carry the `false` this SDK serialises. Semantically the same card.
fn implicit_false_dropped(mut value: serde_json::Value) -> serde_json::Value {
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(map) => {
                map.retain(|_, v| *v != serde_json::Value::Bool(false));
                map.values_mut().for_each(strip);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut value);
    value
}

/// BIND-EQUIV-001's capability half: the card each binding's client
/// fetches through `GetExtendedAgentCard` is the same card, so a client
/// learns the same capabilities whichever binding it picked.
#[tokio::test]
async fn bind_equiv_001_every_binding_serves_the_same_capabilities() {
    let agent = serve(false, true).await;
    let mut cards = Vec::new();
    for binding in Binding::ALL {
        let client = agent.client(binding, None).await;
        let card = client
            .get_extended_agent_card()
            .await
            .unwrap_or_else(|e| panic!("{binding:?}: {e:?}"));
        cards.push((
            binding,
            implicit_false_dropped(serde_json::to_value(&card).expect("card")),
        ));
    }
    let (first, expected) = &cards[0];
    assert_eq!(expected["capabilities"]["streaming"], true);
    assert_eq!(expected["capabilities"]["pushNotifications"], true);
    for (binding, card) in &cards[1..] {
        assert_eq!(
            card, expected,
            "{binding:?} and {first:?} serve different cards"
        );
    }
}
