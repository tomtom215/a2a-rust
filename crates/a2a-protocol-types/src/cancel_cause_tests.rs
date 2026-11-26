// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use super::*;

#[test]
fn every_cause_reads_back_as_itself() {
    for &cause in CancelCause::ALL {
        let mut m = Message::agent_text("m", "stopped");
        set_cause(&mut m, cause);
        assert_eq!(cause_of(&m), Some(cause));
        assert_eq!(CancelCause::from_wire(&cause.to_string()), Some(cause));
    }
}

#[test]
fn the_uri_is_named_once_and_other_metadata_is_kept() {
    let mut m = Message::agent_text("m", "stopped");
    m.metadata = Some(serde_json::json!({ "mine": 1 }));
    set_cause(&mut m, CancelCause::Halted);
    set_cause(&mut m, CancelCause::LeaseLapsed);
    assert_eq!(
        m.extensions.as_deref(),
        Some(&[CANCEL_EXTENSION_URI.to_owned()][..])
    );
    let md = m.metadata.as_ref().unwrap();
    assert_eq!(md["mine"], 1);
    assert_eq!(md[CANCEL_METADATA_KEY], "lease-lapsed");
}

#[test]
fn metadata_that_is_not_an_object_is_replaced() {
    let mut m = Message::agent_text("m", "stopped");
    m.metadata = Some(serde_json::json!([1]));
    set_cause(&mut m, CancelCause::CancelRequested);
    assert_eq!(cause_of(&m), Some(CancelCause::CancelRequested));
}

#[test]
fn no_cause_and_an_unknown_cause_read_as_none() {
    let mut m = Message::agent_text("m", "stopped");
    assert_eq!(cause_of(&m), None);
    m.metadata = Some(serde_json::json!({ CANCEL_METADATA_KEY: "budget-spent" }));
    assert_eq!(cause_of(&m), None);
    m.metadata = Some(serde_json::json!({ CANCEL_METADATA_KEY: 3 }));
    assert_eq!(cause_of(&m), None);
}
