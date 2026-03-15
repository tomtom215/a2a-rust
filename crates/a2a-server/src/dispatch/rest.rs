// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! REST dispatcher.
//!
//! [`RestDispatcher`] routes HTTP requests by method and path to the
//! appropriate [`RequestHandler`] method, following the REST transport
//! convention defined in the A2A protocol.

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;

use crate::agent_card::StaticAgentCardHandler;
use crate::error::ServerError;
use crate::executor::AgentExecutor;
use crate::handler::{RequestHandler, SendMessageResult};
use crate::streaming::build_sse_response;

/// REST HTTP request dispatcher.
///
/// Routes requests by HTTP method and path to the underlying [`RequestHandler`].
pub struct RestDispatcher<E: AgentExecutor> {
    handler: Arc<RequestHandler<E>>,
    card_handler: Option<StaticAgentCardHandler>,
}

impl<E: AgentExecutor> RestDispatcher<E> {
    /// Creates a new REST dispatcher.
    #[must_use]
    pub fn new(handler: Arc<RequestHandler<E>>) -> Self {
        let card_handler = handler
            .agent_card
            .as_ref()
            .and_then(|card| StaticAgentCardHandler::new(card).ok());
        Self {
            handler,
            card_handler,
        }
    }

    /// Dispatches an HTTP request and returns a response.
    ///
    /// # Route table
    ///
    /// | Method | Path | Handler |
    /// |---|---|---|
    /// | `POST` | `/messages/send` | `on_send_message(streaming: false)` |
    /// | `POST` | `/messages/stream` | `on_send_message(streaming: true)` |
    /// | `GET` | `/tasks/{id}` | `on_get_task` |
    /// | `POST` | `/tasks/{id}/cancel` | `on_cancel_task` |
    /// | `GET` | `/tasks` | `on_list_tasks` |
    /// | `GET` | `/tasks/{id}/subscribe` | `on_resubscribe` |
    /// | `POST` | `/tasks/{taskId}/push-config` | `on_set_push_config` |
    /// | `GET` | `/tasks/{taskId}/push-config/{id}` | `on_get_push_config` |
    /// | `GET` | `/tasks/{taskId}/push-config` | `on_list_push_configs` |
    /// | `DELETE` | `/tasks/{taskId}/push-config/{id}` | `on_delete_push_config` |
    /// | `GET` | `/agent/authenticatedExtendedCard` | `on_get_authenticated_extended_card` |
    /// | `GET` | `/.well-known/agent-card.json` | static agent card |
    #[allow(clippy::too_many_lines)]
    pub async fn dispatch(
        &self,
        req: hyper::Request<Incoming>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        match (method.as_str(), segments.as_slice()) {
            // Agent card.
            ("GET", [".well-known", "agent-card.json"]) => self
                .card_handler
                .as_ref()
                .map_or_else(not_found_response, |h| {
                    h.handle().map(http_body_util::BodyExt::boxed)
                }),

            // Messages.
            ("POST", ["messages", "send"]) => self.handle_send(req, false).await,
            ("POST", ["messages", "stream"]) => self.handle_send(req, true).await,

            // Tasks.
            ("GET", ["tasks"]) => {
                let query = req.uri().query().unwrap_or("");
                self.handle_list_tasks(query).await
            }
            ("GET", ["tasks", id]) => self.handle_get_task(id).await,
            ("POST", ["tasks", id, "cancel"]) => self.handle_cancel_task(id).await,
            ("GET", ["tasks", id, "subscribe"]) => self.handle_resubscribe(id).await,

            // Push config.
            ("POST", ["tasks", task_id, "push-config"]) => {
                self.handle_set_push_config(req, task_id).await
            }
            ("GET", ["tasks", task_id, "push-config", config_id]) => {
                self.handle_get_push_config(task_id, config_id).await
            }
            ("GET", ["tasks", task_id, "push-config"]) => {
                self.handle_list_push_configs(task_id).await
            }
            ("DELETE", ["tasks", task_id, "push-config", config_id]) => {
                self.handle_delete_push_config(task_id, config_id).await
            }

            // Extended card.
            ("GET", ["agent", "authenticatedExtendedCard"]) => self.handle_extended_card().await,

            _ => not_found_response(),
        }
    }

    // ── Route handlers ───────────────────────────────────────────────────

