// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Reading a request's params on the HTTP+JSON binding, which happens
//! before the handler — and so before the interceptor chain — sees the call.
//!
//! Until 2026-10-20 a request whose body was not JSON, whose body did not
//! fit the method's params, or whose query held a value that did not parse,
//! was answered `400` there and then, so a caller with no credential learned
//! which field it got wrong where a well-formed request would have been
//! refused `401` (`docs/handoff.md`, "Open engineering"; JSON-RPC was fixed
//! the same way that day, `handler/unreadable.rs`). Such a request is now
//! admitted first, and answered its `400` only when the chain admits it.
//!
//! What still comes first is what decides how, or whether, to read the
//! request at all: the body's size cap (`413`) and read timeout (`408`)
//! (spec §13.4 asks for limits on message sizes, and the bytes are never
//! parsed), the media type and `A2A-Version` (spec §3.6.2: the version names
//! the semantics the request is read with), and routing. Each refuses a
//! well-formed request alike, so none says anything about a request's
//! fields.

use std::collections::HashMap;
use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;

use super::RestDispatcher;
use super::error_response::{
    bad_request_response, error_json_response, params_from_value, server_error_to_response,
};
use super::response::{read_body_limited, set_path_field};
use crate::error::ServerError;
use crate::handler::RequestHandler;

type HttpResponse = hyper::Response<BoxBody<Bytes, Infallible>>;

/// A request whose params could not be read: the response a caller the
/// interceptor chain admits is sent, and the error the chain's
/// `on_complete` is told the call failed with.
pub struct Unreadable {
    response: HttpResponse,
    error: ServerError,
}

impl Unreadable {
    /// A body that is not JSON: a plain `400`, as it always was.
    pub fn not_json(err: &serde_json::Error) -> Self {
        let message = err.to_string();
        Self {
            response: error_json_response(400, &message),
            error: ServerError::InvalidParams(message),
        }
    }

    /// JSON of the wrong shape: a `400` whose `BadRequest` names `field`.
    pub fn violation(field: &str, description: &str) -> Self {
        Self {
            response: bad_request_response(field, description),
            error: ServerError::InvalidField {
                field: field.to_owned(),
                message: description.to_owned(),
            },
        }
    }

    /// The answer to the call: the chain's refusal, when it does not admit
    /// the caller, and the `400` otherwise.
    pub async fn answer(
        self,
        handler: &RequestHandler,
        method: &str,
        headers: &HashMap<String, String>,
    ) -> HttpResponse {
        match handler
            .admit_unreadable(method, Some(headers), self.error)
            .await
        {
            Ok(_) => self.response,
            Err(refused) => server_error_to_response(&refused),
        }
    }
}

/// A body that is not JSON: [`Unreadable::not_json`].
impl From<serde_json::Error> for Unreadable {
    fn from(err: serde_json::Error) -> Self {
        Self::not_json(&err)
    }
}

/// A query value that did not parse, answered as it always was.
impl From<ServerError> for Unreadable {
    fn from(error: ServerError) -> Self {
        Self {
            response: server_error_to_response(&error),
            error,
        }
    }
}

impl std::fmt::Debug for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Unreadable")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl RestDispatcher {
    /// Reads a `method` call's params from its bounded POST body, filling in
    /// fields the client put in the URL instead: the path tenant (when the
    /// prefix form was used) and any other path variable the route bound. An
    /// empty body is the empty object, so a bodiless
    /// `POST /tasks/{id}:cancel` still parses.
    ///
    /// `Err` carries the response to return: `413` for an oversized body,
    /// `408` for one not received in time, and, for one that is not JSON or
    /// does not fit the params, the `400` when the interceptor chain admits
    /// the caller and its refusal when it does not.
    // The `Err` is the finished response, sent as-is by the one caller of
    // each route; boxing it would only add an allocation on the error path.
    #[allow(clippy::result_large_err)]
    pub(super) async fn read_params<T: serde::de::DeserializeOwned>(
        &self,
        req: hyper::Request<Incoming>,
        method: &str,
        tenant: Option<&str>,
        path_fields: &[(&str, &str)],
        headers: &HashMap<String, String>,
    ) -> Result<T, HttpResponse> {
        let body_bytes = read_body_limited(
            req.into_body(),
            self.config.max_request_body_size,
            self.config.body_read_timeout,
        )
        .await
        .map_err(|(status, msg)| error_json_response(status, &msg))?;
        let parsed = body_params(&body_bytes, tenant, path_fields);
        match parsed {
            Ok(params) => Ok(params),
            Err(unreadable) => Err(unreadable.answer(&self.handler, method, headers).await),
        }
    }

    /// The answer to a `method` call whose query did not parse: `err`, when
    /// the interceptor chain admits the caller, and its refusal otherwise.
    pub(super) async fn refuse_unreadable(
        &self,
        method: &str,
        headers: &HashMap<String, String>,
        err: ServerError,
    ) -> HttpResponse {
        Unreadable::from(err)
            .answer(&self.handler, method, headers)
            .await
    }
}

/// The params in a body read in full, with the path's fields set over it.
#[allow(clippy::result_large_err)] // the error holds the response, answered once
fn body_params<T: serde::de::DeserializeOwned>(
    body: &[u8],
    tenant: Option<&str>,
    path_fields: &[(&str, &str)],
) -> Result<T, Unreadable> {
    let mut value: serde_json::Value = if body.iter().all(u8::is_ascii_whitespace) {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(body).map_err(|e| Unreadable::not_json(&e))?
    };
    for &(field, path_value) in path_fields {
        value = set_path_field(value, field, path_value);
    }
    // The path form is authoritative: under `google.api.http` a variable
    // bound from the path is the field's value, whatever the body says.
    // (A body-only tenant was already applied by the caller, so `tenant`
    // here is either the path's or the body's own.)
    if let Some(tenant) = tenant
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert(
            "tenant".to_owned(),
            serde_json::Value::String(tenant.to_owned()),
        );
    }
    params_from_value(value)
}
