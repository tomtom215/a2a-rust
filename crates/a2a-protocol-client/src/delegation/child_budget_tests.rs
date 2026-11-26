// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A meter a handed-back turn did not report cannot be known: carried as
//! granted, and kept whole at settlement.

use std::collections::BTreeMap;
use std::time::Duration;

use a2a_protocol_types::budget::{Budget, BudgetHandle, BudgetStatus};

use super::ChildBudget;

fn spent(tokens: u64) -> BudgetStatus {
    BudgetStatus::new(
        None,
        BTreeMap::new(),
        BTreeMap::from([("tokens".to_owned(), tokens)]),
    )
}

fn child(parent: &BudgetHandle) -> ChildBudget {
    ChildBudget::new(
        parent
            .grant(&Budget::new().with_meter("tokens", 100).unwrap())
            .unwrap(),
    )
}

fn parent() -> BudgetHandle {
    BudgetHandle::start(
        &Budget::new()
            .with_time(Duration::from_secs(60))
            .with_meter("tokens", 1000)
            .unwrap(),
    )
}

#[tokio::test(start_paused = true)]
async fn an_unreported_turn_leaves_the_meter_as_granted_and_reserved() {
    let parent = parent();
    let mut budget = child(&parent);
    budget.handed_back(None);
    assert_eq!(budget.left().meters.get("tokens"), Some(&100));
    budget.settled_with(Some(spent(20)));
    budget.settle();
    assert_eq!(parent.remaining("tokens"), Some(900), "the whole 100 kept");
}

#[tokio::test(start_paused = true)]
async fn reported_turns_are_summed() {
    let parent = parent();
    let mut budget = child(&parent);
    budget.handed_back(Some(&spent(10)));
    budget.handed_back(Some(&spent(15)));
    assert_eq!(budget.left().meters.get("tokens"), Some(&75));
    budget.settled_with(Some(spent(5)));
    budget.settle();
    assert_eq!(parent.remaining("tokens"), Some(970), "charged 10 + 15 + 5");
}

#[tokio::test(start_paused = true)]
async fn time_runs_out_with_the_grant() {
    let parent = parent();
    let budget = ChildBudget::new(
        parent
            .grant(&Budget::new().with_time(Duration::from_secs(2)))
            .unwrap(),
    );
    assert!(!budget.out_of_time());
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(budget.out_of_time());
    assert_eq!(budget.left().remaining_ms, Some(0));
}
