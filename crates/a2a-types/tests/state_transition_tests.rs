// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for task state transition validation.

use a2a_protocol_types::task::TaskState;

#[test]
fn terminal_states_cannot_transition() {
    let terminals = [
        TaskState::Completed,
        TaskState::Failed,
        TaskState::Canceled,
        TaskState::Rejected,
    ];
    let all_states = [
        TaskState::Unspecified,
        TaskState::Submitted,
        TaskState::Working,
        TaskState::InputRequired,
        TaskState::AuthRequired,
        TaskState::Completed,
        TaskState::Failed,
        TaskState::Canceled,
        TaskState::Rejected,
    ];

    for terminal in &terminals {
        for target in &all_states {
            assert!(
                !terminal.can_transition_to(*target),
                "{terminal} should not transition to {target}"
            );
        }
    }
}

#[test]
fn submitted_valid_transitions() {
    let valid = [
        TaskState::Working,
        TaskState::Failed,
        TaskState::Canceled,
        TaskState::Rejected,
    ];
    for target in &valid {
        assert!(
            TaskState::Submitted.can_transition_to(*target),
            "Submitted -> {target} should be valid"
        );
    }
}

#[test]
fn submitted_invalid_transitions() {
    let invalid = [
        TaskState::Completed,
        TaskState::InputRequired,
        TaskState::AuthRequired,
    ];
    for target in &invalid {
        assert!(
            !TaskState::Submitted.can_transition_to(*target),
            "Submitted -> {target} should be invalid"
        );
    }
}

#[test]
fn working_valid_transitions() {
    let valid = [
        TaskState::Completed,
        TaskState::Failed,
        TaskState::Canceled,
        TaskState::InputRequired,
        TaskState::AuthRequired,
    ];
    for target in &valid {
        assert!(
            TaskState::Working.can_transition_to(*target),
            "Working -> {target} should be valid"
        );
    }
}

#[test]
fn working_cannot_go_to_submitted_or_rejected() {
    assert!(
        !TaskState::Working.can_transition_to(TaskState::Submitted),
        "Working -> Submitted should be invalid"
    );
    assert!(
        !TaskState::Working.can_transition_to(TaskState::Rejected),
        "Working -> Rejected should be invalid"
    );
}

#[test]
fn input_required_valid_transitions() {
    let valid = [TaskState::Working, TaskState::Failed, TaskState::Canceled];
    for target in &valid {
        assert!(
            TaskState::InputRequired.can_transition_to(*target),
            "InputRequired -> {target} should be valid"
        );
    }
}

#[test]
fn auth_required_valid_transitions() {
    let valid = [TaskState::Working, TaskState::Failed, TaskState::Canceled];
    for target in &valid {
        assert!(
            TaskState::AuthRequired.can_transition_to(*target),
            "AuthRequired -> {target} should be valid"
        );
    }
}

#[test]
fn unspecified_can_transition_to_anything() {
    let all = [
        TaskState::Submitted,
        TaskState::Working,
        TaskState::InputRequired,
        TaskState::AuthRequired,
        TaskState::Completed,
        TaskState::Failed,
        TaskState::Canceled,
        TaskState::Rejected,
    ];
    for target in &all {
        assert!(
            TaskState::Unspecified.can_transition_to(*target),
            "Unspecified -> {target} should be valid"
        );
    }
}

#[test]
fn completed_to_working_is_invalid() {
    assert!(
        !TaskState::Completed.can_transition_to(TaskState::Working),
        "Completed -> Working should be invalid (terminal state)"
    );
}

#[test]
fn task_state_display_format() {
    assert_eq!(TaskState::Working.to_string(), "TASK_STATE_WORKING");
    assert_eq!(
        TaskState::InputRequired.to_string(),
        "TASK_STATE_INPUT_REQUIRED"
    );
}
