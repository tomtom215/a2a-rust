// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Shared helpers for TCK tests.

use serde_json::Value;

/// Sends a JSON-RPC request and returns the parsed response.
pub async fn jsonrpc_request(url: &str, method: &str, params: Value) -> Result<Value, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": uuid::Uuid::new_v4().to_string(),
        "method": method,
        "params": params
    });

    http_post(url, &body).await
}

/// Sends a JSON-RPC request over a WebSocket connection.
///
/// The `websocket` binding carries the *identical* JSON-RPC envelope as the
/// `jsonrpc` binding — §12 defines a custom binding, not a custom protocol —
/// so this differs from [`jsonrpc_request`] only in carriage. That is what
/// makes cross-binding equivalence meaningful to assert: the two bindings are
/// compared on the same request, not on two hand-written approximations of it.
///
/// One connection per call. Wasteful, and deliberate: it keeps each check
/// independent, so a server that mishandles connection reuse cannot make a
/// later check pass on state left by an earlier one.
pub async fn ws_jsonrpc_request(url: &str, method: &str, params: Value) -> Result<Value, String> {
    use futures_util::{SinkExt as _, StreamExt as _};
    use tokio_tungstenite::tungstenite::Message;

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": uuid::Uuid::new_v4().to_string(),
        "method": method,
        "params": params
    });

    let ws_url = to_ws_url(url);
    let mut req = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
        ws_url.as_str(),
    )
    .map_err(|e| format!("websocket: bad url {ws_url}: {e}"))?;
    // §3.6.1: clients send A2A-Version on every request, and the server's
    // strict default rejects a handshake without it.
    req.headers_mut().insert(
        "a2a-version",
        "1.0".parse().map_err(|e| format!("header: {e}"))?,
    );

    let (mut ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(|e| format!("websocket connect to {ws_url} failed: {e}"))?;

    ws.send(Message::Text(body.to_string().into()))
        .await
        .map_err(|e| format!("websocket send failed: {e}"))?;

    // Read until a text frame arrives; ignore ping/pong housekeeping.
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(20), ws.next())
            .await
            .map_err(|_| "websocket: timed out awaiting response".to_string())?
            .ok_or_else(|| "websocket: closed before responding".to_string())?
            .map_err(|e| format!("websocket read failed: {e}"))?;
        match msg {
            Message::Text(text) => {
                return serde_json::from_str(&text)
                    .map_err(|e| format!("websocket: response is not JSON: {e}: {text}"));
            }
            Message::Close(_) => return Err("websocket: closed before responding".to_string()),
            _ => continue,
        }
    }
}

/// Sends one raw text frame and returns the first text frame that comes back.
///
/// The §12 counterpart to [`post_raw`]: it exists so a malformed *frame* can
/// be tested the way a malformed HTTP *body* is. Deliberately takes `&str`
/// rather than a `Value` so the payload can be syntactically invalid JSON —
/// serialising a `Value` could not produce one.
///
/// `Ok(None)` means the server closed the connection or went quiet instead of
/// answering; that is a distinct outcome from an error frame, and callers
/// decide whether it conforms.
pub async fn ws_send_raw(url: &str, payload: &str) -> Result<Option<Value>, String> {
    use futures_util::{SinkExt as _, StreamExt as _};
    use tokio_tungstenite::tungstenite::Message;

    let ws_url = to_ws_url(url);
    let mut req = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
        ws_url.as_str(),
    )
    .map_err(|e| format!("websocket: bad url {ws_url}: {e}"))?;
    req.headers_mut().insert(
        "a2a-version",
        "1.0".parse().map_err(|e| format!("header: {e}"))?,
    );

    let (mut ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(|e| format!("websocket connect to {ws_url} failed: {e}"))?;
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|e| format!("websocket send failed: {e}"))?;

    loop {
        let next = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next()).await;
        match next {
            // No reply before the deadline, or the peer hung up: report the
            // silence rather than inventing a response for it.
            Err(_) | Ok(None) => return Ok(None),
            Ok(Some(Err(e))) => return Err(format!("websocket read failed: {e}")),
            Ok(Some(Ok(Message::Text(text)))) => {
                return serde_json::from_str(&text)
                    .map(Some)
                    .map_err(|e| format!("websocket: response is not JSON: {e}: {text}"));
            }
            Ok(Some(Ok(Message::Close(_)))) => return Ok(None),
            Ok(Some(Ok(_))) => continue,
        }
    }
}

