// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Cross-binding equivalence: A2A v1.0 §5.1.
//!
//! Every other check in this kit grades one binding at a time. §5.1 cannot be
//! graded that way — its four `MUST`s are all statements about the *relation*
//! between bindings, and each is trivially satisfiable by any single one of
//! them. The official suite records the same shape problem: its `task-28`
//! notes these "require cross-transport comparison tests … a different testing
//! pattern than single-transport tests", and all four have sat `NOT TESTED`
//! since April.
//!
//! The requirement texts are quoted verbatim from
//! `a2aproject/a2a-tck@5996b79` (2026-06-29), `tck/requirements/interop.py`:
//!
//! | ID | Level | Title | Description |
//! |---|---|---|---|
//! | `BIND-EQUIV-001` | MUST | All supported protocols provide identical functionality | "When an agent supports multiple protocols, all supported protocols MUST provide the same set of operations and capabilities." |
//! | `BIND-EQUIV-002` | MUST | All bindings return semantically equivalent results | "All supported protocols MUST return semantically equivalent results for the same requests." |
//! | `BIND-EQUIV-003` | MUST | All bindings map errors consistently | "All supported protocols MUST map errors consistently using appropriate protocol-specific codes." |
//! | `BIND-EQUIV-004` | MUST | All bindings support same authentication schemes | "All supported protocols MUST support the same authentication schemes declared in the AgentCard." |
//!
//! One upstream discrepancy, noted because it changes what 004 means: the
//! backlog ticket `task-28` summarises `BIND-EQUIV-004` as "Streaming
//! equivalence", while `interop.py` — the file the suite actually loads —
//! defines it as the authentication-scheme requirement above. This module
//! follows `interop.py`.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::runner::{Outcome, TestResult};
use crate::tests::helpers;

/// A binding the target advertises and this kit can drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Iface {
    /// This kit's binding name (`jsonrpc`, `rest`, `websocket`, `grpc`).
    pub binding: String,
    /// The endpoint from the card.
    pub url: String,
    /// The `protocolBinding` string exactly as the card spelled it, so
    /// messages name what the operator published rather than our alias.
    pub declared: String,
}

/// Maps a card's `protocolBinding` to the binding name this kit drives.
///
/// §5.3's canonical names are `JSONRPC`, `GRPC` and `HTTP+JSON`; `REST` is the
/// legacy spelling of the last, and `WEBSOCKET` is §12's custom binding. An
/// unrecognised name is not an error — §12 explicitly permits custom bindings,
/// and a kit that cannot drive one should say so rather than fail the agent.
pub fn binding_for(declared: &str) -> Option<&'static str> {
    match declared.to_ascii_uppercase().as_str() {
        "JSONRPC" => Some("jsonrpc"),
        "HTTP+JSON" | "REST" => Some("rest"),
        "WEBSOCKET" => Some("websocket"),
        "GRPC" => Some("grpc"),
        _ => None,
    }
}

/// Reads the card and returns the interfaces this kit can drive, plus the
/// `protocolBinding` names it had to skip.
pub async fn discover(url: &str) -> Result<(Value, Vec<Iface>, Vec<String>), String> {
    let (status, card) = helpers::rest_get(url, "/.well-known/agent-card.json")
        .await
        .map_err(|e| format!("fetching the agent card from {url}: {e}"))?;
    if status != 200 {
        return Err(format!(
            "the agent card at {url}/.well-known/agent-card.json returned HTTP {status}"
        ));
    }

    let mut drivable = Vec::new();
    let mut skipped = Vec::new();
    for iface in card
        .get("supportedInterfaces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(declared) = iface.get("protocolBinding").and_then(Value::as_str) else {
            continue;
        };
        let Some(iface_url) = iface.get("url").and_then(Value::as_str) else {
            return Err(format!(
                "the card advertises a {declared} interface with no url — a client cannot reach it"
            ));
        };
        match binding_for(declared) {
            Some(binding) => drivable.push(Iface {
                binding: binding.to_owned(),
                url: iface_url.to_owned(),
                declared: declared.to_owned(),
            }),
            None => skipped.push(declared.to_owned()),
        }
    }
    Ok((card, drivable, skipped))
}

// ── BIND-EQUIV-001: identical functionality ──────────────────────────────────

/// One row of §5.3's method-mapping table.
struct Operation {
    /// The functionality name from §5.3, used in reports.
    name: &'static str,
    /// The JSON-RPC / gRPC method name (§5.3 gives them the same name).
    method: &'static str,
    /// `(HTTP verb, path template)` from §5.3, with `{id}` and `{configId}`
    /// substituted at probe time.
    rest: (&'static str, &'static str),
}

