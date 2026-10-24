// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A transport given to `with_custom_transport`: the system, address and
//! agent its call spans carry.

use super::*;

/// A transport that answers every call with one task, naming its system as
/// told.
#[cfg(feature = "tracing")]
struct Named(Option<&'static str>, Option<&'static str>);
#[cfg(feature = "tracing")]
impl crate::transport::Transport for Named {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _headers: &'a std::collections::HashMap<String, String>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = crate::error::ClientResult<serde_json::Value>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Ok(serde_json::json!({"id": "t1", "contextId": "c1",
                "status": {"state": "TASK_STATE_COMPLETED"}}))
        })
    }
    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _headers: &'a std::collections::HashMap<String, String>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = crate::error::ClientResult<crate::streaming::EventStream>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Err(crate::error::ClientError::Transport("unused".into())) })
    }
    fn rpc_system(&self) -> Option<&'static str> {
        self.0
    }
    fn server_url(&self) -> Option<&str> {
        self.1
    }
}

/// A transport given to `with_custom_transport` names its own
/// `rpc.system.name`, with a retry policy too (the builder reads the name
/// before wrapping the transport in retries), and the span keeps the agent
/// from the card, and the address and port of the URL it names; one that
/// names neither records neither.
#[cfg(feature = "tracing")]
#[test]
fn a_custom_transport_names_its_system_and_keeps_the_agent() {
    use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};

    let card = AgentCard::new(
        "worker",
        "1",
        AgentInterface::jsonrpc("http://agent.example"),
    )
    .with_description("d")
    .with_skill(AgentSkill::new("s", "S", "d").with_tags(["t"]));
    let call = |system: Option<&'static str>, url: Option<&'static str>, retry: bool| {
        let mut builder = crate::ClientBuilder::from_card(&card)
            .unwrap()
            .with_custom_transport(Named(system, url));
        if retry {
            builder = builder.with_retry_policy(crate::RetryPolicy::default());
        }
        let client = builder.build().unwrap();
        recorded(|| {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
                .block_on(client.get_task(a2a_protocol_types::TaskQueryParams {
                    tenant: None,
                    id: "t1".into(),
                    history_length: None,
                }))
                .unwrap();
        })
    };
    for retry in [false, true] {
        let fields = call(Some("jsonrpc"), Some("wss://[::1]/a2a"), retry);
        let field = |k: &str| fields.get(k).map(String::as_str);
        assert_eq!(field("rpc.system.name"), Some("jsonrpc"), "retry: {retry}");
        assert_eq!(field("gen_ai.agent.name"), Some("worker"), "retry: {retry}");
        assert_eq!(field("server.address"), Some("::1"), "retry: {retry}");
        assert_eq!(field("server.port"), Some("443"), "retry: {retry}");
    }
    let fields = call(Some("ws"), Some("ws://agent.example:8081/"), false);
    assert_eq!(fields.get("server.port").map(String::as_str), Some("8081"));
    let fields = call(Some("ws"), Some("ws://agent.example/"), false);
    assert_eq!(fields.get("server.port").map(String::as_str), Some("80"));
    let fields = call(None, None, false);
    assert_eq!(fields.get("rpc.system.name"), None);
    assert_eq!(fields.get("server.address"), None);
    assert_eq!(fields.get("server.port"), None);
}

/// `build_grpc` reads a custom transport's system and URL as `build` does.
#[cfg(all(feature = "tracing", feature = "grpc"))]
#[test]
fn a_custom_transport_given_to_build_grpc_names_its_system_and_address() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let client = runtime
        .block_on(
            crate::ClientBuilder::new("http://grpc.example:50051")
                .with_custom_transport(Named(Some("jsonrpc"), Some("ws://agent.example:8081/")))
                .build_grpc(),
        )
        .unwrap();
    let fields = recorded(|| {
        runtime
            .block_on(client.get_task(a2a_protocol_types::TaskQueryParams {
                tenant: None,
                id: "t1".into(),
                history_length: None,
            }))
            .unwrap();
    });
    let field = |k: &str| fields.get(k).map(String::as_str);
    assert_eq!(field("rpc.system.name"), Some("jsonrpc"));
    assert_eq!(field("server.address"), Some("agent.example"));
    assert_eq!(field("server.port"), Some("8081"));
}
