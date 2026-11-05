// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The admission configuration: its defaults, its setters, and what the
//! build refuses.

use std::time::Duration;

use super::*;

#[test]
fn a_share_defaults_to_weight_one_priority_zero_nothing_reserved() {
    let share = TenantShare::default();
    assert_eq!(share, TenantShare::new());
    assert_eq!(
        (share.weight(), share.priority(), share.reserved()),
        (1, 0, 0)
    );
    let set = TenantShare::new()
        .with_weight(3)
        .with_priority(2)
        .with_reserved(5);
    assert_eq!((set.weight(), set.priority(), set.reserved()), (3, 2, 5));
    assert_eq!(TenantShare::new().with_weight(0).weight(), 1);
    assert_eq!(
        format!("{set:?}"),
        "TenantShare { weight: 3, priority: 2, reserved: 5 }"
    );
}

#[test]
fn a_gate_waits_for_nothing_until_told_to() {
    let gate = Admission::new(8);
    assert_eq!(gate.capacity(), 8);
    assert_eq!(gate.max_queued(), 0);
    assert_eq!(gate.max_wait(), Duration::ZERO);
    let gate = gate
        .with_max_queued(5)
        .with_max_wait(Duration::from_millis(250))
        .with_default_share(TenantShare::new().with_weight(2))
        .with_tenant("a", TenantShare::new().with_priority(1));
    assert_eq!(gate.max_queued(), 5);
    assert_eq!(gate.max_wait(), Duration::from_millis(250));
    assert_eq!(gate.share("a").priority(), 1);
    assert_eq!(gate.share("b").weight(), 2);
    assert_eq!(gate.invalid(8), None);
}

/// Each refusal, at its boundary.
#[test]
fn the_build_refuses_a_gate_that_cannot_work() {
    assert!(
        Admission::new(0)
            .invalid(1024)
            .unwrap()
            .contains("at least 1")
    );
    assert_eq!(Admission::new(1).invalid(1024), None);
    assert_eq!(Admission::new(64).invalid(64), None);
    assert!(
        Admission::new(65)
            .invalid(64)
            .unwrap()
            .contains("above the concurrent-stream cap (64)")
    );
    let reserving_default =
        Admission::new(4).with_default_share(TenantShare::new().with_reserved(1));
    assert!(
        reserving_default
            .invalid(64)
            .unwrap()
            .contains("default share reserves")
    );
    let full = Admission::new(4)
        .with_tenant("a", TenantShare::new().with_reserved(3))
        .with_tenant("b", TenantShare::new().with_reserved(1));
    assert_eq!(full.invalid(64), None, "reservations may take it all");
    let over = full.with_tenant("c", TenantShare::new().with_reserved(1));
    assert_eq!(
        over.invalid(64).unwrap(),
        "the admission gate's reservations (5) are more than its capacity (4)"
    );
    let huge = Admission::new(4)
        .with_tenant("a", TenantShare::new().with_reserved(usize::MAX))
        .with_tenant("b", TenantShare::new().with_reserved(1));
    assert!(
        huge.invalid(64)
            .unwrap()
            .contains(&format!("({})", usize::MAX))
    );
}

#[test]
fn the_load_is_three_numbers() {
    let load = AdmissionLoad {
        running: 1,
        queued: 2,
        capacity: 3,
    };
    assert_eq!(
        format!("{load:?}"),
        "AdmissionLoad { running: 1, queued: 2, capacity: 3 }"
    );
}
