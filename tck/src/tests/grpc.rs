// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Conformance checks for the §10 gRPC binding.
//!
//! These are separate function bodies rather than a JSON adapter over the
//! other bindings' checks, and that is deliberate. The obvious shortcut —
//! convert each protobuf response to `ProtoJSON` and reuse the existing
//! assertions — produces checks that cannot fail. `task_state_values`, for
//! instance, asserts the state is one of the nine `TASK_STATE_*` strings; over
//! gRPC the state is an `i32` on the wire, and the string only exists after
//! *this crate's* enum-to-name mapping runs. The assertion would then be about
//! the TCK's own converter, not the server's wire format, and would pass no
//! matter what the server sent.
//!
//! So each check asserts what is actually observable at this binding: that an
//! enum decodes to a value the schema defines, that a fault arrives as the
//! `tonic::Code` §5.4 maps it to, that a repeated field is populated. Check
//! *names* are shared with the other bindings so a run can be compared across
//! them; the assertions are the binding's own.

use tonic::transport::Channel;
use tonic::{Code, Request};

/// The TCK's own generated client and message types.
///
/// Compiled from `tck/proto` by this crate's `build.rs`, with no `extern_path`
/// back to `a2a-protocol-types`. See that file for why a conformance kit must
/// not share the implementation's schema mapping.
#[allow(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    missing_docs,
    unused_qualifications
)]
pub mod pb {
    tonic::include_proto!("lf.a2a.v1");
}

use pb::a2a_service_client::A2aServiceClient;

type Client = A2aServiceClient<Channel>;

/// Connects to the gRPC endpoint named by the agent card.
///
/// A gRPC target is a name-resolver string, but `tonic`'s `Endpoint` wants a
/// URI, so a scheme is added when the card omits one (`127.0.0.1:9998` is the
/// correct advertisement, per the note in `tck/sut/src/main.rs`).
pub async fn connect(target: &str) -> Result<Client, String> {
    let uri = if target.contains("://") {
        target.to_owned()
    } else {
        format!("http://{target}")
    };
    A2aServiceClient::connect(uri.clone())
        .await
        .map_err(|e| format!("gRPC connect to {uri} failed: {e}"))
}

/// A `SendMessageRequest` carrying one text part, matching what the other
/// bindings send in `helpers::make_send_params`.
fn send_request(text: &str, context_id: Option<&str>) -> pb::SendMessageRequest {
    pb::SendMessageRequest {
        tenant: String::new(),
        message: Some(pb::Message {
            message_id: uuid::Uuid::new_v4().to_string(),
            context_id: context_id.unwrap_or_default().to_owned(),
            task_id: String::new(),
            role: pb::Role::User as i32,
            parts: vec![pb::Part {
                content: Some(pb::part::Content::Text(text.to_owned())),
                ..Default::default()
            }],
            metadata: None,
            extensions: Vec::new(),
            reference_task_ids: Vec::new(),
        }),
        configuration: None,
        metadata: None,
    }
}

/// Unwraps the `task` arm of a `SendMessageResponse`.
fn expect_task(resp: pb::SendMessageResponse) -> Result<pb::Task, String> {
    match resp.payload {
        Some(pb::send_message_response::Payload::Task(task)) => Ok(task),
        Some(pb::send_message_response::Payload::Message(_)) => {
            Err("SendMessage returned a Message where this check needs a Task".to_string())
        }
        None => Err("SendMessageResponse has no payload — neither task nor message".to_string()),
    }
}

/// The state as the schema defines it, or an error naming the raw value.
///
/// `prost` stores an enum field as `i32` and does not reject unknown values,
/// so a server that puts an out-of-range number on the wire produces a struct
/// that looks fine until something tries to name the value. That is the real
/// wire-format check available at this binding, and it is what
/// `task_state_values` asserts.
fn decode_state(raw: i32) -> Result<pb::TaskState, String> {
    pb::TaskState::try_from(raw)
        .map_err(|_| format!("status.state is {raw}, which lf.a2a.v1.TaskState does not define"))
}

// ── Checks ───────────────────────────────────────────────────────────────────

