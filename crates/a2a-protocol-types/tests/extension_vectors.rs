// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The published conformance vectors of every extension this SDK defines
//! (`extensions/<name>/`, ADR 0049), run through the SDK's own readers: each
//! valid value reads as the vector says and survives a write and a second
//! read unchanged, each invalid value is refused, and each tolerated value
//! (one a receiver accepts but a sender must not write) reads as stated.
//! The file names the URI and metadata key it is for; both are checked
//! against the constants. Lineage's vectors run in `lineage::vectors_tests`,
//! and the hand-off marker's in the server crate's claim stores, its readers.

use a2a_protocol_types::Message;
use a2a_protocol_types::approval::{
    APPROVAL_EXTENSION_URI, APPROVAL_METADATA_KEY, ApprovalDecision, ApprovalRequest,
};
use a2a_protocol_types::budget::{BUDGET_EXTENSION_URI, BUDGET_METADATA_KEY, Budget, BudgetStatus};
use a2a_protocol_types::failure::{FAILURE_EXTENSION_URI, FAILURE_METADATA_KEY, class_of};
use a2a_protocol_types::idempotency::{
    IDEMPOTENCY_EXTENSION_URI, IDEMPOTENCY_METADATA_KEY, key_of,
};
use a2a_protocol_types::lease::{LEASE_EXTENSION_URI, LEASE_METADATA_KEY, Lease};
use a2a_protocol_types::provenance::{
    PROVENANCE_EXTENSION_URI, PROVENANCE_METADATA_KEY, provenance_of,
};
use serde_json::{Value, json};

/// Reads `metadata` (an object holding the extension's key) as the SDK
/// does, and answers what was read as JSON, or why it was refused.
type Reader = fn(&Value) -> Result<Value, String>;

fn message(metadata: &Value) -> Message {
    let mut m = Message::user_text("m", "vectors");
    m.metadata = Some(metadata.clone());
    m
}

fn json_of<T: serde::Serialize>(read: Option<T>) -> Value {
    serde_json::to_value(read.expect("the key was present")).expect("serializes")
}

fn lease(md: &Value) -> Result<Value, String> {
    Lease::read(&message(md))
        .map(json_of)
        .map_err(|e| e.message)
}

fn budget_grant(md: &Value) -> Result<Value, String> {
    Budget::read(&message(md))
        .map(json_of)
        .map_err(|e| e.message)
}

fn budget_status(md: &Value) -> Result<Value, String> {
    BudgetStatus::read_from(Some(md))
        .map(json_of)
        .map_err(|e| e.message)
}

fn idempotency(md: &Value) -> Result<Value, String> {
    key_of(&message(md))
        .map(|k| Value::from(k.expect("the key was present")))
        .map_err(|e| e.to_string())
}

fn failure(md: &Value) -> Result<Value, String> {
    Ok(class_of(&message(md)).map_or(Value::Null, |c| Value::from(c.as_str())))
}

fn approval_request(md: &Value) -> Result<Value, String> {
    ApprovalRequest::read(&message(md))
        .map(json_of)
        .map_err(|e| e.message)
}

fn approval_decision(md: &Value) -> Result<Value, String> {
    ApprovalDecision::read(&message(md))
        .map(json_of)
        .map_err(|e| e.message)
}

fn provenance(md: &Value) -> Result<Value, String> {
    provenance_of(&message(md))
        .map(json_of)
        .map_err(|e| e.message)
}

/// Runs one vectors file; returns how many cases it ran.
fn run(file: &str, text: &str, uri: &str, key: &str, read: Reader) -> usize {
    let doc: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
    assert_eq!(doc["uri"], uri, "{file}: uri");
    assert_eq!(doc["metadataKey"], key, "{file}: metadataKey");
    let carry = |value: &Value| json!({ key: value });
    let cases = |list: &str| doc[list].as_array().cloned().unwrap_or_default();
    let mut ran = 0;
    for case in cases("valid").iter().chain(cases("tolerated").iter()) {
        let name = &case["name"];
        let read_once = read(&carry(&case["value"]))
            .unwrap_or_else(|e| panic!("{file}: {name} was refused: {e}"));
        assert_eq!(read_once, case["reads"], "{file}: {name}");
        if !read_once.is_null() {
            let again = read(&carry(&read_once))
                .unwrap_or_else(|e| panic!("{file}: {name} written back was refused: {e}"));
            assert_eq!(again, read_once, "{file}: {name} written and read again");
        }
        ran += 1;
    }
    for case in cases("invalid") {
        let name = &case["name"];
        assert!(
            read(&carry(&case["value"])).is_err(),
            "{file}: {name} was read, not refused"
        );
        ran += 1;
    }
    ran
}

/// One vectors file: its path and text, the URI and key it must name, the
/// reader to run it through, and how many cases it holds.
type Suite = (
    (&'static str, &'static str),
    &'static str,
    &'static str,
    Reader,
    usize,
);

macro_rules! vectors {
    ($path:literal) => {
        (
            $path,
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/extensions/", $path)),
        )
    };
}

#[test]
fn every_published_vector_reads_as_it_says() {
    let suites: [Suite; 8] = [
        (
            vectors!("lease-v1/vectors.json"),
            LEASE_EXTENSION_URI,
            LEASE_METADATA_KEY,
            lease,
            10,
        ),
        (
            vectors!("budget-v1/grant.vectors.json"),
            BUDGET_EXTENSION_URI,
            BUDGET_METADATA_KEY,
            budget_grant,
            16,
        ),
        (
            vectors!("budget-v1/status.vectors.json"),
            BUDGET_EXTENSION_URI,
            BUDGET_METADATA_KEY,
            budget_status,
            6,
        ),
        (
            vectors!("idempotency-v1/vectors.json"),
            IDEMPOTENCY_EXTENSION_URI,
            IDEMPOTENCY_METADATA_KEY,
            idempotency,
            9,
        ),
        (
            vectors!("failure-v1/vectors.json"),
            FAILURE_EXTENSION_URI,
            FAILURE_METADATA_KEY,
            failure,
            7,
        ),
        (
            vectors!("approval-v1/request.vectors.json"),
            APPROVAL_EXTENSION_URI,
            APPROVAL_METADATA_KEY,
            approval_request,
            8,
        ),
        (
            vectors!("approval-v1/decision.vectors.json"),
            APPROVAL_EXTENSION_URI,
            APPROVAL_METADATA_KEY,
            approval_decision,
            7,
        ),
        (
            vectors!("provenance-v1/vectors.json"),
            PROVENANCE_EXTENSION_URI,
            PROVENANCE_METADATA_KEY,
            provenance,
            8,
        ),
    ];
    for ((file, text), uri, key, read, expected) in suites {
        assert_eq!(
            run(file, text, uri, key, read),
            expected,
            "{file}: the number of cases the file holds"
        );
    }
}
