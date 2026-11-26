// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A child's budget across the turns a hand-off splits it into (COR-03,
//! E03 R1).
//!
//! The server opens a budget account from each message it receives, so a
//! continuation that carried the first message's budget again gave the child
//! the whole grant a second time: its deadline restarted, and its meters were
//! full. A continuation now carries what is left: the grant's time less the
//! time since it was made, and each meter less what the handed-back turns
//! reported spending. A turn that reported no spend of a meter leaves that
//! meter unknown: the continuation carries it as granted (the spend cannot be
//! known), and the settlement keeps the whole reservation of it, as
//! [`ChildGrant::settle`] does for a meter the child never reported.
//!
//! Each turn reports its own spend, so the settlement charges the parent the
//! sum over the turns, not the last turn's alone, which would refund what the
//! earlier turns spent.

use std::collections::{BTreeMap, BTreeSet};

use a2a_protocol_types::budget::{Budget, BudgetStatus, ChildGrant};
use tokio::time::Instant;

/// A child's grant and what its turns have reported spending.
#[derive(Debug)]
pub(super) struct ChildBudget {
    grant: ChildGrant,
    /// When the grant was made: the child's time is counted from here.
    granted_at: Instant,
    /// The report on the event that settled the child, if any.
    last: Option<BudgetStatus>,
    /// What the handed-back turns reported spending, summed by meter.
    earlier: BTreeMap<String, u64>,
    /// Meters some handed-back turn did not report.
    unknown: BTreeSet<String>,
}

impl ChildBudget {
    pub(super) fn new(grant: ChildGrant) -> Self {
        Self {
            grant,
            granted_at: Instant::now(),
            last: None,
            earlier: BTreeMap::new(),
            unknown: BTreeSet::new(),
        }
    }

    /// Notes the report on the event that settled the child.
    pub(super) fn settled_with(&mut self, report: Option<BudgetStatus>) {
        self.last = report;
    }

    /// Notes what a turn that was handed back reported spending.
    pub(super) fn handed_back(&mut self, report: Option<&BudgetStatus>) {
        for meter in self.grant.budget().meters.keys() {
            match report.and_then(|r| r.spent.get(meter)) {
                Some(&spent) => {
                    let sum = self.earlier.entry(meter.clone()).or_insert(0);
                    *sum = sum.saturating_add(spent);
                }
                None => {
                    self.unknown.insert(meter.clone());
                }
            }
        }
    }

    /// The budget a continuation carries: the time left of the grant, and
    /// each meter less what the earlier turns are known to have spent.
    pub(super) fn left(&self) -> Budget {
        let mut left = self.grant.budget().clone();
        let elapsed = u64::try_from(self.granted_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        left.remaining_ms = left.remaining_ms.map(|ms| ms.saturating_sub(elapsed));
        for (meter, amount) in &mut left.meters {
            if !self.unknown.contains(meter) {
                let spent = self.earlier.get(meter).copied().unwrap_or(0);
                *amount = amount.saturating_sub(spent);
            }
        }
        left
    }

    /// Whether the grant's time has run out, so no continuation is worth
    /// sending: the server refuses a budget with no time left.
    pub(super) fn out_of_time(&self) -> bool {
        self.left().remaining_ms == Some(0)
    }

    /// Settles the grant with the spend of every turn: for each meter every
    /// turn reported, the earlier turns' sum and the last report's; any
    /// other meter keeps the whole reservation.
    pub(super) fn settle(self) {
        let Some(last) = self.last else {
            self.grant.settle(None);
            return;
        };
        let spent = last
            .spent
            .iter()
            .filter(|(meter, _)| !self.unknown.contains(*meter))
            .map(|(meter, &spent)| {
                let before = self.earlier.get(meter).copied().unwrap_or(0);
                (meter.clone(), before.saturating_add(spent))
            })
            .collect();
        let total = BudgetStatus::new(last.remaining_ms, last.meters, spent);
        self.grant.settle(Some(&total));
    }
}

#[cfg(test)]
#[path = "child_budget_tests.rs"]
mod tests;
