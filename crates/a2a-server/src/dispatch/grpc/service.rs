// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `A2aService` trait implementation for the gRPC dispatcher.

use std::sync::Arc;

use tonic::{Request, Response, Status};

use super::helpers::{
    decode_json, encode_json, extract_metadata, reader_to_grpc_stream, server_error_to_status,
    GrpcStream,
};
use super::{A2aService, GrpcConfig, JsonPayload};
use crate::handler::{RequestHandler, SendMessageResult};

/// The tonic service implementation that bridges gRPC to the handler.
///
/// This type implements the generated `A2aService` trait and is not
/// typically used directly — use [`super::GrpcDispatcher`] instead.
pub struct GrpcServiceImpl {
    pub(super) handler: Arc<RequestHandler>,
    pub(super) config: GrpcConfig,
}

#[tonic::async_trait]
impl A2aService for GrpcServiceImpl {
    // ── Messaging ────────────────────────────────────────────────────────

    async fn send_message(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self
            .handler
            .on_send_message(params, false, Some(&headers))
            .await
        {
            Ok(SendMessageResult::Response(resp)) => Ok(Response::new(encode_json(&resp)?)),
            Ok(SendMessageResult::Stream(_)) => Err(Status::internal(
                "unexpected stream response for unary call",
            )),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    type SendStreamingMessageStream = GrpcStream;

    async fn send_streaming_message(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<Self::SendStreamingMessageStream>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self
            .handler
            .on_send_message(params, true, Some(&headers))
            .await
        {
            Ok(SendMessageResult::Stream(reader)) => {
                let stream = reader_to_grpc_stream(reader, self.config.stream_channel_capacity);
                Ok(Response::new(stream))
            }
            Ok(SendMessageResult::Response(resp)) => {
                // Wrap single response as a one-element stream.
                let payload = encode_json(&resp)?;
                let stream = Box::pin(tokio_stream::once(Ok(payload)));
                Ok(Response::new(stream as GrpcStream))
            }
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    // ── Task lifecycle ───────────────────────────────────────────────────

    async fn get_task(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self.handler.on_get_task(params, Some(&headers)).await {
            Ok(task) => Ok(Response::new(encode_json(&task)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    async fn list_tasks(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self.handler.on_list_tasks(params, Some(&headers)).await {
            Ok(resp) => Ok(Response::new(encode_json(&resp)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    async fn cancel_task(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self.handler.on_cancel_task(params, Some(&headers)).await {
            Ok(task) => Ok(Response::new(encode_json(&task)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    type SubscribeToTaskStream = GrpcStream;

    async fn subscribe_to_task(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<Self::SubscribeToTaskStream>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self.handler.on_resubscribe(params, Some(&headers)).await {
            Ok(reader) => {
                let stream = reader_to_grpc_stream(reader, self.config.stream_channel_capacity);
                Ok(Response::new(stream))
            }
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    // ── Push notification config ─────────────────────────────────────────

    async fn create_task_push_notification_config(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let config = decode_json(request.get_ref())?;
        match self
            .handler
            .on_set_push_config(config, Some(&headers))
            .await
        {
            Ok(cfg) => Ok(Response::new(encode_json(&cfg)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    async fn get_task_push_notification_config(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self
            .handler
            .on_get_push_config(params, Some(&headers))
            .await
        {
            Ok(cfg) => Ok(Response::new(encode_json(&cfg)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    async fn list_task_push_notification_configs(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params: a2a_protocol_types::params::ListPushConfigsParams =
            decode_json(request.get_ref())?;
        match self
            .handler
            .on_list_push_configs(&params.task_id, params.tenant.as_deref(), Some(&headers))
            .await
        {
            Ok(configs) => {
                let resp = a2a_protocol_types::responses::ListPushConfigsResponse {
                    configs,
                    next_page_token: None,
                };
                Ok(Response::new(encode_json(&resp)?))
            }
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    async fn delete_task_push_notification_config(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        let params = decode_json(request.get_ref())?;
        match self
            .handler
            .on_delete_push_config(params, Some(&headers))
            .await
        {
            Ok(()) => Ok(Response::new(encode_json(&serde_json::json!({}))?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }

    // ── Agent card ───────────────────────────────────────────────────────

    async fn get_extended_agent_card(
        &self,
        request: Request<JsonPayload>,
    ) -> Result<Response<JsonPayload>, Status> {
        let headers = extract_metadata(request.metadata());
        match self
            .handler
            .on_get_extended_agent_card(Some(&headers))
            .await
        {
            Ok(card) => Ok(Response::new(encode_json(&card)?)),
            Err(e) => Err(server_error_to_status(&e)),
        }
    }
}