pub async fn send_message_basic(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let resp = client
        .send_message(Request::new(send_request(
            "TCK: grpc send_message_basic",
            None,
        )))
        .await
        .map_err(|s| format!("SendMessage failed: {}: {}", s.code(), s.message()))?;
    if resp.into_inner().payload.is_none() {
        return Err("SendMessageResponse has no payload".to_string());
    }
    Ok(())
}

pub async fn send_message_returns_task(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let resp = client
        .send_message(Request::new(send_request("TCK: grpc returns_task", None)))
        .await
        .map_err(|s| format!("SendMessage failed: {}: {}", s.code(), s.message()))?;
    let task = expect_task(resp.into_inner())?;

    if task.id.is_empty() {
        return Err("task.id is empty".to_string());
    }
    let status = task
        .status
        .ok_or_else(|| "task has no status".to_string())?;
    let state = decode_state(status.state)?;
    if state == pb::TaskState::Unspecified {
        return Err("task.status.state is TASK_STATE_UNSPECIFIED after SendMessage".to_string());
    }
    Ok(())
}

pub async fn send_message_context_id(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let first = expect_task(
        client
            .send_message(Request::new(send_request("TCK: grpc first turn", None)))
            .await
            .map_err(|s| format!("SendMessage failed: {}: {}", s.code(), s.message()))?
            .into_inner(),
    )?;
    if first.context_id.is_empty() {
        return Err("first task has an empty contextId".to_string());
    }

    let second = expect_task(
        client
            .send_message(Request::new(send_request(
                "TCK: grpc second turn",
                Some(&first.context_id),
            )))
            .await
            .map_err(|s| format!("SendMessage failed: {}: {}", s.code(), s.message()))?
            .into_inner(),
    )?;
    if second.context_id != first.context_id {
        return Err(format!(
            "contextId changed between turns: '{}' vs '{}'",
            first.context_id, second.context_id
        ));
    }
    Ok(())
}

pub async fn get_task_existing(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let created = expect_task(
        client
            .send_message(Request::new(send_request("TCK: grpc get_task", None)))
            .await
            .map_err(|s| format!("SendMessage failed: {}: {}", s.code(), s.message()))?
            .into_inner(),
    )?;

    let fetched = client
        .get_task(Request::new(pb::GetTaskRequest {
            tenant: String::new(),
            id: created.id.clone(),
            history_length: None,
        }))
        .await
        .map_err(|s| format!("GetTask failed: {}: {}", s.code(), s.message()))?
        .into_inner();

    if fetched.id != created.id {
        return Err(format!(
            "task id mismatch: created '{}', retrieved '{}'",
            created.id, fetched.id
        ));
    }
    if fetched.status.is_none() {
        return Err("retrieved task has no status".to_string());
    }
    if fetched.context_id.is_empty() {
        return Err("retrieved task has an empty contextId".to_string());
    }
    Ok(())
}

/// §5.4 maps `TaskNotFoundError` to gRPC `NOT_FOUND`. Any other status — or a
/// success carrying a fabricated task — fails.
pub async fn get_task_not_found(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let status = client
        .get_task(Request::new(pb::GetTaskRequest {
            tenant: String::new(),
            id: "tck-nonexistent-task-2f9c8e11-does-not-exist".to_string(),
            history_length: None,
        }))
        .await
        .err()
        .ok_or_else(|| "GetTask on an unknown id must fail, got a task".to_string())?;

    if status.code() != Code::NotFound {
        return Err(format!(
            "§5.4 maps TaskNotFoundError to NOT_FOUND; got {}: {}",
            status.code(),
            status.message()
        ));
    }
    Ok(())
}

pub async fn list_tasks_basic(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    client
        .send_message(Request::new(send_request("TCK: grpc list_tasks", None)))
        .await
        .map_err(|s| format!("SendMessage failed: {}: {}", s.code(), s.message()))?;

    let listed = client
        .list_tasks(Request::new(pb::ListTasksRequest::default()))
        .await
        .map_err(|s| format!("ListTasks failed: {}: {}", s.code(), s.message()))?
        .into_inner();

    if listed.tasks.is_empty() {
        return Err("ListTasks returned no tasks after one was created".to_string());
    }
    Ok(())
}