/// §5.3's method-mapping table in full. Probing a subset would let a binding
/// drop an operation the table lists and still pass.
const OPERATIONS: &[Operation] = &[
    Operation {
        name: "Send message",
        method: "SendMessage",
        rest: ("POST", "/message:send"),
    },
    Operation {
        name: "Stream message",
        method: "SendStreamingMessage",
        rest: ("POST", "/message:stream"),
    },
    Operation {
        name: "Get task",
        method: "GetTask",
        rest: ("GET", "/tasks/{id}"),
    },
    Operation {
        name: "List tasks",
        method: "ListTasks",
        rest: ("GET", "/tasks"),
    },
    Operation {
        name: "Cancel task",
        method: "CancelTask",
        rest: ("POST", "/tasks/{id}:cancel"),
    },
    Operation {
        name: "Subscribe to task",
        method: "SubscribeToTask",
        rest: ("POST", "/tasks/{id}:subscribe"),
    },
    Operation {
        name: "Create push notification config",
        method: "CreateTaskPushNotificationConfig",
        rest: ("POST", "/tasks/{id}/pushNotificationConfigs"),
    },
    Operation {
        name: "Get push notification config",
        method: "GetTaskPushNotificationConfig",
        rest: ("GET", "/tasks/{id}/pushNotificationConfigs/{configId}"),
    },
    Operation {
        name: "List push notification configs",
        method: "ListTaskPushNotificationConfigs",
        rest: ("GET", "/tasks/{id}/pushNotificationConfigs"),
    },
    Operation {
        name: "Delete push notification config",
        method: "DeleteTaskPushNotificationConfig",
        rest: ("DELETE", "/tasks/{id}/pushNotificationConfigs/{configId}"),
    },
    Operation {
        name: "Get extended Agent Card",
        method: "GetExtendedAgentCard",
        rest: ("GET", "/extendedAgentCard"),
    },
];

/// Whether a binding offers an operation at all.
///
/// The distinction that matters for §5.1 is *offered* versus *not offered* —
/// not whether a particular call succeeded. A `CancelTask` that answers "this
/// task is not cancelable" is offered; one that answers "no such method" is
/// not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Availability {
    Offered,
    NotOffered,
}

impl Availability {
    const fn label(self) -> &'static str {
        match self {
            Self::Offered => "offered",
            Self::NotOffered => "not offered",
        }
    }
}

/// Classifies a JSON-RPC error code from a **ghost-id** probe as "this method
/// does not exist here".
///
/// Only `-32601`, JSON-RPC 2.0's *Method not found*. §5.4 draws the line this
/// follows: `UNIMPLEMENTED` — and its JSON-RPC counterpart `-32601` — means
/// the method is not served, while `UnsupportedOperationError` (`-32004`,
/// gRPC `FAILED_PRECONDITION`, HTTP `400`) means the method *is* served and
/// the agent refused it.
///
/// `-32004` was on this list until 2026-08-30, and it made the three bindings
/// answer differently about the same agent. Against an agent built on the
/// official Python SDK that serves `GetExtendedAgentCard` on both bindings and
/// declines it on both — `-32004` on JSON-RPC, `400` with
/// `reason: UNSUPPORTED_OPERATION` on HTTP+JSON — this read "JSONRPC does not
/// offer it, HTTP+JSON does" and reported a `BIND-EQUIV-001` violation that
/// was not there. The REST arm below has always required `404`/`501`, and the
/// gRPC arm `UNIMPLEMENTED`; only the JSON-RPC arm treated a refusal as an
/// absence.
///
/// Two consequences worth stating rather than discovering:
///
/// * The state-refusal hazard the ghost id was introduced for is gone as a
///   side effect. This SDK answers `SubscribeToTask` on a completed task with
///   `-32004 … is in terminal state …`; that is a refusal, and a refusal is
///   no longer read as an absence whatever id it was probed with. The ghost
///   id is kept because it still separates "no such method" from "no such
///   task" on a binding whose error model hides the difference.
/// * An agent that signals genuine non-support with `-32004` rather than
///   `-32601` now reads as "offered". That is what §5.4 says those codes
///   mean, and grading it the other way is what produced the false positive.
const fn ghost_probe_says_unimplemented(code: i64) -> bool {
    code == -32601
}

/// Probes one operation on one binding.
///
/// The identifier a probe uses is chosen per binding, because the two error
/// models hide the answer in opposite directions:
///
/// - The envelope bindings and gRPC distinguish "no such method" from "no such
///   task" by *code*, so they probe with an id nothing owns. Nothing is
///   mutated, and no state-dependent refusal can be mistaken for absence.
/// - REST answers `404` to both, so it must probe with a **live** id: then a
///   `404` can only mean the route is absent. Its destructive route gets a
///   throwaway config created for the purpose, so a probe never removes state
///   a later check reads.
///
/// A probe that cannot be classified is an error, not a default. Treating an
/// unreachable binding as "offered" would let a target whose every probe
/// failed report perfect agreement — four bindings agreeing about nothing.
async fn probe(iface: &Iface, op: &Operation, fixture: &Fixture) -> Result<Availability, String> {
    let classified = match iface.binding.as_str() {
        "jsonrpc" | "websocket" => {
            let params = probe_params(op.method, UNKNOWN_TASK_ID, UNKNOWN_CONFIG_ID);
            match helpers::rpc_probe(&iface.url, &iface.binding, op.method, params).await {
                Err(e) => return Err(e),
                Ok(Some(code)) if ghost_probe_says_unimplemented(code) => Availability::NotOffered,
                Ok(_) => Availability::Offered,
            }
        }
        "rest" => {
            let (verb, template) = op.rest;
            // GET on the live config, DELETE on the throwaway: both resolve
            // the `{configId}` route segment with something that exists, so a
            // 404 is unambiguous, and only the throwaway is destroyed.
            let config = if verb == "DELETE" {
                &fixture.delete_config_id
            } else {
                &fixture.config_id
            };
            let path = template
                .replace("{id}", &fixture.task_id)
                .replace("{configId}", config);
            match helpers::http_status(&iface.url, verb, &path).await {
                Err(e) => return Err(e),
                // 404 with a live id means the route is absent; 501 is the
                // explicit "not implemented".
                Ok(404 | 501) => Availability::NotOffered,
                Ok(_) => Availability::Offered,
            }
        }
        "grpc" => {
            let unimplemented = crate::tests::grpc::probe_method(
                &iface.url,
                op.method,
                UNKNOWN_TASK_ID,
                UNKNOWN_CONFIG_ID,
            )
            .await?;
            if unimplemented {
                Availability::NotOffered
            } else {
                Availability::Offered
            }
        }
        other => return Err(format!("cannot drive binding {other}")),
    };
    Ok(classified)
}