// ── Credentialed variants, for BIND-EQUIV-004's enforcement half ─────────────
//
// Separate entry points rather than an extra parameter on the existing
// helpers. Every other check in this kit drives an *unauthenticated* target
// and must keep doing so unchanged; threading an `Option<&str>` through them
// all would put a credential argument on 40 call sites to serve one check.
//
// These are also the only helpers that may send `Authorization`. Keeping that
// in three named functions means "which requests can carry a credential?" is
// answerable by reading this section, rather than by auditing every caller.

/// [`jsonrpc_request`] with an optional `Authorization: Bearer` header.
pub async fn jsonrpc_call_with_auth(
    url: &str,
    method: &str,
    params: Value,
    token: Option<&str>,
) -> Result<Value, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": uuid::Uuid::new_v4().to_string(),
        "method": method,
        "params": params
    });
    let body_bytes = serde_json::to_vec(&body).map_err(|e| format!("serialize: {e}"))?;

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();
    let mut req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(url)
        .header("content-type", "application/json")
        .header("a2a-version", "1.0");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let req = req
        .body(http_body_util::Full::new(hyper::body::Bytes::from(
            body_bytes,
        )))
        .map_err(|e| format!("build request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();
    serde_json::from_slice(&body).map_err(|e| {
        format!(
            "parse response: {e} (body: {})",
            String::from_utf8_lossy(&body)
        )
    })
}

/// [`http_get`] with an optional `Authorization: Bearer` header.
pub async fn http_get_with_auth(url: &str, token: Option<&str>) -> Result<(u16, Value), String> {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();
    let mut req = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(url)
        .header("a2a-version", "1.0");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let req = req
        .body(http_body_util::Full::new(hyper::body::Bytes::new()))
        .map_err(|e| format!("build request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status().as_u16();
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();
    // A refusal may not carry a JSON body; the status is the signal, so an
    // unparseable body becomes Null rather than an error.
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    Ok((status, value))
}

/// [`ws_jsonrpc_request`] with an optional `Authorization: Bearer` header on
/// the handshake.
///
/// The header goes on the HTTP upgrade, which is where a WebSocket client
/// presents credentials — and is exactly the step a binding could forget to
/// forward into its `CallContext`.
pub async fn ws_jsonrpc_request_with_auth(
    url: &str,
    method: &str,
    params: Value,
    token: Option<&str>,
) -> Result<Value, String> {
    use futures_util::{SinkExt as _, StreamExt as _};
    use tokio_tungstenite::tungstenite::Message;

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": uuid::Uuid::new_v4().to_string(),
        "method": method,
        "params": params
    });

    let ws_url = to_ws_url(url);
    let mut req = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
        ws_url.as_str(),
    )
    .map_err(|e| format!("websocket: bad url {ws_url}: {e}"))?;
    req.headers_mut().insert(
        "a2a-version",
        "1.0".parse().map_err(|e| format!("header: {e}"))?,
    );
    if let Some(t) = token {
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {t}")
                .parse()
                .map_err(|e| format!("header: {e}"))?,
        );
    }

    // A server that refuses the credential may reject the *handshake* rather
    // than answering with an error frame. That is a refusal, not a transport
    // fault, so it is reported as a JSON-RPC-shaped error the caller can read
    // the same way as an in-band one.
    let (mut ws, _) = match tokio_tungstenite::connect_async(req).await {
        Ok(pair) => pair,
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            return Ok(serde_json::json!({
                "error": {
                    "code": resp.status().as_u16(),
                    "message": "websocket handshake rejected",
                }
            }));
        }
        Err(e) => return Err(format!("websocket connect to {ws_url} failed: {e}")),
    };

    ws.send(Message::Text(body.to_string().into()))
        .await
        .map_err(|e| format!("websocket send failed: {e}"))?;

    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(20), ws.next())
            .await
            .map_err(|_| "websocket: timed out awaiting response".to_string())?
            .ok_or_else(|| "websocket: closed before responding".to_string())?
            .map_err(|e| format!("websocket read failed: {e}"))?;
        match msg {
            Message::Text(text) => {
                return serde_json::from_str(&text)
                    .map_err(|e| format!("websocket: response is not JSON: {e}: {text}"));
            }
            Message::Close(_) => return Err("websocket: closed before responding".to_string()),
            _ => continue,
        }
    }
}

