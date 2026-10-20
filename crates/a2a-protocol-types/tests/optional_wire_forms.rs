// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Wire forms the specification leaves optional, each asserted on what the
//! parser and serializer actually do:
//!
//! * spec §5.6.1: a timestamp without millisecond precision MAY omit the
//!   fractional seconds or zero-fill them;
//! * a2a.proto `ImplicitOAuthFlow.scopes` and `PasswordOAuthFlow.scopes`:
//!   "The map MAY be empty";
//! * spec Appendix A.2: a server MAY accept both legacy and current request
//!   forms, and emits only the current one.

#![allow(deprecated)] // the implicit and password flows are deprecated in A2A 1.0

use a2a_protocol_types::message::{Message, MessageRole};
use a2a_protocol_types::parse_iso8601_to_unix_millis;
use a2a_protocol_types::security::{ImplicitFlow, OAuthFlows, PasswordOAuthFlow};
use a2a_protocol_types::task::TaskState;
use serde_json::json;

// ── §5.6.1 fractional seconds ───────────────────────────────────────────────

/// 2026-03-15T14:30:45Z, in milliseconds since the epoch.
const INSTANT: i64 = 1_773_585_045_000;

#[test]
fn timestamp_without_or_with_zero_filled_fraction_names_the_same_instant() {
    for form in [
        "2026-03-15T14:30:45Z",
        "2026-03-15T14:30:45.0Z",
        "2026-03-15T14:30:45.000Z",
        "2026-03-15T14:30:45.000000Z",
    ] {
        assert_eq!(
            parse_iso8601_to_unix_millis(form),
            Some(INSTANT),
            "{form} must read as the same whole second"
        );
    }
    // The control: a fraction that is not zero is read, so the forms above
    // agree because their fractions are zero or absent, not because the
    // fraction is ignored.
    assert_eq!(
        parse_iso8601_to_unix_millis("2026-03-15T14:30:45.007Z"),
        Some(INSTANT + 7)
    );
}

// ── a2a.proto: the implicit and password flows' scopes MAY be empty ─────────

#[test]
fn implicit_flow_scopes_may_be_empty_or_omitted() {
    for wire in [
        json!({"implicit": {"authorizationUrl": "https://id.example.com/authorize"}}),
        json!({"implicit": {"authorizationUrl": "https://id.example.com/authorize", "scopes": {}}}),
    ] {
        let flows: OAuthFlows = serde_json::from_value(wire.clone())
            .unwrap_or_else(|e| panic!("{wire} must parse: {e}"));
        let OAuthFlows::Implicit(flow) = &flows else {
            panic!("expected implicit, got {flows:?}");
        };
        assert!(flow.scopes.is_empty(), "{wire}");
        assert_eq!(flow.authorization_url, "https://id.example.com/authorize");
    }
    // An empty map survives the round trip as an empty map.
    let flow = ImplicitFlow {
        authorization_url: "https://id.example.com/authorize".into(),
        refresh_url: None,
        scopes: std::collections::HashMap::new(),
    };
    let back: OAuthFlows =
        serde_json::from_value(serde_json::to_value(OAuthFlows::Implicit(flow)).unwrap()).unwrap();
    assert!(matches!(back, OAuthFlows::Implicit(ref f) if f.scopes.is_empty()));
}

#[test]
fn password_flow_scopes_may_be_empty_or_omitted() {
    for wire in [
        json!({"password": {"tokenUrl": "https://id.example.com/token"}}),
        json!({"password": {"tokenUrl": "https://id.example.com/token", "scopes": {}}}),
    ] {
        let flows: OAuthFlows = serde_json::from_value(wire.clone())
            .unwrap_or_else(|e| panic!("{wire} must parse: {e}"));
        let OAuthFlows::Password(flow) = &flows else {
            panic!("expected password, got {flows:?}");
        };
        assert!(flow.scopes.is_empty(), "{wire}");
        assert_eq!(flow.token_url, "https://id.example.com/token");
    }
    let flow = PasswordOAuthFlow {
        token_url: "https://id.example.com/token".into(),
        refresh_url: None,
        scopes: std::collections::HashMap::new(),
    };
    let back: OAuthFlows =
        serde_json::from_value(serde_json::to_value(OAuthFlows::Password(flow)).unwrap()).unwrap();
    assert!(matches!(back, OAuthFlows::Password(ref f) if f.scopes.is_empty()));
}

// ── Appendix A.2: legacy forms in, current forms out ────────────────────────

#[test]
fn legacy_enum_and_field_forms_are_accepted_and_only_current_forms_are_emitted() {
    // v0.3 spelled the role and the task state in lower case.
    let state: TaskState = serde_json::from_value(json!("input-required")).unwrap();
    assert_eq!(state, TaskState::InputRequired);
    assert_eq!(
        serde_json::to_value(state).unwrap(),
        json!("TASK_STATE_INPUT_REQUIRED")
    );
    let state: TaskState = serde_json::from_value(json!("completed")).unwrap();
    assert_eq!(
        serde_json::to_value(state).unwrap(),
        json!("TASK_STATE_COMPLETED")
    );

    let legacy = json!({
        "message_id": "m-1",
        "context_id": "ctx-1",
        "task_id": "t-1",
        "role": "user",
        "parts": [{"text": "hi"}]
    });
    let message: Message = serde_json::from_value(legacy).expect("the legacy form is accepted");
    assert_eq!(message.role, MessageRole::User);
    assert_eq!(message.id.0, "m-1");
    assert_eq!(
        message.context_id.as_ref().map(|c| c.0.as_str()),
        Some("ctx-1")
    );

    let emitted = serde_json::to_value(&message).unwrap();
    assert_eq!(emitted["role"], "ROLE_USER", "{emitted}");
    assert_eq!(emitted["messageId"], "m-1", "{emitted}");
    assert_eq!(emitted["contextId"], "ctx-1", "{emitted}");
    assert_eq!(emitted["taskId"], "t-1", "{emitted}");
    for legacy_key in ["message_id", "context_id", "task_id"] {
        assert!(
            emitted.get(legacy_key).is_none(),
            "{legacy_key} must not be emitted: {emitted}"
        );
    }
}