/// Minimal params for an availability probe — enough to reach the method's
/// dispatch, not enough to care whether it succeeds.
fn probe_params(method: &str, task_id: &str, config_id: &str) -> Value {
    match method {
        "SendMessage" | "SendStreamingMessage" => {
            helpers::make_send_params("TCK: equivalence probe")
        }
        "GetTask" | "CancelTask" | "SubscribeToTask" => serde_json::json!({ "id": task_id }),
        "ListTasks" => serde_json::json!({}),
        "CreateTaskPushNotificationConfig" => serde_json::json!({
            "taskId": task_id,
            "url": "https://example.com/webhook"
        }),
        "GetTaskPushNotificationConfig" | "DeleteTaskPushNotificationConfig" => {
            serde_json::json!({ "taskId": task_id, "id": config_id })
        }
        "ListTaskPushNotificationConfigs" => serde_json::json!({ "taskId": task_id }),
        _ => serde_json::json!({}),
    }
}

async fn bind_equiv_001(ifaces: &[Iface], fixture: &Fixture) -> Result<(), String> {
    let mut table: BTreeMap<&str, Vec<(String, Availability)>> = BTreeMap::new();
    for op in OPERATIONS {
        for iface in ifaces {
            let got = probe(iface, op, fixture).await.map_err(|e| {
                format!(
                    "probing '{}' over {}: {e} — an unclassifiable probe leaves the \
                     comparison incomplete, so this is reported rather than assumed",
                    op.name, iface.declared
                )
            })?;
            table
                .entry(op.name)
                .or_default()
                .push((iface.declared.clone(), got));
        }
    }

    // A comparison in which nothing is offered anywhere agrees perfectly and
    // proves nothing. Any real A2A agent offers `SendMessage`; a table with no
    // `Offered` at all means the probes never reached the target.
    if !table
        .values()
        .flatten()
        .any(|(_, a)| *a == Availability::Offered)
    {
        return Err(format!(
            "no binding offered any of the {} operations in §5.3's table. Every probe \
             agreeing on 'absent' is not equivalence, it is a target that was never \
             reached.",
            OPERATIONS.len()
        ));
    }

    let mut mismatches = Vec::new();
    for (op_name, row) in &table {
        let first = row[0].1;
        if row.iter().any(|(_, a)| *a != first) {
            let detail = row
                .iter()
                .map(|(b, a)| format!("{b}={}", a.label()))
                .collect::<Vec<_>>()
                .join(", ");
            mismatches.push(format!("{op_name}: {detail}"));
        }
    }

    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "§5.1 requires every binding to offer the same set of operations; \
             {} differ — {}",
            mismatches.len(),
            mismatches.join(" | ")
        ))
    }
}

// ── BIND-EQUIV-002: semantically equivalent results ──────────────────────────

/// The semantic content of a task, independent of how a binding encodes it.
///
/// Deliberately excludes anything a binding may legitimately render
/// differently — timestamp precision, field ordering, absent-versus-empty
/// collections — and keeps what §5.1 means by "the same result": the identity
/// of the task, its lifecycle state, and the content it produced.
#[derive(Debug, PartialEq, Eq)]
pub struct TaskView {
    pub id: String,
    pub context_id: String,
    pub state: String,
    /// Text parts of every artifact, in order.
    pub artifact_text: Vec<String>,
}

/// Reads a task through one binding and reduces it to its semantic content.
async fn task_view(iface: &Iface, task_id: &str) -> Result<TaskView, String> {
    if iface.binding == "grpc" {
        return crate::tests::grpc::task_view(&iface.url, task_id).await;
    }
    let raw = helpers::get_task(&iface.url, &iface.binding, task_id).await?;
    Ok(TaskView {
        id: raw
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        context_id: raw
            .get("contextId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        state: raw
            .pointer("/status/state")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        artifact_text: raw
            .get("artifacts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|a| {
                a.get("parts")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect(),
    })
}

/// One task, created once, read back through every binding.
///
/// Reading the *same resource* is what makes this a comparison rather than
/// four independent smoke tests: differing task ids would explain away any
/// difference, so the check would have nothing to assert.
async fn bind_equiv_002(ifaces: &[Iface], fixture: &Fixture) -> Result<(), String> {
    let mut views = Vec::new();
    for iface in ifaces {
        let view = task_view(iface, &fixture.task_id).await.map_err(|e| {
            format!(
                "reading task {} over {}: {e}",
                fixture.task_id, iface.declared
            )
        })?;
        views.push((iface.declared.clone(), view));
    }

    // Two empty views compare equal. If a binding's reader silently produced
    // nothing — a moved field, a shape this kit does not recognise — every
    // view would be blank and the check would pass on four readings of
    // nothing. Require the content that makes the comparison meaningful.
    for (binding, view) in &views {
        if view.id.is_empty() || view.state.is_empty() {
            return Err(format!(
                "{binding} produced a degenerate view of task {} ({view:?}). Empty views \
                 compare equal, so this would pass without comparing anything.",
                fixture.task_id
            ));
        }
        if view.id != fixture.task_id {
            return Err(format!(
                "{binding} returned task '{}' when asked for '{}' — the bindings are not \
                 reading the same resource, so nothing they agree on would mean anything",
                view.id, fixture.task_id
            ));
        }
    }

    let (first_binding, first) = &views[0];
    let mut diffs = Vec::new();
    for (binding, view) in &views[1..] {
        if view != first {
            diffs.push(format!(
                "{first_binding} saw {first:?} but {binding} saw {view:?}"
            ));
        }
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "§5.1 requires semantically equivalent results for the same request; \
             the same task reads differently per binding — {}",
            diffs.join(" | ")
        ))
    }
}

