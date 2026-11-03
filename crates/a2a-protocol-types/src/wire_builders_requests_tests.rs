// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The builders of the request parameters and paged responses: `new` yields
//! exactly the required fields, and each `with_*` sets its own field and no
//! other. The generated structs do not implement `PartialEq`, so the values are
//! compared as the JSON they serialize to, which is the wire form.

use serde_json::{Value, json};

use super::*;
use crate::task::{Task, TaskStatus};

fn json_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serializes")
}

/// `got` equals `base` with `key` set to `want` and nothing else changed.
#[track_caller]
fn only_sets(base: &Value, got: Value, key: &str, want: Value) {
    let mut expected = base.clone();
    expected
        .as_object_mut()
        .expect("an object")
        .insert(key.to_owned(), want);
    assert_eq!(
        got, expected,
        "the setter for {key} must set {key} and no other field"
    );
}

#[test]
fn send_message_configuration_new_is_the_default_and_each_setter_sets_one_field() {
    assert_eq!(
        json_of(&SendMessageConfiguration::new()),
        json_of(&SendMessageConfiguration::default())
    );
    let base = json_of(&SendMessageConfiguration::new());
    only_sets(
        &base,
        json_of(&SendMessageConfiguration::new().with_accepted_output_modes(vec!["a/b".into()])),
        "acceptedOutputModes",
        json!(["a/b"]),
    );
    only_sets(
        &base,
        json_of(&SendMessageConfiguration::new().with_history_length(7)),
        "historyLength",
        json!(7),
    );
    only_sets(
        &base,
        json_of(&SendMessageConfiguration::new().with_return_immediately(true)),
        "returnImmediately",
        json!(true),
    );
    let push = TaskPushNotificationConfig::for_url("https://example.test/hook");
    only_sets(
        &base,
        json_of(&SendMessageConfiguration::new().with_task_push_notification_config(push.clone())),
        "taskPushNotificationConfig",
        json_of(&push),
    );
    // `false` is a value, not an absence.
    let off = SendMessageConfiguration::new().with_return_immediately(false);
    assert_eq!(off.return_immediately, Some(false));
}

#[test]
fn list_tasks_params_new_is_empty_and_each_setter_sets_one_field() {
    let base = json_of(&ListTasksParams::new());
    assert_eq!(base, json!({}));
    only_sets(
        &base,
        json_of(&ListTasksParams::new().with_tenant("t")),
        "tenant",
        json!("t"),
    );
    only_sets(
        &base,
        json_of(&ListTasksParams::new().with_context_id("c")),
        "contextId",
        json!("c"),
    );
    only_sets(
        &base,
        json_of(&ListTasksParams::new().with_status(TaskState::Working)),
        "status",
        json_of(&TaskState::Working),
    );
    only_sets(
        &base,
        json_of(&ListTasksParams::new().with_page_size(25)),
        "pageSize",
        json!(25),
    );
    only_sets(
        &base,
        json_of(&ListTasksParams::new().with_page_token("p")),
        "pageToken",
        json!("p"),
    );
    only_sets(
        &base,
        json_of(&ListTasksParams::new().with_history_length(3)),
        "historyLength",
        json!(3),
    );
    only_sets(
        &base,
        json_of(&ListTasksParams::new().with_status_timestamp_after("2026-11-01T00:00:00Z")),
        "statusTimestampAfter",
        json!("2026-11-01T00:00:00Z"),
    );
    only_sets(
        &base,
        json_of(&ListTasksParams::new().with_include_artifacts(true)),
        "includeArtifacts",
        json!(true),
    );
}

#[test]
fn cancel_task_params_new_sets_the_id_and_each_setter_sets_one_field() {
    let base = json_of(&CancelTaskParams::new("t1"));
    assert_eq!(base, json!({"id": "t1"}));
    only_sets(
        &base,
        json_of(&CancelTaskParams::new("t1").with_tenant("acme")),
        "tenant",
        json!("acme"),
    );
    only_sets(
        &base,
        json_of(&CancelTaskParams::new("t1").with_metadata(json!({"why": "x"}))),
        "metadata",
        json!({"why": "x"}),
    );
}