/// `ListTasks` with an optional `authorization` metadata entry, for
/// `BIND-EQUIV-004`'s enforcement half.
///
/// Returns whether the call was **served**, not whether it succeeded in the
/// usual sense: any `Status` back is a refusal for this purpose, since the
/// question is only whether the binding applied the card's declared security.
/// Which status it chose is `BIND-EQUIV-003`'s subject.
///
/// A connect failure stays an `Err`. A gRPC listener that is simply down would
/// otherwise read as a binding correctly refusing anonymous callers, which is
/// the shape of false pass this kit exists to avoid.
pub async fn list_tasks_auth_probe(target: &str, token: Option<&str>) -> Result<bool, String> {
    let mut client = connect(target).await?;
    let mut request = Request::new(pb::ListTasksRequest::default());
    if let Some(t) = token {
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {t}")
                .parse()
                .map_err(|e| format!("gRPC metadata: {e}"))?,
        );
    }
    Ok(client.list_tasks(request).await.is_ok())
}

/// Cancelling is allowed to fail — a task already terminal is not cancelable —
/// but the failure must be the mapped status, not an arbitrary one.
pub async fn cancel_task(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let created = expect_task(
        client
            .send_message(Request::new(send_request("TCK: grpc cancel", None)))
            .await
            .map_err(|s| format!("SendMessage failed: {}: {}", s.code(), s.message()))?
            .into_inner(),
    )?;

    match client
        .cancel_task(Request::new(pb::CancelTaskRequest {
            id: created.id.clone(),
            ..Default::default()
        }))
        .await
    {
        Ok(resp) => {
            let task = resp.into_inner();
            let status = task.status.ok_or_else(|| "cancelled task has no status".to_string())?;
            decode_state(status.state)?;
            Ok(())
        }
        // §5.4: TaskNotCancelableError → FAILED_PRECONDITION. The echo-style
        // executors this runs against complete before the cancel lands, so
        // this is the expected path, not a fallback.
        Err(s) if s.code() == Code::FailedPrecondition => Ok(()),
        Err(s) => Err(format!(
            "CancelTask must succeed or map TaskNotCancelableError to FAILED_PRECONDITION; got {}: {}",
            s.code(),
            s.message()
        )),
    }
}

/// The stream must arrive and terminate: at least one payload, and a terminal
/// task state before the deadline.
pub async fn streaming_send_message(target: &str) -> Result<(), String> {
    use tokio_stream::StreamExt as _;

    let mut client = connect(target).await?;
    let mut stream = client
        .send_streaming_message(Request::new(send_request("TCK: grpc streaming", None)))
        .await
        .map_err(|s| format!("SendStreamingMessage failed: {}: {}", s.code(), s.message()))?
        .into_inner();

    let mut frames = 0usize;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let next = tokio::time::timeout_at(deadline, stream.next()).await;
        let Ok(item) = next else { break };
        let Some(item) = item else { break };
        let item = item.map_err(|s| format!("stream error: {}: {}", s.code(), s.message()))?;
        frames += 1;

        if let Some(state) = stream_state(&item) {
            if is_terminal(decode_state(state)?) {
                return Ok(());
            }
        }
    }

    if frames == 0 {
        return Err("gRPC streaming produced no messages".to_string());
    }
    Err(format!(
        "gRPC streaming produced {frames} message(s) but never reached a terminal state"
    ))
}

/// The task state carried by a `StreamResponse`, if the variant has one.
///
/// `Task` and `StatusUpdate` carry a status; `Message` and `ArtifactUpdate` do
/// not, and yield `None` so the caller keeps reading.
fn stream_state(item: &pb::StreamResponse) -> Option<i32> {
    match item.payload.as_ref()? {
        pb::stream_response::Payload::Task(t) => Some(t.status.as_ref()?.state),
        pb::stream_response::Payload::StatusUpdate(u) => Some(u.status.as_ref()?.state),
        pb::stream_response::Payload::Message(_)
        | pb::stream_response::Payload::ArtifactUpdate(_) => None,
    }
}