// ── BIND-EQUIV-003: consistent error mapping ─────────────────────────────────

/// A fault whose per-binding representation §5.4 fixes.
struct Fault {
    /// The A2A error type from §5.4's table.
    error_type: &'static str,
    /// The JSON-RPC code §5.4 assigns. The §12 WebSocket binding carries the
    /// same envelope, so §5.4's "custom bindings MUST define equivalent error
    /// code mappings that preserve the semantic meaning" resolves to the same
    /// code there.
    jsonrpc: i64,
    /// The HTTP status §5.4 assigns.
    http: u16,
    /// The gRPC status name §5.4 assigns, and its numeric code.
    grpc: (&'static str, i32),
    /// Printed when [`Fault::triggerable`] says this target cannot produce
    /// the fault, so a skipped comparison explains itself.
    why_untriggerable: &'static str,
}

const FAULTS: &[Fault] = &[
    Fault {
        error_type: "TaskNotFoundError",
        jsonrpc: -32001,
        http: 404,
        grpc: ("NOT_FOUND", 5),
        why_untriggerable: "an id nothing owns is always available",
    },
    Fault {
        error_type: "TaskNotCancelableError",
        jsonrpc: -32002,
        // 400, not 409. Graded against 409 until 2026-08-30, which failed a
        // conformant agent built on the official Python SDK — the kit and
        // this repository's SDK were reading the same stale copy of §5.4, so
        // they agreed with each other and with nobody else.
        http: 400,
        grpc: ("FAILED_PRECONDITION", 9),
        why_untriggerable: "the fixture task never reached a terminal state, so \
                            cancelling it is a legitimate success on every binding",
    },
];

impl Fault {
    /// Whether this target can be made to produce the fault at all.
    ///
    /// `TaskNotFoundError` always can — an id nothing owns is free. The
    /// cancellation fault needs a task that has already finished, which
    /// depends on the agent under test, not on the binding.
    fn triggerable(&self, fixture: &Fixture) -> bool {
        match self.error_type {
            "TaskNotCancelableError" => fixture.terminal_task_id.is_some(),
            _ => true,
        }
    }
}

/// What one binding answered, rendered for comparison against §5.4.
struct Answer {
    binding: String,
    got: String,
    expected: String,
    matches: bool,
}

async fn fault_answer(iface: &Iface, fault: &Fault, fixture: &Fixture) -> Result<Answer, String> {
    // The two faults need different triggers: an id no task has, and a task
    // that has already reached a terminal state.
    let target = match fault.error_type {
        "TaskNotFoundError" => UNKNOWN_TASK_ID.to_owned(),
        "TaskNotCancelableError" => fixture
            .terminal_task_id
            .clone()
            .ok_or("no terminal task; bind_equiv_003 should have skipped this fault")?,
        other => return Err(format!("no trigger defined for {other}")),
    };
    let method = match fault.error_type {
        "TaskNotFoundError" => "GetTask",
        _ => "CancelTask",
    };

    match iface.binding.as_str() {
        "jsonrpc" | "websocket" => {
            let resp = helpers::rpc(
                &iface.url,
                &iface.binding,
                method,
                serde_json::json!({ "id": target }),
            )
            .await?;
            let code = resp
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(Value::as_i64);
            Ok(Answer {
                binding: iface.declared.clone(),
                got: code.map_or_else(|| "no error at all".to_owned(), |c| c.to_string()),
                expected: fault.jsonrpc.to_string(),
                matches: code == Some(fault.jsonrpc),
            })
        }
        "rest" => {
            let (verb, path) = if method == "GetTask" {
                ("GET", format!("/tasks/{target}"))
            } else {
                ("POST", format!("/tasks/{target}:cancel"))
            };
            let status = helpers::http_status(&iface.url, verb, &path).await?;
            Ok(Answer {
                binding: iface.declared.clone(),
                got: format!("HTTP {status}"),
                expected: format!("HTTP {}", fault.http),
                matches: status == fault.http,
            })
        }
        "grpc" => {
            let (name, code) = crate::tests::grpc::fault_code(&iface.url, method, &target).await?;
            Ok(Answer {
                binding: iface.declared.clone(),
                got: format!("{name} ({code})"),
                expected: format!("{} ({})", fault.grpc.0, fault.grpc.1),
                matches: code == fault.grpc.1,
            })
        }
        other => Err(format!("cannot drive binding {other}")),
    }
}

/// The unknown-task id, fixed so a failure message is reproducible by hand.
const UNKNOWN_TASK_ID: &str = "tck-equivalence-no-such-task-6b1f2d90";

/// The unknown-config id, used the same way.
const UNKNOWN_CONFIG_ID: &str = "tck-equivalence-no-such-config-6b1f2d90";

async fn bind_equiv_003(ifaces: &[Iface], fixture: &Fixture) -> Result<(), String> {
    let mut wrong = Vec::new();
    let mut graded = 0usize;
    for fault in FAULTS {
        // A fault this target cannot be made to produce says nothing about
        // how it maps errors. `TaskNotCancelableError` needs a task already
        // in a terminal state; against an agent whose work is still running,
        // `CancelTask` succeeds, and grading that as "expected -32002, got no
        // error at all" would blame the binding for the harness's inability
        // to set up the fault.
        if !fault.triggerable(fixture) {
            println!(
                "         (skipping {}: {} — nothing to compare, so nothing is claimed)",
                fault.error_type, fault.why_untriggerable
            );
            continue;
        }
        graded += 1;
        for iface in ifaces {
            let answer = fault_answer(iface, fault, fixture).await.map_err(|e| {
                format!(
                    "triggering {} over {}: {e}",
                    fault.error_type, iface.declared
                )
            })?;
            if !answer.matches {
                wrong.push(format!(
                    "{} over {}: expected {}, got {}",
                    fault.error_type, answer.binding, answer.expected, answer.got
                ));
            }
        }
    }

    // Skipping every fault would leave a check that compared nothing and
    // passed — the same vacuity the other two guard against.
    if graded == 0 {
        return Err(format!(
            "none of the {} faults in §5.4's table could be triggered against this \
             target, so nothing about its error mapping was compared",
            FAULTS.len()
        ));
    }

    if wrong.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "§5.4 fixes each error type's representation per binding; {} answer differently — {}",
            wrong.len(),
            wrong.join(" | ")
        ))
    }
}

