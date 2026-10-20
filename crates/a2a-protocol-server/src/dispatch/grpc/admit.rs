// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What every gRPC method does before its call runs: read the metadata —
//! refusing an unsupported `A2A-Version` — and convert the request. A
//! request refused here never reaches the handler or the call's span, so it
//! is recorded in `rpc.server.call.duration` here, with the status it was
//! answered with, as the JSON-RPC binding records the requests it refuses
//! before routing (ADR 0013). Until 2026-10-20 these refusals were recorded
//! nowhere.

use std::collections::HashMap;
use std::time::Instant;

use a2a_protocol_types::proto::convert::ConvertError;
use tonic::{Request, Status};

use super::helpers::{code_name, request_headers};
use super::native::A2aServiceImpl;
use crate::rpc_span::{RpcSystem, record_refused};

impl A2aServiceImpl {
    /// The call's headers and its request as `decode` converts it, or the
    /// status it is refused with — recorded as a call of `method`.
    #[allow(clippy::result_large_err)]
    pub(super) fn admit<T, P>(
        &self,
        method: &str,
        request: Request<T>,
        decode: impl FnOnce(T) -> Result<P, Status>,
    ) -> Result<(HashMap<String, String>, P), Status> {
        let started = Instant::now();
        let admitted = request_headers(&request, self.config.require_version_header)
            .and_then(|headers| Ok((headers, decode(request.into_inner())?)));
        if let Err(status) = &admitted {
            record_refused(
                &self.handler,
                RpcSystem::Grpc,
                method,
                started,
                code_name(status.code()),
            );
        }
        admitted
    }
}

/// Converts a request whose conversion can fail: [`bad_request`] if it does.
#[allow(clippy::result_large_err)]
pub(super) fn convert<T, P>(request: T) -> Result<P, Status>
where
    T: TryInto<P, Error = ConvertError>,
{
    request.try_into().map_err(bad_request)
}

/// Maps a request-side conversion failure to `INVALID_ARGUMENT`, with a
/// `google.rpc.BadRequest` naming the field (spec §10.6 / §9.5: well-known
/// detail types for validation errors), as the HTTP bindings now send.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn bad_request(err: ConvertError) -> Status {
    use tonic_types::StatusExt as _;
    let mut details = tonic_types::ErrorDetails::new();
    details.add_bad_request_violation(err.field, err.reason.clone());
    Status::with_error_details(tonic::Code::InvalidArgument, err.to_string(), details)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use a2a_protocol_types::proto as apb;
    use tonic::Request;

    use super::super::native::A2aServiceImpl;
    use super::super::pb::a2a_service_server::A2aService as _;
    use crate::{Metrics, RpcCall};

    /// `(method, status_code, error_type)` of each recorded call.
    type Seen = Arc<Mutex<Vec<(Option<String>, Option<String>, Option<String>)>>>;

    struct Recording(Seen);

    impl Metrics for Recording {
        fn on_rpc_call(&self, call: &RpcCall<'_>) {
            assert_eq!(call.system, "grpc");
            self.0.lock().unwrap().push((
                call.method.map(str::to_owned),
                call.status_code.map(str::to_owned),
                call.error_type.map(str::to_owned),
            ));
        }
    }

    fn service(seen: &Seen) -> A2aServiceImpl {
        struct Idle;
        crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
        A2aServiceImpl {
            handler: Arc::new(
                crate::RequestHandlerBuilder::new(Idle)
                    .without_agent_card()
                    .with_metrics(Recording(Arc::clone(seen)))
                    .build()
                    .unwrap(),
            ),
            config: super::super::GrpcConfig::default(),
        }
    }

    fn get_task(version: Option<&'static str>, id: &str) -> Request<apb::GetTaskRequest> {
        let mut request = Request::new(apb::GetTaskRequest {
            id: id.to_owned(),
            ..Default::default()
        });
        if let Some(version) = version {
            request
                .metadata_mut()
                .insert("a2a-version", version.parse().unwrap());
        }
        request
    }

    /// A call refused for its `A2A-Version` — absent, or one this server
    /// does not speak — is recorded once each, under its method, with the
    /// status it was answered; one the handler ran is recorded by its span,
    /// once, as before.
    #[tokio::test]
    async fn a_version_refusal_is_one_duration_point() {
        let seen = Seen::default();
        let service = service(&seen);
        for version in [None, Some("2.0")] {
            let refused = service
                .get_task(get_task(version, "t"))
                .await
                .expect_err("refused");
            assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        }
        let _ = service
            .get_task(get_task(Some("1.0"), "t"))
            .await
            .expect_err("no such task");
        let get_task = || Some("lf.a2a.v1.A2AService/GetTask".to_owned());
        // `FAILED_PRECONDITION` is no server failure under the gRPC
        // conventions, so it carries no `error.type` (`RpcSystem::is_server_error`).
        let refused = (get_task(), Some("FAILED_PRECONDITION".to_owned()), None);
        assert_eq!(
            *seen.lock().unwrap(),
            [
                refused.clone(),
                refused,
                (get_task(), Some("NOT_FOUND".to_owned()), None)
            ]
        );
    }

    /// A request that does not convert is refused `INVALID_ARGUMENT` before
    /// the handler runs, and recorded as such.
    #[tokio::test]
    async fn a_request_that_does_not_convert_is_one_duration_point() {
        let seen = Seen::default();
        let service = service(&seen);
        let refused = service
            .send_message({
                let mut request = Request::new(apb::SendMessageRequest::default());
                request
                    .metadata_mut()
                    .insert("a2a-version", "1.0".parse().unwrap());
                request
            })
            .await
            .expect_err("no message");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
        assert_eq!(
            *seen.lock().unwrap(),
            [(
                Some("lf.a2a.v1.A2AService/SendMessage".to_owned()),
                Some("INVALID_ARGUMENT".to_owned()),
                None
            )]
        );
    }
}