/// The four states §3.2 calls terminal.
const fn is_terminal(state: pb::TaskState) -> bool {
    matches!(
        state,
        pb::TaskState::Completed
            | pb::TaskState::Failed
            | pb::TaskState::Canceled
            | pb::TaskState::Rejected
    )
}

async fn create_task_and_config(
    client: &mut Client,
    label: &str,
) -> Result<(String, pb::TaskPushNotificationConfig), String> {
    let task = expect_task(
        client
            .send_message(Request::new(send_request(label, None)))
            .await
            .map_err(|s| format!("SendMessage failed: {}: {}", s.code(), s.message()))?
            .into_inner(),
    )?;
    let created = client
        .create_task_push_notification_config(Request::new(pb::TaskPushNotificationConfig {
            tenant: String::new(),
            id: String::new(),
            task_id: task.id.clone(),
            url: "https://example.com/webhook".to_string(),
            token: String::new(),
            authentication: None,
        }))
        .await
        .map_err(|s| {
            format!(
                "CreateTaskPushNotificationConfig failed: {}: {}",
                s.code(),
                s.message()
            )
        })?
        .into_inner();
    Ok((task.id, created))
}

pub async fn push_config_create(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let (_task_id, created) = create_task_and_config(&mut client, "TCK: grpc push create").await?;
    if created.id.is_empty() {
        return Err("created push config has an empty id".to_string());
    }
    Ok(())
}

pub async fn push_config_get(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let (task_id, created) = create_task_and_config(&mut client, "TCK: grpc push get").await?;

    let fetched = client
        .get_task_push_notification_config(Request::new(pb::GetTaskPushNotificationConfigRequest {
            tenant: String::new(),
            task_id,
            id: created.id.clone(),
        }))
        .await
        .map_err(|s| {
            format!(
                "GetTaskPushNotificationConfig failed: {}: {}",
                s.code(),
                s.message()
            )
        })?
        .into_inner();

    if fetched.id != created.id {
        return Err(format!(
            "push config id mismatch: created '{}', retrieved '{}'",
            created.id, fetched.id
        ));
    }
    Ok(())
}

pub async fn push_config_list(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let (task_id, _created) = create_task_and_config(&mut client, "TCK: grpc push list").await?;

    let listed = client
        .list_task_push_notification_configs(Request::new(
            pb::ListTaskPushNotificationConfigsRequest {
                tenant: String::new(),
                task_id,
                page_size: 0,
                page_token: String::new(),
            },
        ))
        .await
        .map_err(|s| {
            format!(
                "ListTaskPushNotificationConfigs failed: {}: {}",
                s.code(),
                s.message()
            )
        })?
        .into_inner();

    if listed.configs.is_empty() {
        return Err("expected at least one push config after creation".to_string());
    }
    Ok(())
}

pub async fn push_config_delete(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let (task_id, created) = create_task_and_config(&mut client, "TCK: grpc push delete").await?;

    client
        .delete_task_push_notification_config(Request::new(
            pb::DeleteTaskPushNotificationConfigRequest {
                tenant: String::new(),
                task_id: task_id.clone(),
                id: created.id.clone(),
            },
        ))
        .await
        .map_err(|s| {
            format!(
                "DeleteTaskPushNotificationConfig failed: {}: {}",
                s.code(),
                s.message()
            )
        })?;

    // Deleting must actually delete: a config still readable afterwards means
    // the RPC returned Empty and did nothing, which no status code reveals.
    let after = client
        .get_task_push_notification_config(Request::new(pb::GetTaskPushNotificationConfigRequest {
            tenant: String::new(),
            task_id,
            id: created.id.clone(),
        }))
        .await;
    match after {
        Err(s) if s.code() == Code::NotFound => Ok(()),
        Err(s) => Err(format!(
            "after delete, Get must be NOT_FOUND; got {}: {}",
            s.code(),
            s.message()
        )),
        Ok(_) => Err("push config is still readable after delete".to_string()),
    }
}