// ── BIND-EQUIV-004: same authentication schemes ──────────────────────────────

/// §5.1's authentication clause, checked at the level the card can express.
///
/// The `AgentCard` declares `securitySchemes` and `securityRequirements` once,
/// at card level; `AgentInterface` (§5.2) has no security fields at all. So
/// the schemes are shared by construction *unless* an agent publishes
/// per-interface security, which the v1.0 schema does not define — and which
/// is exactly the shape a non-conforming agent would invent to give one
/// binding weaker auth than another.
///
/// **The structural half.** Proving each binding *enforces* the declared
/// schemes identically is [`bind_equiv_004_enforcement`], which needs a target
/// that actually requires credentials. Against a card declaring none, that
/// probe would be a check that cannot fail, so it is not run and the report
/// says the grade is structural. What is verifiable here is verified; what is
/// not is named.
fn bind_equiv_004(card: &Value, ifaces: &[Iface]) -> Result<(), String> {
    let mut offenders = Vec::new();
    for iface in card
        .get("supportedInterfaces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let declared = iface
            .get("protocolBinding")
            .and_then(Value::as_str)
            .unwrap_or("<unnamed>");
        for field in ["securitySchemes", "securityRequirements", "security"] {
            if iface.get(field).is_some() {
                offenders.push(format!("{declared} carries its own '{field}'"));
            }
        }
    }

    if !offenders.is_empty() {
        return Err(format!(
            "§5.1 requires every binding to support the same declared authentication \
             schemes, and v1.0 has no per-interface security field — an interface that \
             carries one is declaring a binding-specific scheme: {}",
            offenders.join(", ")
        ));
    }

    // A card that declares requirements but no schemes to satisfy them names
    // credentials no client can construct — unusable identically on every
    // binding, which is not the equivalence §5.1 is asking for.
    let has_requirements = card
        .get("securityRequirements")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty());
    let has_schemes = card
        .get("securitySchemes")
        .and_then(Value::as_object)
        .is_some_and(|m| !m.is_empty());
    if has_requirements && !has_schemes {
        return Err(
            "the card lists securityRequirements but declares no securitySchemes, so no \
             binding can satisfy them"
                .to_string(),
        );
    }

    let _ = ifaces;
    Ok(())
}

// ── BIND-EQUIV-004, enforcement half ─────────────────────────────────────────

/// Whether a binding served a request or refused it.
///
/// Deliberately two-valued. *Which* error a binding returns is
/// `BIND-EQUIV-003`'s subject, and this check duplicating that comparison
/// would report one divergence as two failures. What is asked here is the
/// question `BIND-EQUIV-003` cannot ask: did the binding apply the card's
/// declared security at all?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthOutcome {
    Served,
    Refused,
}

