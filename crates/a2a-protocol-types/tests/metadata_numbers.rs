// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Every integer an extension of this SDK reads from metadata, written as a
//! protobuf `Struct` writes it: a2a-python keeps metadata in a `Struct`,
//! whose only number is a double, so `30000` reaches us as `30000.0`. Each
//! decoder takes the whole double as the integer and refuses a fraction, a
//! negative, and a value past its type, as invalid params. Found by the
//! Python interop run on the lineage depth.

use a2a_protocol_types::Message;
use a2a_protocol_types::budget::{BUDGET_EXTENSION_URI, BUDGET_METADATA_KEY, Budget, BudgetStatus};
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::lease::{LEASE_METADATA_KEY, Lease};
use a2a_protocol_types::lineage::{LINEAGE_METADATA_KEY, Lineage};
use serde_json::{Value, json};

fn carrying(key: &str, value: Value) -> Message {
    let mut m = Message::user_text("m", "work");
    m.metadata = Some(json!({ key: value }));
    m
}

#[test]
fn a_lease_ttl_written_as_a_double_is_read() {
    let lease = Lease::read(&carrying(LEASE_METADATA_KEY, json!({"ttlMs": 30000.0})))
        .unwrap()
        .expect("a lease");
    assert_eq!(lease.ttl_ms, 30_000);
    for bad in [json!(30000.5), json!(-1.0), json!(-1), json!(1.0e19)] {
        let err =
            Lease::read(&carrying(LEASE_METADATA_KEY, json!({"ttlMs": bad}))).expect_err("refused");
        assert_eq!(err.code, ErrorCode::InvalidParams, "{bad}: {err:?}");
    }
}

#[test]
fn a_budget_grant_written_as_doubles_is_read() {
    let grant = Budget::read(&carrying(
        BUDGET_METADATA_KEY,
        json!({"remainingMs": 30000.0, "meters": {"tokens": 50000.0}}),
    ))
    .unwrap()
    .expect("a budget");
    let expected = Budget::new()
        .with_time(std::time::Duration::from_millis(30_000))
        .with_meter("tokens", 50_000)
        .unwrap();
    assert_eq!(grant, expected);
    for bad in [
        json!({"remainingMs": 30000.5}),
        json!({"remainingMs": -5.0}),
        json!({"meters": {"tokens": 0.5}}),
        json!({"meters": {"tokens": -1}}),
        json!({"meters": {"tokens": 1.0e20}}),
    ] {
        let err = Budget::read(&carrying(BUDGET_METADATA_KEY, bad.clone())).expect_err("refused");
        assert_eq!(err.code, ErrorCode::InvalidParams, "{bad}: {err:?}");
    }
    assert_eq!(
        BUDGET_EXTENSION_URI,
        "https://a2a-rust.com/extensions/budget/v1"
    );
}

#[test]
fn a_budget_status_written_as_doubles_is_read() {
    let metadata = json!({ BUDGET_METADATA_KEY: {
        "remainingMs": 12000.0,
        "meters": {"tokens": 41000.0},
        "spent": {"tokens": 9000.0},
    }});
    let status = BudgetStatus::read_from(Some(&metadata))
        .unwrap()
        .expect("a status");
    assert_eq!(
        status,
        BudgetStatus::new(
            Some(12_000),
            [("tokens".to_owned(), 41_000)].into(),
            [("tokens".to_owned(), 9_000)].into(),
        )
    );
    let bad = json!({ BUDGET_METADATA_KEY: {"spent": {"tokens": 1.5}} });
    assert_eq!(
        BudgetStatus::read_from(Some(&bad)).unwrap_err().code,
        ErrorCode::InvalidParams
    );
}

#[test]
fn a_lineage_depth_written_as_a_double_is_read() {
    let lineage = Lineage::read(&carrying(
        LINEAGE_METADATA_KEY,
        json!({"root": "job", "depth": 2.0, "parentTaskId": "p"}),
    ))
    .unwrap()
    .expect("a lineage");
    assert_eq!(lineage.depth, 2);
    for bad in [json!(2.5), json!(-1.0), json!(4_294_967_297.0)] {
        let err = Lineage::read(&carrying(
            LINEAGE_METADATA_KEY,
            json!({"root": "job", "depth": bad, "parentTaskId": "p"}),
        ))
        .expect_err("refused");
        assert_eq!(err.code, ErrorCode::InvalidParams, "{bad}: {err:?}");
    }
}
