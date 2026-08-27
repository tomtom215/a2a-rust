// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the shared helpers, and for the bundled data they read.
//!
//! These are small functions, but three of the demo's five acts turn on them:
//! `find_service` returning `None` is what parks a task in `INPUT_REQUIRED`
//! (Act 1), `known_services` is what the resulting question offers the
//! operator, and `extract_text` is how every agent reads its input. A silent
//! change in any of them degrades the demo into something that still runs and
//! no longer demonstrates what the README says it does — which is the failure
//! mode an example has, rather than crashing.

use a2a_protocol_types::message::{MessageRole, Part, PartContent};

use crate::{
    agent_message, extract_text, find_service, known_services, user_message, INCIDENT_LOG,
    RUNBOOKS, SURFACE_PAUSE_PREFIX,
};

// ── The bundled data ─────────────────────────────────────────────────────────

#[test]
fn known_services_are_exactly_the_runbook_headings() {
    let headings: Vec<&str> = RUNBOOKS
        .lines()
        .filter(|l| l.starts_with("## "))
        .map(|l| l[3..].trim())
        .collect();
    assert_eq!(known_services(), headings);
    assert!(
        !headings.is_empty(),
        "an empty runbook file would leave every alert parked in INPUT_REQUIRED \
         forever, with the demo still exiting 0"
    );
}

#[test]
fn every_known_service_has_log_lines_to_find() {
    // The logs agent is the demo's evidence. A service with a runbook and no
    // log lines produces an incident report whose Evidence section is the
    // string "0 lines" — which reads as a working demo.
    for service in known_services() {
        let hits = INCIDENT_LOG.lines().filter(|l| l.contains(service)).count();
        assert!(hits > 0, "no log lines mention '{service}'");
    }
}

#[test]
fn every_known_service_has_a_non_empty_runbook_section() {
    // RunbookExecutor slices between `## <service>` and the next `## `. A
    // heading with nothing under it would emit an empty artifact rather than
    // fail, so nothing else would notice.
    for service in known_services() {
        let marker = format!("## {service}");
        let start = RUNBOOKS.find(&marker).expect("heading came from this file");
        let rest = &RUNBOOKS[start + marker.len()..];
        let end = rest.find("\n## ").unwrap_or(rest.len());
        assert!(
            !rest[..end].trim().is_empty(),
            "runbook section for '{service}' is empty"
        );
    }
}

// ── find_service ─────────────────────────────────────────────────────────────

#[test]
fn find_service_is_case_insensitive() {
    // Alerts arrive from humans and from monitoring systems that shout.
    let service = known_services()[0];
    assert_eq!(
        find_service(&format!("{} IS DOWN", service.to_uppercase())),
        Some(service)
    );
}

#[test]
fn find_service_returns_none_when_no_service_is_named() {
    // This `None` is Act 1: it is the only thing that parks a triage task in
    // INPUT_REQUIRED, which is the property the whole example exists to show.
    assert_eq!(find_service("everything is on fire"), None);
    assert_eq!(find_service(""), None);
}

#[test]
fn find_service_returns_the_first_known_service_mentioned() {
    let services = known_services();
    assert!(services.len() >= 2, "this test needs two services to order");
    let text = format!("{} and {} both look bad", services[1], services[0]);
    // Precedence is runbook order, not order of appearance in the text.
    assert_eq!(find_service(&text), Some(services[0]));
}

#[test]
fn the_surface_sweeps_pause_prefix_names_no_known_service() {
    // Act 4 reuses this prefix to obtain a *non-terminal* task for
    // SubscribeToTask instead of inventing a sleep. If a service were ever
    // added whose name appears in this string, the task would complete
    // instead of parking, and Act 4's "exits non-zero if any cell never ran"
    // guarantee would quietly weaken rather than fail.
    assert_eq!(
        find_service(SURFACE_PAUSE_PREFIX),
        None,
        "SURFACE_PAUSE_PREFIX now names a known service"
    );
}

// ── extract_text ─────────────────────────────────────────────────────────────

#[test]
fn extract_text_joins_text_parts_and_ignores_the_rest() {
    let parts = vec![
        Part::text("payments-api"),
        Part {
            content: PartContent::Data(serde_json::json!({"severity": "page"})),
            ..Part::text("")
        },
        Part::text("latency spike"),
    ];
    // The non-text part contributes nothing at all — not an empty string that
    // would show up as a doubled separator.
    assert_eq!(extract_text(&parts), "payments-api latency spike");
}

#[test]
fn extract_text_of_a_textless_message_is_empty() {
    // An empty string finds no service, so a textless message parks or fails
    // rather than being silently treated as a valid alert.
    let parts = vec![Part {
        content: PartContent::Url("https://example.invalid/x".to_string()),
        ..Part::text("")
    }];
    assert_eq!(extract_text(&parts), "");
    assert_eq!(extract_text(&[]), "");
}

// ── Message builders ─────────────────────────────────────────────────────────

#[test]
fn message_builders_set_the_role_the_protocol_expects() {
    let agent = agent_message("from the agent");
    assert_eq!(agent.role, MessageRole::Agent);
    assert_eq!(extract_text(&agent.parts), "from the agent");

    let user = user_message("from the operator");
    assert_eq!(user.role, MessageRole::User);
    assert_eq!(extract_text(&user.parts), "from the operator");

    // Distinct ids: two messages in one context must not collide.
    assert_ne!(agent_message("a").id, agent_message("a").id);
}