/// Issues one read through `iface`, with `token` if given, and reports whether
/// it was served.
///
/// `ListTasks` is the operation because it needs no fixture and every binding
/// implements it — the probe must not be able to fail for want of a task. The
/// name is §5.3's, matching [`OPERATIONS`]; an earlier draft of this probe sent
/// the JSON-RPC method as `tasks/list`, which authenticated correctly and then
/// failed method dispatch. Both JSON-RPC-family bindings reported `Refused` and
/// the check declared a binding asymmetry that did not exist. The acceptance
/// sweep is what caught it — on the rejection sweep alone, a probe that can
/// never succeed looks exactly like enforcement working.
///
/// A transport-level error is *not* reported as `Refused`: a connection that
/// could not be made says nothing about authentication, and folding it in
/// would let a broken listener read as correct enforcement. Those return
/// `Err`, which fails the check loudly.
async fn auth_probe(iface: &Iface, token: Option<&str>) -> Result<AuthOutcome, String> {
    match iface.binding.as_str() {
        "grpc" => crate::tests::grpc::list_tasks_auth_probe(&iface.url, token)
            .await
            .map(|served| {
                if served {
                    AuthOutcome::Served
                } else {
                    AuthOutcome::Refused
                }
            }),
        "websocket" => {
            let resp = helpers::ws_jsonrpc_request_with_auth(
                &iface.url,
                "ListTasks",
                serde_json::json!({}),
                token,
            )
            .await?;
            Ok(jsonrpc_outcome(&resp))
        }
        "rest" => {
            let (status, _body) =
                helpers::http_get_with_auth(&format!("{}/tasks", iface.url), token).await?;
            Ok(if (200..300).contains(&status) {
                AuthOutcome::Served
            } else {
                AuthOutcome::Refused
            })
        }
        // jsonrpc, and anything else this kit drives over the JSON-RPC envelope.
        _ => {
            let resp = helpers::jsonrpc_call_with_auth(
                &iface.url,
                "ListTasks",
                serde_json::json!({}),
                token,
            )
            .await?;
            Ok(jsonrpc_outcome(&resp))
        }
    }
}

fn jsonrpc_outcome(resp: &Value) -> AuthOutcome {
    if resp.get("error").is_some() {
        AuthOutcome::Refused
    } else {
        AuthOutcome::Served
    }
}

/// §5.1's authentication clause, checked behaviourally.
///
/// Two sweeps, and both matter:
///
/// * **Without credentials, every binding must refuse.** One binding that
///   serves an uncredentialed caller while the others refuse is precisely the
///   asymmetry §5.1 forbids, and it is the realistic defect — a transport that
///   forgets to populate the header its authenticator reads looks completely
///   normal until someone tries it.
/// * **With credentials, every binding must serve.** Without this half the
///   check passes trivially against a server that is simply broken, which is
///   the failure mode this repository keeps finding in its own gates.
///
/// The token is supplied by the caller (`--auth-token`) rather than read from
/// the target, because a kit that could derive the credential from the agent
/// it grades would not be testing anything. When no token is given, the second
/// sweep cannot run; the first still can, and the report says which half was
/// graded rather than implying both.
async fn bind_equiv_004_enforcement(
    ifaces: &[Iface],
    token: Option<&str>,
) -> Result<String, String> {
    let mut served_without = Vec::new();
    for iface in ifaces {
        if auth_probe(iface, None).await? == AuthOutcome::Served {
            served_without.push(iface.declared.clone());
        }
    }
    if !served_without.is_empty() {
        return Err(format!(
            "the card declares securityRequirements, so every binding must apply them, but {} \
             served an uncredentialed request that the other {} refused — that is a \
             binding-specific authentication posture, which is what §5.1 forbids",
            served_without.join(", "),
            ifaces.len() - served_without.len()
        ));
    }

    let Some(token) = token else {
        return Ok(format!(
            "enforcement graded on the rejection half only: all {} bindings refused an \
             uncredentialed request. The acceptance half needs --auth-token",
            ifaces.len()
        ));
    };

    let mut refused_with = Vec::new();
    for iface in ifaces {
        if auth_probe(iface, Some(token)).await? == AuthOutcome::Refused {
            refused_with.push(iface.declared.clone());
        }
    }
    if !refused_with.is_empty() {
        return Err(format!(
            "with the supplied credential {} still refused the request while the other {} \
             served it — the bindings do not accept the card's declared scheme identically \
             (if the credential is simply wrong, every binding would appear here, and none \
             of this would be about equivalence)",
            refused_with.join(", "),
            ifaces.len() - refused_with.len()
        ));
    }

    Ok(format!(
        "enforcement graded on both halves: all {} bindings refused an uncredentialed request \
         and served a credentialed one",
        ifaces.len()
    ))
}

// ── Fixture ──────────────────────────────────────────────────────────────────

/// State every check shares: one live task, a push config to read, a second
/// one the destructive probe may consume, and a task known to be terminal.
struct Fixture {
    task_id: String,
    config_id: String,
    /// Consumed by the REST `DELETE` availability probe, so that probe never
    /// removes the config the read probes depend on.
    delete_config_id: String,
    /// A task observed to have reached a terminal state, if one did.
    ///
    /// `Option`, not `String`, because whether a task finishes is a property
    /// of the agent under test. This was previously set to `task_id`
    /// unconditionally and documented as "a task known to be terminal" — an
    /// assumption, not an observation. Against an agent still working, the
    /// cancel would legitimately succeed and `BIND-EQUIV-003` would report a
    /// §5.4 violation that was really a harness limitation.
    terminal_task_id: Option<String>,
}

