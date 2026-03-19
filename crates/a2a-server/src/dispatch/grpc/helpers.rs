// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Helper functions shared across the gRPC dispatcher submodules.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;

use super::JsonPayload;
use crate::error::ServerError;
use crate::streaming::EventQueueReader;

/// The streaming response type for gRPC server-streaming methods.
pub(super) type GrpcStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<JsonPayload, Status>> + Send + 'static>>;

/// Extracts gRPC metadata into a `HashMap` matching the HTTP headers
/// interface used by `RequestHandler`.
pub(super) fn extract_metadata(metadata: &tonic::metadata::MetadataMap) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for kv in metadata.iter() {
        if let tonic::metadata::KeyAndValueRef::Ascii(key, value) = kv {
            if let Ok(v) = value.to_str() {
                map.insert(key.as_str().to_owned(), v.to_owned());
            }
        }
    }
    map
}

/// Deserializes a JSON payload from a gRPC request.
#[allow(clippy::result_large_err)]
pub(super) fn decode_json<T: serde::de::DeserializeOwned>(
    payload: &JsonPayload,
) -> Result<T, Status> {
    serde_json::from_slice(&payload.data)
        .map_err(|e| Status::invalid_argument(format!("invalid JSON payload: {e}")))
}

/// Serializes a value into a JSON payload for a gRPC response.
#[allow(clippy::result_large_err)]
pub(super) fn encode_json<T: serde::Serialize>(value: &T) -> Result<JsonPayload, Status> {
    let data = serde_json::to_vec(value)
        .map_err(|e| Status::internal(format!("JSON serialization failed: {e}")))?;
    Ok(JsonPayload { data })
}

/// Converts a [`ServerError`] into a tonic [`Status`].
pub(super) fn server_error_to_status(err: &ServerError) -> Status {
    let a2a_err = err.to_a2a_error();
    let code = match a2a_err.code {
        a2a_protocol_types::ErrorCode::TaskNotFound => tonic::Code::NotFound,
        a2a_protocol_types::ErrorCode::TaskNotCancelable => tonic::Code::FailedPrecondition,
        a2a_protocol_types::ErrorCode::InvalidParams
        | a2a_protocol_types::ErrorCode::ParseError => tonic::Code::InvalidArgument,
        a2a_protocol_types::ErrorCode::MethodNotFound
        | a2a_protocol_types::ErrorCode::PushNotificationNotSupported => tonic::Code::Unimplemented,
        _ => tonic::Code::Internal,
    };
    Status::new(code, a2a_err.message)
}

/// Resolves a `ToSocketAddrs` to a single `SocketAddr`.
pub(super) async fn resolve_addr(
    addr: impl tokio::net::ToSocketAddrs,
) -> std::io::Result<SocketAddr> {
    tokio::net::lookup_host(addr).await?.next().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "could not resolve address",
        )
    })
}

/// Converts an [`InMemoryQueueReader`] into a gRPC streaming response.
pub(super) fn reader_to_grpc_stream(
    mut reader: crate::streaming::InMemoryQueueReader,
    capacity: usize,
) -> GrpcStream {
    let (tx, rx) = mpsc::channel(capacity);
    tokio::spawn(async move {
        loop {
            match reader.read().await {
                Some(Ok(event)) => {
                    let payload = match encode_json(&event) {
                        Ok(p) => p,
                        Err(status) => {
                            let _ = tx.send(Err(status)).await;
                            break;
                        }
                    };
                    if tx.send(Ok(payload)).await.is_err() {
                        break;
                    }
                }
                Some(Err(_)) => {
                    let _ = tx.send(Err(Status::internal("event queue error"))).await;
                    break;
                }
                None => break,
            }
        }
    });
    Box::pin(ReceiverStream::new(rx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ServerError;

    // decode_json / encode_json round-trip
    #[test]
    fn encode_decode_json_roundtrip() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct Foo {
            x: u32,
        }
        let original = Foo { x: 42 };
        let payload = encode_json(&original).unwrap();
        let decoded: Foo = decode_json(&payload).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn decode_json_invalid_returns_status_error() {
        let payload = JsonPayload {
            data: b"not-json".to_vec(),
        };
        let result: Result<serde_json::Value, _> = decode_json(&payload);
        assert!(result.is_err());
    }

    // server_error_to_status mapping
    #[test]
    fn task_not_found_maps_to_not_found() {
        let status = server_error_to_status(&ServerError::TaskNotFound("t1".into()));
        assert_eq!(status.code(), tonic::Code::NotFound);
    }

    #[test]
    fn task_not_cancelable_maps_to_failed_precondition() {
        let status = server_error_to_status(&ServerError::TaskNotCancelable("t1".into()));
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
    }

    #[test]
    fn invalid_params_maps_to_invalid_argument() {
        let status = server_error_to_status(&ServerError::InvalidParams("bad".into()));
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn method_not_found_maps_to_unimplemented() {
        let status = server_error_to_status(&ServerError::MethodNotFound("Foo".into()));
        assert_eq!(status.code(), tonic::Code::Unimplemented);
    }

    #[test]
    fn internal_error_maps_to_internal() {
        let status = server_error_to_status(&ServerError::Internal("oops".into()));
        assert_eq!(status.code(), tonic::Code::Internal);
    }

    // extract_metadata
    #[test]
    fn extract_metadata_ascii_keys() {
        let mut meta = tonic::metadata::MetadataMap::new();
        meta.insert("authorization", "Bearer token".parse().unwrap());
        let map = extract_metadata(&meta);
        assert_eq!(
            map.get("authorization").map(String::as_str),
            Some("Bearer token")
        );
    }

    #[test]
    fn extract_metadata_empty() {
        let meta = tonic::metadata::MetadataMap::new();
        let map = extract_metadata(&meta);
        assert!(map.is_empty());
    }
}
