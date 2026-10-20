// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Ingress checks on the identifiers a client supplies: message, context,
//! task and push-config ids.
//!
//! Spec §14: implementations "MUST sanitize user-provided content". An id
//! is stored, keys the stores, and is logged — `GetTask` and `CancelTask`
//! log the id they were asked for — so a control character in one is how
//! log injection starts: `"a\r\nb"` wrote a line of the client's choosing.
//! The idempotency key was charset-restricted for exactly that reason while
//! every id beside it was not. A control character is refused; anything
//! else is the client's choice, letters outside ASCII included.

use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::TaskId;

use crate::error::{ServerError, ServerResult};

/// Refuses a value holding a control character (`char::is_control`: C0,
/// DEL and C1, which is where CR, LF, NUL and NEL are).
///
/// For the ids that have no length bound of their own to check against —
/// the push-config ids and a `ListTasks` context filter — and for
/// [`validate_id`], which checks it on the raw value: the trim it measures
/// length by would hide a trailing `\n`, and the raw value is what is
/// stored.
pub(super) fn refuse_control_chars(raw: &str, name: &str) -> ServerResult<()> {
    if raw.chars().any(char::is_control) {
        return Err(ServerError::InvalidField {
            field: name.to_owned(),
            message: format!("{name} must not contain control characters"),
        });
    }
    Ok(())
}

/// The client's ids in a push config to be stored, by
/// `CreateTaskPushNotificationConfig` or inline in `SendMessage`: its `id`,
/// which no check reached, and its `taskId`.
pub(super) fn refuse_control_chars_in_push_config(
    config: &TaskPushNotificationConfig,
) -> ServerResult<()> {
    refuse_control_chars(config.id.as_deref().unwrap_or_default(), "id")?;
    refuse_control_chars(config.task_id.as_deref().unwrap_or_default(), "taskId")
}

/// The `contextId` filter of `ListTasks`: not stored, but handed to every
/// store backend and its logs, so refused as the id it names would be.
pub(super) fn refuse_control_chars_in_filter(params: &ListTasksParams) -> ServerResult<()> {
    refuse_control_chars(
        params.context_id.as_deref().unwrap_or_default(),
        "contextId",
    )
}

/// Validates an ID string: rejects empty/whitespace-only values, control
/// characters and excessively long values.
pub(super) fn validate_id(raw: &str, name: &str, max_length: usize) -> ServerResult<()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ServerError::InvalidField {
            field: name.to_owned(),
            message: format!("{name} must not be empty or whitespace-only"),
        });
    }
    refuse_control_chars(raw, name)?;
    if trimmed.len() > max_length {
        return Err(ServerError::InvalidField {
            field: name.to_owned(),
            message: format!(
                "{name} exceeds maximum length (got {}, max {max_length})",
                trimmed.len()
            ),
        });
    }
    Ok(())
}

/// The `id` of `GetTask`, `CancelTask` or `SubscribeToTask`, which a2a.proto marks
/// REQUIRED. gRPC's absent string and JSON's `""` used to reach the store and
/// answer `TaskNotFound`; they are `InvalidParams`.
pub(super) fn task_id_param(raw: &str, max_length: usize) -> ServerResult<TaskId> {
    validate_id(raw, "id", max_length)?;
    Ok(TaskId::new(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(raw: &str) -> Option<String> {
        match validate_id(raw, "message.id", 1024) {
            Ok(()) => None,
            Err(ServerError::InvalidField { field, message }) => {
                assert_eq!(field, "message.id", "{raw:?}");
                Some(message)
            }
            Err(other) => panic!("{raw:?}: not InvalidField: {other:?}"),
        }
    }

    #[test]
    fn validate_id_rejects_crlf_inside_an_id() {
        let message = refusal("a\r\nb").expect("CR LF must be refused");
        assert!(message.contains("control characters"), "{message}");
        assert!(message.contains("message.id"), "{message}");
    }

    /// C0, DEL and C1 each, at the start, the middle and the end. The end
    /// matters on its own: `trim` strips a trailing `\n` before the length
    /// check, and the id stored is the untrimmed one.
    #[test]
    fn validate_id_rejects_every_control_character_anywhere() {
        for c in [
            '\0', '\t', '\n', '\r', '\u{1b}', '\u{1f}', '\u{7f}', '\u{85}', '\u{9f}',
        ] {
            for raw in [format!("{c}id"), format!("i{c}d"), format!("id{c}")] {
                let message = refusal(&raw).unwrap_or_else(|| panic!("{raw:?} accepted"));
                assert!(message.contains("control characters"), "{raw:?}: {message}");
            }
        }
    }

    /// Letters outside ASCII, emoji, and the punctuation real ids carry are
    /// not control characters, and pass.
    #[test]
    fn validate_id_accepts_unicode_letters_and_punctuation() {
        for raw in [
            "tâche-任务-задача",
            "مهمة",
            "🚀-launch",
            "550e8400-e29b-41d4-a716-446655440000",
            "tck-complete-task-1a2b3c",
            "ctx/1?a=%2F#é.. ",
            "a b+c:d",
        ] {
            assert_eq!(refusal(raw), None, "{raw:?}");
        }
    }

    /// An empty or whitespace-only id keeps the message it always had, even
    /// when that whitespace is a control character such as `\t` or `\n`.
    #[test]
    fn validate_id_reports_whitespace_only_as_empty_first() {
        let message = refusal(" \t\n ").expect("refused");
        assert!(message.contains("empty"), "{message}");
    }

    #[test]
    fn task_id_param_rejects_a_control_character() {
        let err = task_id_param("t\u{0}1", 1024).unwrap_err();
        assert!(
            matches!(err, ServerError::InvalidField { ref field, .. } if field == "id"),
            "{err:?}"
        );
    }

    /// `isolatable` refuses a tenant holding the caller separator by
    /// refusing every control character, which holds while it is one.
    #[test]
    fn caller_separator_is_a_control_character() {
        assert!(crate::store::tenant::CALLER_SEPARATOR.is_control());
    }

    #[test]
    fn refuse_control_chars_in_push_config_checks_id_and_task_id() {
        let config = |id: &str, task: &str| TaskPushNotificationConfig {
            id: Some(id.to_owned()),
            ..TaskPushNotificationConfig::new(task, "https://example.com/hook")
        };
        assert!(refuse_control_chars_in_push_config(&config("cfg-é", "t-1")).is_ok());
        for (bad, field) in [(config("c\r\n", "t"), "id"), (config("c", "t\n"), "taskId")] {
            let err = refuse_control_chars_in_push_config(&bad).unwrap_err();
            assert!(
                matches!(err, ServerError::InvalidField { field: ref f, .. } if f == field),
                "{err:?}"
            );
        }
    }

    #[test]
    fn refuse_control_chars_names_the_field_and_passes_the_rest() {
        assert!(refuse_control_chars("cfg-é", "id").is_ok());
        assert!(
            refuse_control_chars("", "id").is_ok(),
            "emptiness is not its check"
        );
        let err = refuse_control_chars("cfg\n", "taskId").unwrap_err();
        assert!(
            matches!(err, ServerError::InvalidField { ref field, ref message }
                if field == "taskId" && message.contains("taskId")),
            "{err:?}"
        );
    }
}