/// Builds the fixture over the first advertised binding.
///
/// Deliberately one binding rather than each in turn: the point of the checks
/// is to compare what the *others* say about the same resource, so it has to
/// be created exactly once.
async fn build_fixture(iface: &Iface) -> Result<Fixture, String> {
    let created = helpers::send_message(
        &iface.url,
        &iface.binding,
        helpers::make_send_params("TCK: equivalence fixture"),
    )
    .await
    .map_err(|e| format!("creating the fixture task over {}: {e}", iface.declared))?;
    let task = helpers::extract_task(&created)?;
    let task_id = task
        .get("id")
        .and_then(Value::as_str)
        .ok_or("fixture task has no id")?
        .to_owned();

    let config = |label: &'static str| {
        let url = iface.url.clone();
        let binding = iface.binding.clone();
        let task_id = task_id.clone();
        async move {
            let resp = helpers::rpc(
                &url,
                &binding,
                "CreateTaskPushNotificationConfig",
                serde_json::json!({ "taskId": task_id, "url": "https://example.com/webhook" }),
            )
            .await
            .map_err(|e| format!("creating the {label} push config: {e}"))?;
            // A target that does not support push configs still needs a config
            // id to build probe paths from. The placeholder keeps the probe
            // honest: the operation then reads as not offered on every binding
            // alike, which is agreement, not a fabricated mismatch.
            Ok::<String, String>(
                resp.pointer("/result/id")
                    .and_then(Value::as_str)
                    .unwrap_or(UNKNOWN_CONFIG_ID)
                    .to_owned(),
            )
        }
    };
    let config_id = config("readable").await?;
    let delete_config_id = config("disposable").await?;

    let terminal_task_id = await_terminal(iface, &task_id).await;

    Ok(Fixture {
        terminal_task_id,
        task_id,
        config_id,
        delete_config_id,
    })
}