#[test]
fn push_config_params_take_the_task_id_first_and_the_config_id_second() {
    for (base, tenanted) in [
        (
            json_of(&GetPushConfigParams::new("task-1", "cfg-1")),
            json_of(&GetPushConfigParams::new("task-1", "cfg-1").with_tenant("acme")),
        ),
        (
            json_of(&DeletePushConfigParams::new("task-1", "cfg-1")),
            json_of(&DeletePushConfigParams::new("task-1", "cfg-1").with_tenant("acme")),
        ),
    ] {
        assert_eq!(base, json!({"taskId": "task-1", "id": "cfg-1"}));
        only_sets(&base, tenanted, "tenant", json!("acme"));
    }
}

#[test]
fn task_id_params_new_sets_the_id_and_with_tenant_sets_the_tenant() {
    let base = json_of(&TaskIdParams::new("t1"));
    assert_eq!(base, json!({"id": "t1"}));
    only_sets(
        &base,
        json_of(&TaskIdParams::new("t1").with_tenant("acme")),
        "tenant",
        json!("acme"),
    );
}

#[test]
fn list_push_configs_params_new_sets_the_task_id_and_each_setter_sets_one_field() {
    let base = json_of(&ListPushConfigsParams::new("t1"));
    assert_eq!(base, json!({"taskId": "t1"}));
    only_sets(
        &base,
        json_of(&ListPushConfigsParams::new("t1").with_page_size(9)),
        "pageSize",
        json!(9),
    );
    only_sets(
        &base,
        json_of(&ListPushConfigsParams::new("t1").with_page_token("p")),
        "pageToken",
        json!("p"),
    );
    only_sets(
        &base,
        json_of(&ListPushConfigsParams::new("t1").with_tenant("acme")),
        "tenant",
        json!("acme"),
    );
}

#[test]
fn get_extended_agent_card_params_new_is_empty_and_with_tenant_sets_the_tenant() {
    let base = json_of(&GetExtendedAgentCardParams::new());
    assert_eq!(base, json!({}));
    assert_eq!(json_of(&GetExtendedAgentCardParams::default()), base);
    only_sets(
        &base,
        json_of(&GetExtendedAgentCardParams::new().with_tenant("acme")),
        "tenant",
        json!("acme"),
    );
}

#[test]
fn task_list_response_setters_replace_what_new_computed() {
    let task = |id: &str| Task::new(id, "c", TaskStatus::new(TaskState::Working));
    let base = json_of(&TaskListResponse::new(vec![task("a"), task("b")]));
    assert_eq!(base["pageSize"], json!(2));
    assert_eq!(base["totalSize"], json!(2));
    only_sets(
        &base,
        json_of(&TaskListResponse::new(vec![task("a"), task("b")]).with_next_page_token("next")),
        "nextPageToken",
        json!("next"),
    );
    only_sets(
        &base,
        json_of(&TaskListResponse::new(vec![task("a"), task("b")]).with_page_size(1)),
        "pageSize",
        json!(1),
    );
    only_sets(
        &base,
        json_of(&TaskListResponse::new(vec![task("a"), task("b")]).with_total_size(40)),
        "totalSize",
        json!(40),
    );
}

#[test]
fn list_push_configs_response_new_has_no_next_page_and_the_setter_adds_one() {
    let cfg = TaskPushNotificationConfig::for_url("https://example.test/hook");
    let base = json_of(&ListPushConfigsResponse::new(vec![cfg.clone()]));
    assert_eq!(base, json!({"configs": [json_of(&cfg)]}));
    only_sets(
        &base,
        json_of(&ListPushConfigsResponse::new(vec![cfg]).with_next_page_token("n")),
        "nextPageToken",
        json!("n"),
    );
}