    async fn handle_send(
        &self,
        req: hyper::Request<Incoming>,
        streaming: bool,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let body_bytes = match req.into_body().collect().await {
            Ok(c) => c.to_bytes(),
            Err(e) => return error_json_response(400, &e.to_string()),
        };
        let params: a2a_types::params::MessageSendParams = match serde_json::from_slice(&body_bytes)
        {
            Ok(p) => p,
            Err(e) => return error_json_response(400, &e.to_string()),
        };
        match self.handler.on_send_message(params, streaming).await {
            Ok(SendMessageResult::Response(resp)) => json_ok_response(&resp),
            Ok(SendMessageResult::Stream(reader)) => build_sse_response(reader, None),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_get_task(&self, id: &str) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_types::params::TaskQueryParams {
            id: a2a_types::task::TaskId::new(id),
            history_length: None,
        };
        match self.handler.on_get_task(params).await {
            Ok(task) => json_ok_response(&task),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_list_tasks(&self, _query: &str) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        // For simplicity, list all tasks (query param parsing can be extended).
        let params = a2a_types::params::ListTasksParams {
            context_id: None,
            status: None,
            page_size: None,
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
        };
        match self.handler.on_list_tasks(params).await {
            Ok(result) => json_ok_response(&result),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_cancel_task(&self, id: &str) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_types::params::TaskIdParams {
            id: a2a_types::task::TaskId::new(id),
        };
        match self.handler.on_cancel_task(params).await {
            Ok(task) => json_ok_response(&task),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_resubscribe(&self, id: &str) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_types::params::TaskIdParams {
            id: a2a_types::task::TaskId::new(id),
        };
        match self.handler.on_resubscribe(params).await {
            Ok(reader) => build_sse_response(reader, None),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_set_push_config(
        &self,
        req: hyper::Request<Incoming>,
        _task_id: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let body_bytes = match req.into_body().collect().await {
            Ok(c) => c.to_bytes(),
            Err(e) => return error_json_response(400, &e.to_string()),
        };
        let config: a2a_types::push::TaskPushNotificationConfig =
            match serde_json::from_slice(&body_bytes) {
                Ok(c) => c,
                Err(e) => return error_json_response(400, &e.to_string()),
            };
        match self.handler.on_set_push_config(config).await {
            Ok(result) => json_ok_response(&result),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_get_push_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_types::params::GetPushConfigParams {
            task_id: a2a_types::task::TaskId::new(task_id),
            id: config_id.to_owned(),
        };
        match self.handler.on_get_push_config(params).await {
            Ok(config) => json_ok_response(&config),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_list_push_configs(
        &self,
        task_id: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        match self
            .handler
            .on_list_push_configs(a2a_types::task::TaskId::new(task_id))
            .await
        {
            Ok(configs) => json_ok_response(&configs),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_delete_push_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_types::params::DeletePushConfigParams {
            task_id: a2a_types::task::TaskId::new(task_id),
            id: config_id.to_owned(),
        };
        match self.handler.on_delete_push_config(params).await {
            Ok(()) => json_ok_response(&serde_json::json!({})),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_extended_card(&self) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        match self.handler.on_get_authenticated_extended_card().await {
            Ok(card) => json_ok_response(&card),
            Err(e) => server_error_to_response(&e),
        }
    }
}

impl<E: AgentExecutor> std::fmt::Debug for RestDispatcher<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestDispatcher").finish()
    }
}

// ── Response helpers ─────────────────────────────────────────────────────────

fn json_ok_response<T: serde::Serialize>(value: &T) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    hyper::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)).boxed())
        .expect("response builder should not fail with valid headers")
}

fn error_json_response(status: u16, message: &str) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let body = serde_json::json!({ "error": message });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    hyper::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(bytes)).boxed())
        .expect("response builder should not fail with valid headers")
}

fn not_found_response() -> hyper::Response<BoxBody<Bytes, Infallible>> {
    error_json_response(404, "not found")
}

fn server_error_to_response(err: &ServerError) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let status = match err {
        ServerError::TaskNotFound(_) | ServerError::MethodNotFound(_) => 404,
        ServerError::TaskNotCancelable(_) => 409,
        ServerError::InvalidParams(_) | ServerError::Serialization(_) => 400,
        ServerError::PushNotSupported => 501,
        _ => 500,
    };
    let a2a_err = err.to_a2a_error();
    let body = serde_json::to_vec(&a2a_err).unwrap_or_default();
    hyper::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)).boxed())
        .expect("response builder should not fail with valid headers")
}