/// Polls a task briefly and returns its id once it reaches a terminal state.
///
/// Bounded and best-effort: an agent that takes longer than this, or that
/// leaves the task open awaiting input, simply yields `None`, and the
/// cancellation fault is skipped with its reason printed rather than graded
/// against a task that was never cancellable-by-refusal in the first place.
async fn await_terminal(iface: &Iface, task_id: &str) -> Option<String> {
    const TERMINAL: [&str; 4] = [
        "TASK_STATE_COMPLETED",
        "TASK_STATE_FAILED",
        "TASK_STATE_CANCELED",
        "TASK_STATE_REJECTED",
    ];
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if let Ok(view) = task_view(iface, task_id).await {
            if TERMINAL.contains(&view.state.as_str()) {
                return Some(task_id.to_owned());
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    None
}

// ── Entry point ──────────────────────────────────────────────────────────────

/// Runs the four §5.1 checks and returns their results.
///
/// Errors (rather than returning results) when the target cannot support the
/// comparison at all: one binding compares with nothing, and a run that grades
/// no requirement must not exit green.
pub async fn run_equivalence(
    url: &str,
    auth_token: Option<&str>,
) -> Result<Vec<TestResult>, String> {
    let (card, ifaces, unsupported) = discover(url).await?;

    if !unsupported.is_empty() {
        println!(
            "Note: the card advertises {} this kit cannot drive; they are excluded \
             from the comparison, so a mismatch involving them would not be seen.",
            unsupported.join(", ")
        );
        println!();
    }

    if ifaces.len() < 2 {
        return Err(format!(
            "§5.1 is a requirement about the relation between bindings, and this target \
             advertises {} this kit can drive{}. There is nothing to compare, and \
             reporting a pass would mean nothing.",
            ifaces.len(),
            if unsupported.is_empty() {
                String::new()
            } else {
                format!(" (skipping {})", unsupported.join(", "))
            }
        ));
    }

    println!(
        "Comparing {} bindings: {}",
        ifaces.len(),
        ifaces
            .iter()
            .map(|i| format!("{} @ {}", i.declared, i.url))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();

    // Does this target actually require credentials? The answer decides which
    // of two runs this is, because the two cannot be the same run.
    //
    // `BIND-EQUIV-001..003` all compare answers about a *fixture* — a task and
    // push configs created up front over the first binding. Against a secured
    // target the kit cannot create one: every request it makes is anonymous by
    // design, and the alternative is to thread a credential through the forty
    // call sites those three checks share, so that one check can use it.
    //
    // So a secured run grades `BIND-EQUIV-004` and nothing else, and says so.
    // That is scoping rather than waiving, and the same argument as the
    // official suite's extension profile: the three checks skipped here are
    // graded in full by the ordinary unsecured run, which is the one CI gates
    // on. Neither run alone covers §5.1; the pair does.
    let requires_credentials = card
        .get("securityRequirements")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty())
        && card
            .get("securitySchemes")
            .and_then(Value::as_object)
            .is_some_and(|m| !m.is_empty());

    let mut results = Vec::new();

    if requires_credentials {
        println!(
            "This target's card declares securityRequirements, so this run grades \
             BIND-EQUIV-004 only. BIND-EQUIV-001..003 compare answers about a fixture \
             task that an anonymous client cannot create here; they are graded by the \
             ordinary unsecured equivalence run."
        );
        println!();
    } else {
        let fixture = build_fixture(&ifaces[0]).await?;
        record(
            &mut results,
            "bind_equiv_001_identical_functionality",
            bind_equiv_001(&ifaces, &fixture).await,
        );
        record(
            &mut results,
            "bind_equiv_002_equivalent_results",
            bind_equiv_002(&ifaces, &fixture).await,
        );
        record(
            &mut results,
            "bind_equiv_003_consistent_error_mapping",
            bind_equiv_003(&ifaces, &fixture).await,
        );
    }

    // The structural half always runs — it is the one a card alone can answer.
    let structural = bind_equiv_004(&card, &ifaces);
    let structural_ok = structural.is_ok();
    record(
        &mut results,
        "bind_equiv_004_shared_auth_schemes",
        structural,
    );

    // The enforcement half runs only against a target that actually requires
    // credentials. Against one that does not, every binding would serve every
    // request and the probe could not fail — the precise shape of decorative
    // check this kit refuses to report as a pass.
    let enforcement_note = if !requires_credentials {
        None
    } else if structural_ok {
        let outcome = bind_equiv_004_enforcement(&ifaces, auth_token).await;
        let note = match &outcome {
            Ok(summary) => summary.clone(),
            Err(_) => "enforcement graded and FAILED — see the check output above".to_string(),
        };
        record(
            &mut results,
            "bind_equiv_004_enforced_identically",
            outcome.map(|_| ()),
        );
        Some(note)
    } else {
        // Declaring per-interface security already failed the structural half.
        // Probing enforcement on top would report the same defect twice and
        // bury which one is the cause.
        Some("enforcement not probed: the structural half failed first".to_string())
    };

    println!();
    match enforcement_note {
        Some(note) => println!("BIND-EQUIV-004: {note}."),
        None => println!(
            "BIND-EQUIV-004 is graded structurally only: the card declares security once, \
             at card level, and no interface may override it. Whether each binding \
             *enforces* those schemes identically needs a target configured to require \
             credentials, which this run does not have — the card declares no \
             securityRequirements. Run against `SUT_PROFILE=secured` with --auth-token \
             to grade that half."
        ),
    }

    Ok(results)
}

fn record(results: &mut Vec<TestResult>, name: &str, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => {
            println!("  [PASS] {name}");
            results.push(TestResult::pass(name));
        }
        Err(msg) => {
            println!("  [FAIL] {name}");
            results.push(TestResult::fail(name, msg));
        }
    }
    debug_assert!(matches!(
        results.last().map(|r| r.outcome),
        Some(Outcome::Pass | Outcome::Fail)
    ));
}

#[cfg(test)]
mod tests {
    use super::{bind_equiv_004, binding_for, Iface, OPERATIONS};

    #[test]
    fn canonical_and_legacy_binding_names_both_resolve() {
        assert_eq!(binding_for("JSONRPC"), Some("jsonrpc"));
        assert_eq!(binding_for("HTTP+JSON"), Some("rest"));
        assert_eq!(binding_for("REST"), Some("rest"));
        assert_eq!(binding_for("GRPC"), Some("grpc"));
        assert_eq!(binding_for("WEBSOCKET"), Some("websocket"));
        // Case is not significant in the card's spelling.
        assert_eq!(binding_for("jsonrpc"), Some("jsonrpc"));
    }

    /// §12 permits custom bindings. A kit that cannot drive one must report
    /// the gap, not fail the agent for having it — so the mapper returns
    /// `None` rather than erroring.
    #[test]
    fn an_unknown_binding_is_skipped_not_rejected() {
        assert_eq!(binding_for("CARRIER-PIGEON"), None);
    }

    /// §5.3's table has eleven rows. A probe list that silently loses one
    /// would let a binding drop that operation and still pass 001.
    #[test]
    fn every_operation_in_the_method_mapping_table_is_probed() {
        assert_eq!(
            OPERATIONS.len(),
            11,
            "§5.3 lists 11 operations; the probe list must cover all of them"
        );
        for op in OPERATIONS {
            assert!(!op.method.is_empty(), "{} has no method name", op.name);
            assert!(
                op.rest.1.starts_with('/'),
                "{}: REST path must be absolute, got {:?}",
                op.name,
                op.rest.1
            );
        }
    }

    fn iface() -> Vec<Iface> {
        vec![Iface {
            binding: "jsonrpc".into(),
            url: "http://x".into(),
            declared: "JSONRPC".into(),
        }]
    }

    #[test]
    fn card_level_security_passes_004() {
        let card = serde_json::json!({
            "supportedInterfaces": [{"protocolBinding": "JSONRPC", "url": "http://x"}],
            "securitySchemes": {"bearer": {"type": "http"}},
            "securityRequirements": [{"bearer": []}]
        });
        assert!(bind_equiv_004(&card, &iface()).is_ok());
    }

    #[test]
    fn per_interface_security_fails_004() {
        let card = serde_json::json!({
            "supportedInterfaces": [
                {"protocolBinding": "JSONRPC", "url": "http://x"},
                {"protocolBinding": "GRPC", "url": "x:1", "securitySchemes": {}}
            ]
        });
        let err = bind_equiv_004(&card, &iface()).expect_err("per-interface security must fail");
        assert!(err.contains("GRPC"), "{err}");
    }

    #[test]
    fn requirements_without_schemes_fail_004() {
        let card = serde_json::json!({
            "supportedInterfaces": [{"protocolBinding": "JSONRPC", "url": "http://x"}],
            "securityRequirements": [{"bearer": []}]
        });
        let err = bind_equiv_004(&card, &iface())
            .expect_err("requirements naming no scheme cannot be satisfied on any binding");
        assert!(err.contains("securitySchemes"), "{err}");
    }

    #[test]
    fn a_card_with_no_security_at_all_passes_004() {
        let card = serde_json::json!({
            "supportedInterfaces": [{"protocolBinding": "JSONRPC", "url": "http://x"}]
        });
        assert!(bind_equiv_004(&card, &iface()).is_ok());
    }
}
