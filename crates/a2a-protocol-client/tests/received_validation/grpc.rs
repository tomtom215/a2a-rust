// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The gRPC binding: a field absent on the wire is refused by the
//! conversion, an empty required list by the same check the JSON bindings
//! run, both as `InvalidAgentResponse`.

use a2a_protocol_client::{A2aClient, ClientBuilder};
use a2a_protocol_types::proto as pb;
use a2a_protocol_types::{AgentCard, Message, MessageRole, MessageSendParams, Part, Task};
use prost::Message as _;

use super::peers;
use super::{assert_refused, task};

async fn client(url: &str) -> A2aClient {
    ClientBuilder::new(url)
        .with_protocol_binding("GRPC")
        .build_grpc()
        .await
        .expect("client")
}

fn pb_task() -> pb::Task {
    let task: Task = serde_json::from_value(task()).expect("task");
    task.try_into().expect("converts")
}

fn query() -> a2a_protocol_types::TaskQueryParams {
    a2a_protocol_types::TaskQueryParams {
        tenant: None,
        id: "t-1".into(),
        history_length: None,
    }
}

#[tokio::test]
async fn a_task_missing_a_required_value_is_refused_over_grpc() {
    let ok = peers::grpc(pb_task().encode_to_vec(), Vec::new()).await;
    client(&ok.url)
        .await
        .get_task(query())
        .await
        .expect("the valid task");
    type Blank = fn(&mut pb::Task);
    let cases: [(&str, Blank); 4] = [
        // Absent on the wire: refused by the conversion.
        ("task.id: missing required field", |t| t.id.clear()),
        ("status.state: missing required field", |t| {
            t.status.as_mut().unwrap().state = 0;
        }),
        ("artifact.artifactId: missing required field", |t| {
            t.artifacts[0].artifact_id.clear();
        }),
        // An empty repeated field converts, as JSON parses its absence; the
        // check every binding runs refuses it.
        ("artifact must contain at least one part", |t| {
            t.artifacts[0].parts.clear()
        }),
    ];
    for (needle, blank) in cases {
        let mut t = pb_task();
        blank(&mut t);
        let bad = peers::grpc(t.encode_to_vec(), Vec::new()).await;
        assert_refused(
            client(&bad.url).await.get_task(query()).await,
            "GetTask",
            needle,
        );
    }
}

#[tokio::test]
async fn a_streamed_event_missing_a_required_value_ends_the_grpc_stream() {
    let event = |context_id: &str| pb::StreamResponse {
        payload: Some(pb::stream_response::Payload::StatusUpdate(
            pb::TaskStatusUpdateEvent {
                task_id: "t".into(),
                context_id: context_id.into(),
                status: Some(pb::TaskStatus {
                    state: pb::TaskState::Working as i32,
                    message: None,
                    timestamp: None,
                }),
                metadata: None,
            },
        )),
    };
    let p = peers::grpc(
        Vec::new(),
        vec![event("c").encode_to_vec(), event("").encode_to_vec()],
    )
    .await;
    let params =
        MessageSendParams::new(Message::new("m", MessageRole::User, vec![Part::text("hi")]));
    let mut stream = client(&p.url)
        .await
        .stream_message(params)
        .await
        .expect("stream");
    stream
        .next()
        .await
        .expect("first")
        .expect("the valid event is delivered");
    assert_refused(
        stream.next().await.expect("second"),
        "streamed event",
        "statusUpdate.contextId: missing required field",
    );
}

/// idx 437: an absent `capabilities` was read as the empty set over gRPC,
/// where the same card over JSON does not parse.
#[tokio::test]
async fn an_extended_card_without_capabilities_is_refused_over_grpc() {
    let card: AgentCard =
        serde_json::from_value(super::cards::card_json("http://127.0.0.1:1")).expect("card");
    let mut proto = pb::AgentCard::try_from(card).expect("converts");
    let ok = peers::grpc(proto.encode_to_vec(), Vec::new()).await;
    client(&ok.url)
        .await
        .get_extended_agent_card()
        .await
        .expect("the valid card");
    proto.capabilities = None;
    let bad = peers::grpc(proto.encode_to_vec(), Vec::new()).await;
    assert_refused(
        client(&bad.url).await.get_extended_agent_card().await,
        "GetExtendedAgentCard",
        "agentCard.capabilities: missing required field",
    );
}
