// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Counter-tests: the calls that must be *refused*.
//!
//! # Why these matter as much as the sweep
//!
//! `demos::sweep` proves the eleven methods work. It cannot prove the server
//! is doing anything other than saying yes. An implementation that accepted
//! every request, returned a plausible task for anything, and never enforced a
//! capability would pass the sweep with a full matrix.
//!
//! Each check here names the specific wrong answer that would indicate a real
//! defect, so a pass means "the server refused, for the documented reason" and
//! not merely "something went wrong".

use a2a_protocol_client::{A2aClient, ClientError};
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::params::TaskQueryParams;
use a2a_protocol_types::push::TaskPushNotificationConfig;

use crate::sweep::make_send_params;

/// Outcome of the counter-test pass.
pub struct CounterOutcome {
    /// Human-readable lines, one per check.
    pub lines: Vec<String>,
    /// Checks whose refusal did not happen, or happened for the wrong reason.
    pub failures: Vec<String>,
}

#[cfg(test)]
mod tests;

fn code_of(e: &ClientError) -> Option<ErrorCode> {
    match e {
        ClientError::Protocol(p) => Some(p.code),
        _ => None,
    }
}

/// Runs every counter-test against `client`.
///
/// `restricted` must be a client pointed at an agent whose card advertises
/// **no** optional capabilities — that is the only way to observe the
/// capability refusals, since one agent cannot both support and not support a
/// feature. Standing up that second agent is the whole reason this function
/// takes two clients.
pub async fn run(client: &A2aClient, restricted: &A2aClient) -> CounterOutcome {
    let mut lines = Vec::new();
    let mut failures = Vec::new();

    macro_rules! expect_code {
        ($label:expr, $want:expr, $call:expr) => {{
            match $call.await {
                Ok(v) => {
                    let msg = format!(
                        "{}: expected {:?}, but the call SUCCEEDED ({v:?})",
                        $label, $want
                    );
                    lines.push(format!("  [FAIL] {msg}"));
                    failures.push(msg);
                }
                Err(e) => match code_of(&e) {
                    Some(got) if got == $want => {
                        lines.push(format!("  [ok]   {:<44} refused: {:?}", $label, got));
                    }
                    Some(got) => {
                        let msg =
                            format!("{}: expected {:?}, got {:?} ({e})", $label, $want, got);
                        lines.push(format!("  [FAIL] {msg}"));
                        failures.push(msg);
                    }
                    None => {
                        let msg = format!(
                            "{}: expected a protocol refusal {:?}, got a transport error: {e}",
                            $label, $want
                        );
                        lines.push(format!("  [FAIL] {msg}"));
                        failures.push(msg);
                    }
                },
            }
        }};
    }

    // Unknown task must be reported, not invented.
    expect_code!(
        "GetTask on an unknown id",
        ErrorCode::TaskNotFound,
        client.get_task(TaskQueryParams {
            tenant: None,
            id: "definitely-not-a-real-task-id".into(),
            history_length: None,
        })
    );

    expect_code!(
        "CancelTask on an unknown id",
        ErrorCode::TaskNotFound,
        client.cancel_task("definitely-not-a-real-task-id".to_owned())
    );

    // Capability gating (spec §3.1.11): an agent that does not advertise a
    // feature must refuse it rather than quietly serving it.
    expect_code!(
        "push config against a card without pushNotifications",
        ErrorCode::PushNotificationNotSupported,
        restricted.set_push_config(TaskPushNotificationConfig {
            tenant: None,
            id: None,
            task_id: Some("any".into()),
            url: "http://127.0.0.1:1/webhook".into(),
            token: None,
            authentication: None,
        })
    );

    expect_code!(
        "extended card against a card without extendedAgentCard",
        ErrorCode::UnsupportedOperation,
        restricted.get_extended_agent_card()
    );

    // A streaming request to an agent that never advertised streaming.
    match restricted.stream_message(make_send_params("nope")).await {
        Ok(_) => {
            let msg = "streaming against a card without streaming: expected a \
                       refusal, but the stream opened"
                .to_owned();
            lines.push(format!("  [FAIL] {msg}"));
            failures.push(msg);
        }
        Err(e) => {
            lines.push(format!(
                "  [ok]   {:<44} refused: {e}",
                "streaming against a card without streaming"
            ));
        }
    }

    CounterOutcome { lines, failures }
}
