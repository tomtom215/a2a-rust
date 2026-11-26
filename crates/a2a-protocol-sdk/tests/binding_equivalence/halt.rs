// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! COR-05: a halted server's refusal is final on every binding. A client
//! with a retry policy, sending a keyed message to an agent it knows honours
//! keys (the send it would retry), sends once and reads the refusal as a
//! halt. Before COR-05, gRPC reported the halt as a dropped connection and
//! retried it, and HTTP+JSON dropped the `HALTED` detail.

use std::time::Duration;

use a2a_protocol_client::retry::RetryPolicy;
use a2a_protocol_server::HaltScope;
use a2a_protocol_types::idempotency;
use a2a_protocol_types::{Message, MessageSendParams, Part};

use super::harness::{Binding, serve};

#[tokio::test]
async fn a_halt_is_final_on_every_binding() {
    let agent = serve(false, false).await;
    agent
        .handler
        .halt(HaltScope::All, "oncall", "stop everything")
        .await;
    for binding in Binding::ALL {
        let client = agent
            .builder(binding, None)
            .await
            .with_peer_honouring_idempotency(true)
            .with_retry_policy(
                RetryPolicy::default()
                    .with_initial_backoff(Duration::from_millis(1))
                    .with_max_retries(3),
            )
            .build()
            .expect("client");
        agent.take_seen();
        let mut message = Message::user(format!("halted-{binding:?}"), vec![Part::text("go")]);
        idempotency::set_key(&mut message, &format!("{binding:?}-halted-key-0123456789")).unwrap();
        let error = client
            .send_message(MessageSendParams::new(message))
            .await
            .expect_err("halted");
        assert!(error.is_halted(), "{binding:?}: {error:?}");
        assert!(!error.is_retryable(), "{binding:?}");
        let sends = agent
            .take_seen()
            .into_iter()
            .filter(|(method, _)| method == "SendMessage")
            .count();
        assert_eq!(sends, 1, "{binding:?}: one attempt");
    }
}
