// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ByteBudget`]'s accounting, its wait, and its headroom rule.

use std::time::Duration;

use super::*;

/// A charge is held until its last clone drops, and only then returned.
#[test]
fn a_charge_is_held_until_its_last_clone_drops() {
    let budget = ByteBudget::new(100);
    let a = budget.try_charge(60).unwrap();
    assert_eq!(budget.held(), 60);
    let shared = a.clone();
    drop(a);
    assert_eq!(budget.held(), 60, "a clone still holds it");
    assert!(budget.try_charge(41).is_none(), "41 of the 40 left");
    let b = budget.try_charge(40).unwrap();
    assert_eq!(budget.held(), 100);
    drop(shared);
    drop(b);
    assert_eq!(budget.held(), 0);
    assert_eq!(budget.total(), 100);
}

/// An event larger than the whole budget is charged the whole budget, so it
/// is accepted once nothing else is held, and refused while anything is.
#[test]
fn an_event_larger_than_the_budget_is_charged_all_of_it() {
    let budget = ByteBudget::new(100);
    let small = budget.try_charge(1).unwrap();
    assert!(budget.try_charge(10_000).is_none());
    drop(small);
    let all = budget.try_charge(10_000).unwrap();
    assert_eq!(budget.held(), 100);
    assert_eq!(format!("{all:?}"), "Charge(100)");
    assert_eq!(
        ByteBudget::new(0).total(),
        1,
        "a budget is at least one byte"
    );
}

/// A charge that waits is given the bytes the moment they are returned.
#[tokio::test(start_paused = true)]
async fn a_charge_waits_for_room_until_its_deadline() {
    let budget = ByteBudget::new(10);
    let held = budget.try_charge(10).unwrap();
    let started = tokio::time::Instant::now();
    let waiter = {
        let budget = budget.clone();
        tokio::spawn(async move {
            budget
                .charge_by(4, started + Duration::from_secs(5))
                .await
                .map(|c| (c, tokio::time::Instant::now()))
        })
    };
    tokio::time::sleep(Duration::from_millis(700)).await;
    drop(held);
    let (charge, at) = waiter.await.unwrap().unwrap();
    assert_eq!(at - started, Duration::from_millis(700));
    assert_eq!(budget.held(), 4);
    drop(charge);

    let _full = budget.try_charge(10).unwrap();
    let started = tokio::time::Instant::now();
    assert!(
        budget
            .charge_by(1, started + Duration::from_secs(2))
            .await
            .is_none()
    );
    assert_eq!(started.elapsed(), Duration::from_secs(2));
}

/// New work is admitted while a tenth of the budget is free, and not below.
#[test]
fn new_work_needs_a_tenth_of_the_budget_free() {
    let budget = ByteBudget::new(100);
    let _most = budget.try_charge(90).unwrap();
    assert!(budget.admits_new_work(), "10 of 100 free");
    let _one = budget.try_charge(1).unwrap();
    assert!(!budget.admits_new_work(), "9 of 100 free");
    let tiny = ByteBudget::new(5);
    let _four = tiny.try_charge(4).unwrap();
    assert!(tiny.admits_new_work(), "a tenth of 5 rounds up to one byte");
    let _last = tiny.try_charge(1).unwrap();
    assert!(!tiny.admits_new_work());
}

#[test]
fn the_debug_form_says_the_size_and_what_is_held() {
    let budget = ByteBudget::new(64);
    let _c = budget.try_charge(16).unwrap();
    assert_eq!(
        format!("{budget:?}"),
        "ByteBudget { total: 64, held: 16, .. }"
    );
}