/// §10 has no method-name dispatch inside a request: an unimplemented method
/// is a distinct HTTP/2 path, and `tonic` answers `UNIMPLEMENTED`. The check
/// keeps its cross-binding name because the requirement is the same — the
/// server must reject a method it does not serve rather than answer it.
pub async fn invalid_method_returns_error(target: &str) -> Result<(), String> {
    let uri = if target.contains("://") {
        target.to_owned()
    } else {
        format!("http://{target}")
    };
    let status =
        raw_grpc_call(&uri, "/lf.a2a.v1.A2AService/NoSuchMethod", &[0, 0, 0, 0, 0]).await?;
    // grpc-status 12 = UNIMPLEMENTED.
    if status != Some(12) {
        return Err(format!(
            "an unknown method must answer grpc-status 12 (UNIMPLEMENTED); got {status:?}"
        ));
    }
    Ok(())
}

/// A request missing its required `message` must be rejected, not accepted.
///
/// §5.4 maps `InvalidParams` to `INVALID_ARGUMENT`; a server that instead
/// creates an empty task has silently invented content the client never sent.
pub async fn invalid_params_returns_error(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let empty = pb::SendMessageRequest::default();
    match client.send_message(Request::new(empty)).await {
        Ok(resp) => Err(format!(
            "SendMessage with no message must be rejected, got payload: {:?}",
            resp.into_inner().payload.is_some()
        )),
        Err(s) if s.code() == Code::InvalidArgument => Ok(()),
        Err(s) => Err(format!(
            "§5.4 maps InvalidParams to INVALID_ARGUMENT; got {}: {}",
            s.code(),
            s.message()
        )),
    }
}

/// A corrupt protobuf body must produce a gRPC status, never a result.
///
/// The generated client cannot send one — it serialises a typed message — so
/// this writes the request frame by hand: the 5-byte gRPC length prefix
/// followed by bytes that are not a valid `GetTaskRequest`. Field 1 is
/// declared `string` in the schema; wire type 5 (32-bit) contradicts that, so
/// any conforming decoder must reject it.
pub async fn malformed_body_returns_error(target: &str) -> Result<(), String> {
    let uri = if target.contains("://") {
        target.to_owned()
    } else {
        format!("http://{target}")
    };
    // tag 0x0d = field 1, wire type 5 (fixed32) where the schema says string.
    let payload: [u8; 5] = [0x0d, 0xff, 0xff, 0xff, 0xff];
    let mut framed = vec![0u8]; // uncompressed
    framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    framed.extend_from_slice(&payload);

    let status = raw_grpc_call(&uri, "/lf.a2a.v1.A2AService/GetTask", &framed).await?;
    match status {
        // 3 = INVALID_ARGUMENT, 13 = INTERNAL. Both are decode failures; what
        // matters is that it is an error and not OK.
        Some(0) => Err("a corrupt protobuf body was answered grpc-status 0 (OK)".to_string()),
        Some(_) => Ok(()),
        None => Err("a corrupt protobuf body produced no grpc-status at all".to_string()),
    }
}