/// Normalises an `http(s)://` or bare authority into a `ws(s)://` URL.
pub fn to_ws_url_public(url: &str) -> String {
    to_ws_url(url)
}

fn to_ws_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if url.starts_with("ws://") || url.starts_with("wss://") {
        url.to_owned()
    } else {
        format!("ws://{url}")
    }
}

/// Issues a JSON-RPC call over whichever transport `binding` names.
///
/// Only the envelope-carrying bindings are valid here; `rest` has its own
/// shape and its own helpers.
pub async fn rpc(url: &str, binding: &str, method: &str, params: Value) -> Result<Value, String> {
    match binding {
        "jsonrpc" => jsonrpc_request(url, method, params).await,
        "websocket" => ws_jsonrpc_request(url, method, params).await,
        other => Err(format!("rpc: {other} is not an envelope binding")),
    }
}

/// Calls `method` and reports the JSON-RPC error code, if the answer was an
/// error envelope at all.
///
/// `Ok(None)` covers every non-error answer, including the ones [`rpc`] cannot
/// parse: `SendStreamingMessage` over §9 replies with an SSE body, not a JSON
/// envelope, and that is a perfectly good sign the method exists. Used by the
/// §5.1 availability probe, which only needs to tell "no such method" from
/// everything else.
pub async fn rpc_probe(
    url: &str,
    binding: &str,
    method: &str,
    params: Value,
) -> Result<Option<i64>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": uuid::Uuid::new_v4().to_string(),
        "method": method,
        "params": params
    });
    let resp = match binding {
        "jsonrpc" => {
            let bytes = serde_json::to_vec(&body).map_err(|e| format!("serialize: {e}"))?;
            // `post_raw` yields Value::Null for a non-JSON body rather than
            // failing, which is exactly the SSE case above.
            post_raw(url, "/", &bytes, "application/json").await?
        }
        "websocket" => ws_jsonrpc_request(url, method, params).await?,
        other => return Err(format!("rpc_probe: {other} is not an envelope binding")),
    };
    Ok(resp
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(Value::as_i64))
}

/// Sends a REST request (POST for actions, GET for queries).
pub async fn rest_post(url: &str, path: &str, body: &Value) -> Result<Value, String> {
    let full_url = format!("{url}{path}");
    http_post(&full_url, body).await
}

/// Sends a REST GET request.
pub async fn rest_get(url: &str, path: &str) -> Result<(u16, Value), String> {
    let full_url = format!("{url}{path}");
    http_get(&full_url).await
}

