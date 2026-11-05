// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A `GetTask` to a stalled replica is answered by its hedge; a blocking
//! send without an idempotency key is never copied. The timings to the
//! millisecond are `balance::tests`'.

use std::time::Duration;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::balance::{BalancePolicy, Hedge, StaticEndpoints};
use a2a_protocol_types::{Message, MessageSendParams, Part};

use super::harness::{Mode, Replica, get, urls};

async fn stalled_pair() -> Vec<std::sync::Arc<Replica>> {
    let replicas = vec![Replica::start("a").await, Replica::start("b").await];
    replicas[0].set(Mode::Stall);
    replicas
}

#[tokio::test]
async fn a_stalled_get_task_is_answered_by_the_other_replica() {
    let replicas = stalled_pair().await;
    let client = ClientBuilder::new(replicas[0].url())
        .with_balancing(
            StaticEndpoints::new(urls(&replicas)),
            BalancePolicy::new().with_hedge(Hedge::after(Duration::from_millis(50))),
        )
        .build()
        .unwrap();
    assert_eq!(get(&client).await.expect("hedged"), "b");
    assert_eq!(replicas[0].methods(), ["GetTask"]);
    assert_eq!(replicas[1].methods(), ["GetTask"]);
}

#[tokio::test]
async fn a_send_without_a_key_is_not_copied() {
    let replicas = stalled_pair().await;
    let client = ClientBuilder::new(replicas[0].url())
        .with_send_timeout(Duration::from_millis(400))
        .with_balancing(
            StaticEndpoints::new(urls(&replicas)),
            BalancePolicy::new().with_hedge(Hedge::after(Duration::from_millis(20))),
        )
        .build()
        .unwrap();
    let params = MessageSendParams::new(Message::user("m-1", vec![Part::text("go")]));
    client
        .send_message(params)
        .await
        .expect_err("timed out on a");
    assert_eq!(replicas[0].methods(), ["SendMessage"]);
    assert_eq!(replicas[1].requests(), 0, "no copy of an unkeyed send");
}
