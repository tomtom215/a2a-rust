// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! P2.2: Security-critical error path tests.
//!
//! Covers ID length boundaries (context_id and task_id at max length),
//! empty IDs, and whitespace-only IDs.

use super::*;

// 12. ID length boundary: exactly 1024 chars passes, 1025 fails

#[tokio::test]
async fn context_id_exactly_max_length_passes() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let long_id = "x".repeat(1024);
    let mut msg = make_message("test");
    msg.context_id = Some(ContextId::new(&long_id));

    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let result = handler.on_send_message(params, false, None).await;
    if let Err(ref e) = result {
        panic!("context_id of exactly 1024 chars should be accepted, got {e:?}");
    }
}

#[tokio::test]
async fn context_id_over_max_length_fails() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let long_id = "x".repeat(1025);
    let mut msg = make_message("test");
    msg.context_id = Some(ContextId::new(&long_id));

    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false, None).await);
    let err_msg = format!("{err:?}");
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "context_id of 1025 chars should be rejected with InvalidParams, got {err_msg}"
    );
}

#[tokio::test]
async fn task_id_exactly_max_length_passes() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let long_id = "t".repeat(1024);
    let mut msg = make_message("test");
    msg.task_id = Some(TaskId::new(&long_id));

    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    // This will either succeed (if no existing task with that ID) or fail
    // for a reason other than length validation.
    let result = handler.on_send_message(params, false, None).await;
    // Should NOT fail with "exceeds maximum length"
    if let Err(ref e) = result {
        let msg = format!("{e}");
        assert!(
            !msg.contains("exceeds maximum length"),
            "task_id of exactly 1024 chars should not fail length check: {msg}"
        );
    }
}

#[tokio::test]
async fn task_id_over_max_length_fails() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let long_id = "t".repeat(1025);
    let mut msg = make_message("test");
    msg.task_id = Some(TaskId::new(&long_id));

    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false, None).await);
    let err_msg = format!("{err:?}");
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "task_id of 1025 chars should be rejected with InvalidParams, got {err_msg}"
    );
}

// 13. Empty string IDs rejected

#[tokio::test]
async fn empty_context_id_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let mut msg = make_message("test");
    msg.context_id = Some(ContextId::new(""));

    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false, None).await);
    let err_msg = format!("{err:?}");
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "empty context_id should be rejected with InvalidParams, got {err_msg}"
    );
}

#[tokio::test]
async fn empty_task_id_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let mut msg = make_message("test");
    msg.task_id = Some(TaskId::new(""));

    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false, None).await);
    let err_msg = format!("{err:?}");
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "empty task_id should be rejected with InvalidParams, got {err_msg}"
    );
}

// 14. Whitespace-only IDs rejected

#[tokio::test]
async fn whitespace_only_context_id_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let mut msg = make_message("test");
    msg.context_id = Some(ContextId::new("   \t\n  "));

    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false, None).await);
    let err_msg = format!("{err:?}");
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "whitespace-only context_id should be rejected with InvalidParams, got {err_msg}"
    );
}

#[tokio::test]
async fn whitespace_only_task_id_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let mut msg = make_message("test");
    msg.task_id = Some(TaskId::new("   "));

    let params = MessageSendParams {
        tenant: None,
        context_id: None,
        message: msg,
        configuration: None,
        metadata: None,
    };

    let err = unwrap_send_err(handler.on_send_message(params, false, None).await);
    let err_msg = format!("{err:?}");
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(_)),
        "whitespace-only task_id should be rejected with InvalidParams, got {err_msg}"
    );
}