/// Issues `verb path` and returns only the status code.
///
/// The equivalence checks need verbs `rest_get`/`rest_post` do not cover
/// (`DELETE`) and care about the status rather than the body — a `404` with a
/// live task id is the signal that a §5.3 route is absent. Bodies are sent
/// only where the route requires one, so a `400` from a missing body cannot be
/// mistaken for a missing route.
pub async fn http_status(url: &str, verb: &str, path: &str) -> Result<u16, String> {
    let full_url = format!("{url}{path}");
    let method = hyper::Method::from_bytes(verb.as_bytes())
        .map_err(|e| format!("bad HTTP verb {verb}: {e}"))?;

    // POST routes in §5.3 take a body; the send/stream pair needs a valid
    // message or the server rejects it before routing is observable.
    let body: Vec<u8> = if method == hyper::Method::POST {
        if path.starts_with("/message:") {
            serde_json::to_vec(&make_send_params("TCK: route probe"))
                .map_err(|e| format!("serialize: {e}"))?
        } else {
            b"{}".to_vec()
        }
    } else {
        Vec::new()
    };

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();
    let req = hyper::Request::builder()
        .method(method)
        .uri(&full_url)
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .body(http_body_util::Full::new(hyper::body::Bytes::from(body)))
        .map_err(|e| format!("build request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    Ok(resp.status().as_u16())
}

/// Low-level HTTP POST that returns parsed JSON.
async fn http_post(url: &str, body: &Value) -> Result<Value, String> {
    http_post_with_content_type(url, body, "application/json").await
}

/// POSTs raw (possibly malformed) bytes and returns the parsed JSON body
/// (or `Value::Null` when the response is not JSON).
pub async fn post_raw(
    url: &str,
    path: &str,
    body: &[u8],
    content_type: &str,
) -> Result<Value, String> {
    let (_status, value) = post_raw_status(url, path, body, content_type).await?;
    Ok(value)
}

/// POSTs raw bytes and returns `(status, body)`; the body is `Value::Null`
/// when the response is not JSON.
pub async fn post_raw_status(
    url: &str,
    path: &str,
    body: &[u8],
    content_type: &str,
) -> Result<(u16, Value), String> {
    let full_url = format!("{url}{path}");
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();
    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(&full_url)
        .header("content-type", content_type)
        .header("a2a-version", "1.0")
        .body(http_body_util::Full::new(hyper::body::Bytes::from(
            body.to_vec(),
        )))
        .map_err(|e| format!("build request: {e}"))?;
    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status().as_u16();
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Ok((status, value))
}

/// Low-level HTTP POST with an explicit Content-Type header.
async fn http_post_with_content_type(
    url: &str,
    body: &Value,
    content_type: &str,
) -> Result<Value, String> {
    let body_bytes = serde_json::to_vec(body).map_err(|e| format!("serialize: {e}"))?;

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(url)
        .header("content-type", content_type)
        // Spec §3.6.1: an absent A2A-Version header is interpreted as 0.3,
        // which a v1.0-only server MUST reject — so every conformance
        // request declares the version it is testing.
        .header("a2a-version", "1.0")
        .body(http_body_util::Full::new(hyper::body::Bytes::from(
            body_bytes,
        )))
        .map_err(|e| format!("build request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();

    serde_json::from_slice(&body).map_err(|e| {
        format!(
            "parse response: {e} (body: {})",
            String::from_utf8_lossy(&body)
        )
    })
}

/// Low-level HTTP GET that returns status code and parsed JSON.
async fn http_get(url: &str) -> Result<(u16, Value), String> {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let req = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(url)
        .header("a2a-version", "1.0")
        .body(http_body_util::Full::new(hyper::body::Bytes::new()))
        .map_err(|e| format!("build request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status().as_u16();

    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();

    // Error responses from paths outside the A2A surface (e.g. a framework
    // 404 for an unrouted path) are legitimately non-JSON — the reference
    // Python SDK returns a plain-text "Not Found". Callers decide from the
    // status code; surface a non-JSON body as `Value::Null` rather than
    // failing the test on body shape.
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);

    Ok((status, json))
}

/// Creates a minimal SendMessage params with a text message (v1.0 wire format).
pub fn make_send_params(text: &str) -> Value {
    serde_json::json!({
        "message": {
            "role": "ROLE_USER",
            "parts": [{"text": text}],
            "messageId": uuid::Uuid::new_v4().to_string()
        }
    })
}

/// Creates SendMessage params with a specific context ID (v1.0 wire format).
///
/// Per v1.0 proto, context_id is on the Message, not at the params level.
pub fn make_send_params_with_context(text: &str, context_id: &str) -> Value {
    serde_json::json!({
        "message": {
            "role": "ROLE_USER",
            "parts": [{"text": text}],
            "messageId": uuid::Uuid::new_v4().to_string(),
            "contextId": context_id
        },
        "configuration": {
            "acceptedOutputModes": ["text/plain"]
        }
    })
}

/// Sends a message via the appropriate binding and returns the response.
///
/// For JSON-RPC: extracts the `result` from the JSON-RPC envelope.
/// For REST: returns the response body directly.
///
/// In v1.0, `SendMessageResponse` is externally tagged: `{"task": {...}}` or
/// `{"message": {...}}`. This function returns the raw result — callers should
/// use [`extract_task`] to unwrap the task from the response.
pub async fn send_message(url: &str, binding: &str, params: Value) -> Result<Value, String> {
    match binding {
        "jsonrpc" | "websocket" => {
            let resp = rpc(url, binding, "SendMessage", params).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result")
                .cloned()
                .ok_or_else(|| "missing 'result' in JSON-RPC response".to_string())
        }
        "rest" => rest_post(url, "/message:send", &params).await,
        _ => Err(format!("unknown binding: {binding}")),
    }
}

/// Sends a message like [`send_message`], but with the A2A media type
/// `application/a2a+json` instead of plain `application/json`.
///
/// Production A2A clients (including `a2a-protocol-client`) send this
/// registered media type, so servers must accept it.
pub async fn send_message_a2a_media_type(
    url: &str,
    binding: &str,
    params: Value,
) -> Result<Value, String> {
    const A2A_MEDIA_TYPE: &str = "application/a2a+json";
    match binding {
        // Reached only if the runner's HTTP_BODY_ONLY scope is widened. Fail
        // loudly rather than quietly POSTing over HTTP and filing the verdict
        // under the WebSocket binding's name.
        "websocket" => Err(
            "application/a2a+json is an HTTP Content-Type; a §12 text frame carries none"
                .to_string(),
        ),
        "jsonrpc" => {
            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "id": uuid::Uuid::new_v4().to_string(),
                "method": "SendMessage",
                "params": params
            });
            let resp = http_post_with_content_type(url, &body, A2A_MEDIA_TYPE).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result")
                .cloned()
                .ok_or_else(|| "missing 'result' in JSON-RPC response".to_string())
        }
        "rest" => {
            let full_url = format!("{url}/message:send");
            http_post_with_content_type(&full_url, &params, A2A_MEDIA_TYPE).await
        }
        _ => Err(format!("unknown binding: {binding}")),
    }
}

/// Extracts a Task object from a v1.0 `SendMessageResponse`.
///
/// The response is externally tagged: `{"task": {...}}` or `{"message": {...}}`.
/// Also accepts bare Task objects (v0.3 untagged format) for backward compat.
pub fn extract_task(result: &Value) -> Result<&Value, String> {
    // v1.0: externally tagged {"task": {...}}
    if let Some(task) = result.get("task") {
        return Ok(task);
    }
    // v0.3 fallback: bare task object with "id" field
    if result.get("id").is_some() && result.get("status").is_some() {
        return Ok(result);
    }
    Err(format!("response is not a task (got: {result})"))
}

/// Gets a task by ID via the appropriate binding.
pub async fn get_task(url: &str, binding: &str, task_id: &str) -> Result<Value, String> {
    match binding {
        "jsonrpc" | "websocket" => {
            let params = serde_json::json!({"id": task_id});
            let resp = rpc(url, binding, "GetTask", params).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result")
                .cloned()
                .ok_or_else(|| "missing 'result' in JSON-RPC response".to_string())
        }
        "rest" => {
            let (status, body) = rest_get(url, &format!("/tasks/{task_id}")).await?;
            if status >= 400 {
                return Err(format!("HTTP {status}: {body}"));
            }
            Ok(body)
        }
        _ => Err(format!("unknown binding: {binding}")),
    }
}

/// Gets a task that is expected to not exist — returns Ok if error, Err if found.
pub async fn get_task_expect_not_found(
    url: &str,
    binding: &str,
    task_id: &str,
) -> Result<(), String> {
    match binding {
        "jsonrpc" | "websocket" => {
            let params = serde_json::json!({"id": task_id});
            let resp = rpc(url, binding, "GetTask", params).await?;
            if resp.get("error").is_some() {
                Ok(())
            } else {
                Err("expected error for non-existent task, got result".to_string())
            }
        }
        "rest" => {
            let (status, _) = rest_get(url, &format!("/tasks/{task_id}")).await?;
            if status == 404 || status >= 400 {
                Ok(())
            } else {
                Err(format!("expected 404 for non-existent task, got {status}"))
            }
        }
        _ => Err(format!("unknown binding: {binding}")),
    }
}
