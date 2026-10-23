// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Each REQUIRED field, set to what the JSON parser still reads as a value,
//! is refused with an error naming it; the same object with it set passes.

use super::*;
use crate::error::ErrorCode;

fn task(json: serde_json::Value) -> Task {
    serde_json::from_value(json).expect("task parses")
}

fn valid_task() -> serde_json::Value {
    serde_json::json!({
        "id": "t-1",
        "contextId": "c-1",
        "status": {"state": "TASK_STATE_COMPLETED"},
        "artifacts": [{"artifactId": "a-1", "parts": [{"text": "out"}]}],
        "history": [{"messageId": "m-1", "role": "ROLE_USER", "parts": [{"text": "in"}]}]
    })
}

/// `field` refused, as `InvalidParams` whose message contains `needle`.
fn refused(result: A2aResult<()>, needle: &str) {
    let err = result.expect_err(needle);
    assert_eq!(err.code, ErrorCode::InvalidParams, "{needle}");
    assert!(err.message.contains(needle), "{needle}: {}", err.message);
}

#[test]
fn a_valid_task_passes() {
    check_task(&task(valid_task())).expect("valid task");
    check_received_event(&StreamResponse::Task(task(valid_task()))).expect("valid task event");
}

#[test]
fn a_task_missing_a_required_value_is_refused() {
    type Break = fn(&mut serde_json::Value);
    let cases: [(&str, Break); 7] = [
        ("task: id is required", |t| t["id"] = "".into()),
        ("TASK_STATE_UNSPECIFIED is no state", |t| {
            t["status"]["state"] = "TASK_STATE_UNSPECIFIED".into();
        }),
        ("artifactId is required", |t| {
            t["artifacts"][0]["artifactId"] = "".into();
        }),
        // `parts` absent: serde reads it as empty (ProtoJSON omits an empty
        // repeated field), so only this check refuses it.
        ("artifact must contain at least one part", |t| {
            t["artifacts"][0].as_object_mut().unwrap().remove("parts");
        }),
        ("history message: messageId is required", |t| {
            t["history"][0]["messageId"] = "".into();
        }),
        ("history message: role is required", |t| {
            t["history"][0]["role"] = "ROLE_UNSPECIFIED".into();
        }),
        ("status message must contain at least one part", |t| {
            t["status"]["message"] =
                serde_json::json!({"messageId": "s", "role": "ROLE_AGENT", "parts": []});
        }),
    ];
    for (needle, break_it) in cases {
        let mut json = valid_task();
        break_it(&mut json);
        refused(check_task(&task(json.clone())), needle);
        refused(
            check_received_event(&StreamResponse::Task(task(json))),
            needle,
        );
    }
}

#[test]
fn an_update_without_its_task_or_context_id_is_refused() {
    let status = serde_json::json!({
        "taskId": "t", "contextId": "c", "status": {"state": "TASK_STATE_WORKING"}
    });
    let artifact = serde_json::json!({
        "taskId": "t", "contextId": "c",
        "artifact": {"artifactId": "a", "parts": [{"text": "x"}]}
    });
    let parse = |key: &str, v: &serde_json::Value| -> StreamResponse {
        serde_json::from_value(serde_json::json!({ key: v })).expect("event parses")
    };
    check_received_event(&parse("statusUpdate", &status)).expect("valid status update");
    check_received_event(&parse("artifactUpdate", &artifact)).expect("valid artifact update");
    for (key, base, name) in [
        ("statusUpdate", &status, "statusUpdate"),
        ("artifactUpdate", &artifact, "artifactUpdate"),
    ] {
        for field in ["taskId", "contextId"] {
            let mut json = base.clone();
            json[field] = "".into();
            let event = parse(key, &json);
            refused(
                check_received_event(&event),
                &format!("{name}: {field} is required"),
            );
            // The server stamps these on its executor's events.
            check_event(&event).expect("ids are not checked here");
        }
    }
    let mut empty_chunk = artifact;
    empty_chunk["artifact"]["parts"] = serde_json::json!([]);
    refused(
        check_received_event(&parse("artifactUpdate", &empty_chunk)),
        "at least one part",
    );
}

#[test]
fn a_direct_reply_is_checked_as_a_message() {
    let reply = |json: serde_json::Value| -> SendMessageResponse {
        SendMessageResponse::Message(serde_json::from_value(json).expect("message parses"))
    };
    let ok = serde_json::json!({"messageId": "m", "role": "ROLE_AGENT", "parts": [{"text": "hi"}]});
    check_response(&reply(ok.clone())).expect("valid reply");
    let mut no_parts = ok;
    no_parts.as_object_mut().unwrap().remove("parts");
    refused(
        check_response(&reply(no_parts)),
        "message must contain at least one part",
    );
    let mut task_json = valid_task();
    task_json["id"] = "".into();
    refused(
        check_response(&SendMessageResponse::Task(task(task_json))),
        "task: id is required",
    );
}

#[test]
fn a_push_config_needs_its_url_and_auth_scheme() {
    let mut config = TaskPushNotificationConfig::new("t", "https://hook.example/cb");
    check_push_config(&config).expect("valid config");
    config.authentication = Some(crate::push::AuthenticationInfo {
        scheme: String::new(),
        credentials: None,
    });
    refused(check_push_config(&config), "authentication.scheme");
    config.authentication = None;
    config.url.clear();
    refused(check_push_config(&config), "url is required");
}

#[test]
fn a_card_is_held_to_the_publishing_rules() {
    let card = || -> serde_json::Value {
        serde_json::json!({
            "name": "a", "description": "d", "version": "1",
            "supportedInterfaces": [{"url": "https://a.example", "protocolBinding": "JSONRPC", "protocolVersion": "1.0"}],
            "capabilities": {},
            "defaultInputModes": ["text/plain"], "defaultOutputModes": ["text/plain"],
            "skills": [{"id": "s", "name": "S", "description": "d", "tags": ["t"]}]
        })
    };
    let parse = |json: serde_json::Value| -> AgentCard {
        serde_json::from_value(json).expect("card parses")
    };
    check_card(&parse(card())).expect("valid card");
    let mut no_skills = card();
    no_skills.as_object_mut().unwrap().remove("skills");
    refused(check_card(&parse(no_skills)), "at least one skill");
    // `validate_security_schemes` runs too, not only `validate`.
    let mut scheme = card();
    scheme["securitySchemes"] =
        serde_json::json!({"k": {"apiKeySecurityScheme": {"location": "header", "name": ""}}});
    refused(
        check_card(&parse(scheme)),
        "apiKeySecurityScheme needs a name",
    );
}
