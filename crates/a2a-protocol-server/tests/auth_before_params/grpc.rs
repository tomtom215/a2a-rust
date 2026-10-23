// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! gRPC: a request that does not convert to the method's params, and the
//! requests that convert but that the handler refuses.

use std::sync::atomic::Ordering;

use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};
use a2a_protocol_types::proto as pb;
use tonic::codegen::http::uri::PathAndQuery;

use super::{Misses, TOKENS, gated};

type Channel = tonic::transport::Channel;

fn request<T>(message: T, token: Option<&str>) -> tonic::Request<T> {
    let mut req = tonic::Request::new(message);
    let metadata = req.metadata_mut();
    metadata.insert("a2a-version", "1.0".parse().expect("metadata"));
    if let Some(token) = token {
        let value = format!("Bearer {token}").parse().expect("metadata");
        metadata.insert("authorization", value);
    }
    req
}

/// The status a unary `method` call is answered with.
async fn unary<Req, Resp>(
    channel: &Channel,
    method: &str,
    message: Req,
    token: Option<&str>,
) -> tonic::Status
where
    Req: prost::Message + Send + Sync + 'static,
    Resp: prost::Message + Default + std::fmt::Debug + Send + Sync + 'static,
{
    let mut grpc = tonic::client::Grpc::new(channel.clone());
    grpc.ready().await.expect("ready");
    let path = PathAndQuery::try_from(format!("/lf.a2a.v1.A2AService/{method}")).expect("path");
    let codec = tonic_prost::ProstCodec::<Req, Resp>::default();
    grpc.unary(request(message, token), path, codec)
        .await
        .expect_err(method)
}

/// The status a `SendStreamingMessage` call is answered with.
async fn streaming(
    channel: &Channel,
    message: pb::SendMessageRequest,
    token: Option<&str>,
) -> tonic::Status {
    let mut grpc = tonic::client::Grpc::new(channel.clone());
    grpc.ready().await.expect("ready");
    let path = PathAndQuery::from_static("/lf.a2a.v1.A2AService/SendStreamingMessage");
    let codec = tonic_prost::ProstCodec::<pb::SendMessageRequest, pb::StreamResponse>::default();
    grpc.server_streaming(request(message, token), path, codec)
        .await
        .expect_err("SendStreamingMessage")
}

/// Every call below with `token`: the method, and the status it got.
async fn calls(channel: &Channel, token: Option<&str>) -> Vec<(&'static str, tonic::Status)> {
    let list = |page_size| pb::ListTasksRequest {
        page_size: Some(page_size),
        ..Default::default()
    };
    vec![
        // Requests that do not convert.
        (
            "SendMessage",
            unary::<_, pb::SendMessageResponse>(
                channel,
                "SendMessage",
                pb::SendMessageRequest::default(),
                token,
            )
            .await,
        ),
        (
            "SendStreamingMessage",
            streaming(channel, pb::SendMessageRequest::default(), token).await,
        ),
        (
            "GetTask",
            unary::<_, pb::Task>(
                channel,
                "GetTask",
                pb::GetTaskRequest {
                    id: "t1".into(),
                    history_length: Some(-1),
                    ..Default::default()
                },
                token,
            )
            .await,
        ),
        (
            "ListTasks",
            unary::<_, pb::ListTasksResponse>(channel, "ListTasks", list(-1), token).await,
        ),
        (
            "ListTaskPushNotificationConfigs",
            unary::<_, pb::ListTaskPushNotificationConfigsResponse>(
                channel,
                "ListTaskPushNotificationConfigs",
                pb::ListTaskPushNotificationConfigsRequest {
                    task_id: "t1".into(),
                    page_size: -1,
                    ..Default::default()
                },
                token,
            )
            .await,
        ),
        // Requests that convert, and that the handler refuses.
        (
            "ListTasks pageSize 0",
            unary::<_, pb::ListTasksResponse>(channel, "ListTasks", list(0), token).await,
        ),
        (
            "ListTasks pageSize 150",
            unary::<_, pb::ListTasksResponse>(channel, "ListTasks", list(150), token).await,
        ),
        (
            "CreateTaskPushNotificationConfig without taskId",
            unary::<_, pb::TaskPushNotificationConfig>(
                channel,
                "CreateTaskPushNotificationConfig",
                pb::TaskPushNotificationConfig {
                    url: "https://hook.example/a".into(),
                    ..Default::default()
                },
                token,
            )
            .await,
        ),
    ]
}

#[tokio::test]
async fn grpc_malformed_request_is_refused_unauthenticated_without_credential() {
    let (handler, checked) = gated();
    let addr = GrpcDispatcher::new(handler, GrpcConfig::default())
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("serve");
    let channel = Channel::from_shared(format!("http://{addr}"))
        .expect("uri")
        .connect()
        .await
        .expect("connect");
    let mut sent = 0;
    let mut misses = Misses::default();
    for token in TOKENS {
        for (what, status) in calls(&channel, token).await {
            sent += 1;
            let want = if token == Some("good") {
                tonic::Code::InvalidArgument
            } else {
                tonic::Code::Unauthenticated
            };
            misses.check(status.code() == want, || {
                format!(
                    "{what} with {token:?}: {:?} {}",
                    status.code(),
                    status.message()
                )
            });
        }
    }
    misses.assert_none();
    assert_eq!(checked.load(Ordering::SeqCst), sent);
}