/// POSTs a raw gRPC frame over HTTP/2 and returns the `grpc-status` the server
/// answered with, from either the headers or the trailers.
///
/// Exists because the two checks above need to send things the generated
/// client refuses to construct: an unknown method path, and a body that is not
/// a valid message.
async fn raw_grpc_call(base: &str, path: &str, body: &[u8]) -> Result<Option<i32>, String> {
    let client: hyper_util::client::legacy::Client<_, http_body_util::Full<hyper::body::Bytes>> =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .http2_only(true)
            .build_http();

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(format!("{}{path}", base.trim_end_matches('/')))
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(http_body_util::Full::new(hyper::body::Bytes::from(
            body.to_vec(),
        )))
        .map_err(|e| format!("build gRPC request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("raw gRPC request to {base}{path} failed: {e}"))?;

    // A "trailers-only" response carries grpc-status in the headers; a normal
    // one carries it in the trailers after the body. Check both.
    let from_headers = resp
        .headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let collected = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| format!("read gRPC response: {e}"))?;
    let from_trailers = collected
        .trailers()
        .and_then(|t| t.get("grpc-status"))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    Ok(from_trailers.or(from_headers))
}

/// Every state the server puts on the wire must be one the schema defines.
///
/// The cross-binding twin of the JSON `task_state_values` check, asserting the
/// same requirement against what is observable here: the JSON bindings can be
/// caught emitting `"completed"` instead of `"TASK_STATE_COMPLETED"`, while a
/// protobuf enum can only be caught emitting a number outside the schema.
pub async fn task_state_values(target: &str) -> Result<(), String> {
    let mut client = connect(target).await?;
    let task = expect_task(
        client
            .send_message(Request::new(send_request("TCK: grpc state values", None)))
            .await
            .map_err(|s| format!("SendMessage failed: {}: {}", s.code(), s.message()))?
            .into_inner(),
    )?;
    let status = task
        .status
        .ok_or_else(|| "task has no status".to_string())?;
    let state = decode_state(status.state)?;
    if state == pb::TaskState::Unspecified {
        return Err(
            "status.state is TASK_STATE_UNSPECIFIED, the proto3 default — the server \
             left the field unset rather than reporting a state"
                .to_string(),
        );
    }

    // ListTasks is the other place states cross the wire, and it is served by
    // a different code path than SendMessage.
    let listed = client
        .list_tasks(Request::new(pb::ListTasksRequest::default()))
        .await
        .map_err(|s| format!("ListTasks failed: {}: {}", s.code(), s.message()))?
        .into_inner();
    for t in &listed.tasks {
        let Some(st) = t.status.as_ref() else {
            return Err(format!("listed task '{}' has no status", t.id));
        };
        decode_state(st.state)?;
    }
    Ok(())
}

// ── Cross-binding equivalence support (§5.1) ─────────────────────────────────

/// Whether the server reports this method as `UNIMPLEMENTED`.
///
/// The §10 half of `BIND-EQUIV-001`'s availability probe. Only the
/// `UNIMPLEMENTED` status answers the question §5.1 asks; every other outcome,
/// success or failure, means the method exists.
pub async fn probe_method(
    target: &str,
    method: &str,
    task_id: &str,
    config_id: &str,
) -> Result<bool, String> {
    use tokio_stream::StreamExt as _;

    let mut c = connect(target).await?;
    let code = match method {
        "SendMessage" => c
            .send_message(Request::new(send_request("TCK: equivalence probe", None)))
            .await
            .err(),
        "SendStreamingMessage" => {
            match c
                .send_streaming_message(Request::new(send_request("TCK: equivalence probe", None)))
                .await
            {
                Err(s) => Some(s),
                // The status of a server-streaming call can arrive with the
                // first message rather than the response head, so drain one
                // before concluding the method exists.
                Ok(resp) => resp.into_inner().next().await.and_then(Result::err),
            }
        }
        "GetTask" => c
            .get_task(Request::new(pb::GetTaskRequest {
                id: task_id.to_owned(),
                ..Default::default()
            }))
            .await
            .err(),
        "ListTasks" => c
            .list_tasks(Request::new(pb::ListTasksRequest::default()))
            .await
            .err(),
        "CancelTask" => c
            .cancel_task(Request::new(pb::CancelTaskRequest {
                id: task_id.to_owned(),
                ..Default::default()
            }))
            .await
            .err(),
        "SubscribeToTask" => {
            match c
                .subscribe_to_task(Request::new(pb::SubscribeToTaskRequest {
                    id: task_id.to_owned(),
                    ..Default::default()
                }))
                .await
            {
                Err(s) => Some(s),
                Ok(resp) => resp.into_inner().next().await.and_then(Result::err),
            }
        }
        "CreateTaskPushNotificationConfig" => c
            .create_task_push_notification_config(Request::new(pb::TaskPushNotificationConfig {
                task_id: task_id.to_owned(),
                url: "https://example.com/webhook".to_owned(),
                ..Default::default()
            }))
            .await
            .err(),
        "GetTaskPushNotificationConfig" => c
            .get_task_push_notification_config(Request::new(
                pb::GetTaskPushNotificationConfigRequest {
                    task_id: task_id.to_owned(),
                    id: config_id.to_owned(),
                    ..Default::default()
                },
            ))
            .await
            .err(),
        "ListTaskPushNotificationConfigs" => c
            .list_task_push_notification_configs(Request::new(
                pb::ListTaskPushNotificationConfigsRequest {
                    task_id: task_id.to_owned(),
                    ..Default::default()
                },
            ))
            .await
            .err(),
        "DeleteTaskPushNotificationConfig" => c
            .delete_task_push_notification_config(Request::new(
                pb::DeleteTaskPushNotificationConfigRequest {
                    task_id: task_id.to_owned(),
                    id: config_id.to_owned(),
                    ..Default::default()
                },
            ))
            .await
            .err(),
        "GetExtendedAgentCard" => c
            .get_extended_agent_card(Request::new(pb::GetExtendedAgentCardRequest::default()))
            .await
            .err(),
        other => return Err(format!("no gRPC probe defined for {other}")),
    };

    Ok(code.is_some_and(|s| s.code() == Code::Unimplemented))
}

/// Reads a task over gRPC and reduces it to the same semantic view the JSON
/// bindings produce, so `BIND-EQUIV-002` compares content and not encoding.
///
/// The enum-to-name mapping here is the one place a protobuf value has to be
/// rendered as a string to be comparable with JSON. That is sound for an
/// equivalence check — both sides are being reduced to a common vocabulary —
/// and unsound for a wire-format check, which is why `task_state_values`
/// asserts on the enum instead.
pub async fn task_view(
    target: &str,
    task_id: &str,
) -> Result<crate::equivalence::TaskView, String> {
    let mut client = connect(target).await?;
    let task = client
        .get_task(Request::new(pb::GetTaskRequest {
            id: task_id.to_owned(),
            ..Default::default()
        }))
        .await
        .map_err(|s| format!("GetTask failed: {}: {}", s.code(), s.message()))?
        .into_inner();

    let state = task
        .status
        .as_ref()
        .map(|s| decode_state(s.state))
        .transpose()?
        .map_or_else(String::new, state_wire_name);

    Ok(crate::equivalence::TaskView {
        id: task.id,
        context_id: task.context_id,
        state,
        artifact_text: task
            .artifacts
            .iter()
            .flat_map(|a| {
                a.parts.iter().filter_map(|p| match p.content.as_ref() {
                    Some(pb::part::Content::Text(t)) => Some(t.clone()),
                    _ => None,
                })
            })
            .collect(),
    })
}

/// The `ProtoJSON` name of a `TaskState`, which is what the JSON bindings put
/// on the wire for the same value.
fn state_wire_name(state: pb::TaskState) -> String {
    match state {
        pb::TaskState::Unspecified => "TASK_STATE_UNSPECIFIED",
        pb::TaskState::Submitted => "TASK_STATE_SUBMITTED",
        pb::TaskState::Working => "TASK_STATE_WORKING",
        pb::TaskState::Completed => "TASK_STATE_COMPLETED",
        pb::TaskState::Failed => "TASK_STATE_FAILED",
        pb::TaskState::Canceled => "TASK_STATE_CANCELED",
        pb::TaskState::InputRequired => "TASK_STATE_INPUT_REQUIRED",
        pb::TaskState::Rejected => "TASK_STATE_REJECTED",
        pb::TaskState::AuthRequired => "TASK_STATE_AUTH_REQUIRED",
    }
    .to_owned()
}

/// Triggers a fault and reports the gRPC status it mapped to, for
/// `BIND-EQUIV-003`.
pub async fn fault_code(
    target: &str,
    method: &str,
    task_id: &str,
) -> Result<(String, i32), String> {
    let mut client = connect(target).await?;
    let status = match method {
        "GetTask" => client
            .get_task(Request::new(pb::GetTaskRequest {
                id: task_id.to_owned(),
                ..Default::default()
            }))
            .await
            .err(),
        "CancelTask" => client
            .cancel_task(Request::new(pb::CancelTaskRequest {
                id: task_id.to_owned(),
                ..Default::default()
            }))
            .await
            .err(),
        other => return Err(format!("no gRPC fault trigger defined for {other}")),
    };
    Ok(status.map_or_else(
        || ("OK".to_owned(), 0),
        |s| (format!("{:?}", s.code()).to_uppercase(), s.code() as i32),
    ))
}

#[cfg(test)]
mod tests {
    use super::{decode_state, is_terminal, pb, state_wire_name, stream_state};

    #[test]
    fn decode_state_rejects_values_the_schema_does_not_define() {
        assert_eq!(decode_state(2), Ok(pb::TaskState::Working));
        let err = decode_state(9999).expect_err("9999 is not a TaskState");
        assert!(err.contains("9999"), "{err}");
    }

    #[test]
    fn terminal_states_are_the_four_spec_names() {
        for terminal in [
            pb::TaskState::Completed,
            pb::TaskState::Failed,
            pb::TaskState::Canceled,
            pb::TaskState::Rejected,
        ] {
            assert!(is_terminal(terminal), "{terminal:?} is terminal");
        }
        for non_terminal in [
            pb::TaskState::Unspecified,
            pb::TaskState::Submitted,
            pb::TaskState::Working,
            pb::TaskState::InputRequired,
            pb::TaskState::AuthRequired,
        ] {
            assert!(!is_terminal(non_terminal), "{non_terminal:?} is not");
        }
    }

    #[test]
    fn stream_state_reads_both_status_carrying_variants() {
        let with_status = |state| {
            Some(pb::TaskStatus {
                state: state as i32,
                message: None,
                timestamp: None,
            })
        };

        let task = pb::StreamResponse {
            payload: Some(pb::stream_response::Payload::Task(pb::Task {
                status: with_status(pb::TaskState::Submitted),
                ..Default::default()
            })),
        };
        assert_eq!(stream_state(&task), Some(pb::TaskState::Submitted as i32));

        let update = pb::StreamResponse {
            payload: Some(pb::stream_response::Payload::StatusUpdate(
                pb::TaskStatusUpdateEvent {
                    status: with_status(pb::TaskState::Completed),
                    ..Default::default()
                },
            )),
        };
        assert_eq!(stream_state(&update), Some(pb::TaskState::Completed as i32));
    }

    /// `BIND-EQUIV-002` compares a gRPC task against a JSON one, so the enum
    /// has to render to the exact `ProtoJSON` name the JSON bindings emit. A
    /// wrong spelling here would report a real equivalence as a mismatch.
    #[test]
    fn every_state_renders_to_its_protojson_name() {
        let expected = [
            (pb::TaskState::Unspecified, "TASK_STATE_UNSPECIFIED"),
            (pb::TaskState::Submitted, "TASK_STATE_SUBMITTED"),
            (pb::TaskState::Working, "TASK_STATE_WORKING"),
            (pb::TaskState::InputRequired, "TASK_STATE_INPUT_REQUIRED"),
            (pb::TaskState::AuthRequired, "TASK_STATE_AUTH_REQUIRED"),
            (pb::TaskState::Completed, "TASK_STATE_COMPLETED"),
            (pb::TaskState::Failed, "TASK_STATE_FAILED"),
            (pb::TaskState::Canceled, "TASK_STATE_CANCELED"),
            (pb::TaskState::Rejected, "TASK_STATE_REJECTED"),
        ];
        for (state, name) in expected {
            assert_eq!(state_wire_name(state), name);
        }
        // Anchored to the schema: a state added to the proto without a name
        // here would make the mapping silently incomplete.
        assert_eq!(
            expected.len(),
            9,
            "lf.a2a.v1.TaskState has 9 values; every one needs a wire name"
        );
        for (state, _) in expected {
            assert!(
                decode_state(state as i32).is_ok(),
                "{state:?} must round-trip through the schema"
            );
        }
    }

    #[test]
    fn stateless_stream_variants_yield_none() {
        let artifact = pb::StreamResponse {
            payload: Some(pb::stream_response::Payload::ArtifactUpdate(
                pb::TaskArtifactUpdateEvent::default(),
            )),
        };
        assert_eq!(stream_state(&artifact), None);

        let empty = pb::StreamResponse { payload: None };
        assert_eq!(stream_state(&empty), None);
    }
}
