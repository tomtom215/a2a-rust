// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which run of a task a record belongs to: [`AuditLog`] remembers, per task,
//! the `seq` of the `run.started` record of the run under way, so the task's
//! events can name it (`runSeq`).

use std::collections::{HashMap, VecDeque};

use super::AuditLog;

/// How many in-flight task runs [`AuditLog`] remembers the caller of.
///
/// A run is forgotten when its task reaches a terminal state. One that never
/// does — its process died — would otherwise be remembered for ever, so the
/// registry is bounded and forgets the oldest first. A task event whose run
/// was forgotten is still recorded, with no `runSeq`.
const MAX_TRACKED_RUNS: usize = 65_536;

#[derive(Default)]
pub(super) struct Runs {
    by_task: HashMap<(String, String), u64>,
    order: VecDeque<(String, String)>,
}

impl AuditLog {
    pub(crate) fn begin_run(&self, chain: &str, task_id: &str, run_seq: u64) {
        let mut runs = self
            .runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (chain.to_owned(), task_id.to_owned());
        if runs.by_task.insert(key.clone(), run_seq).is_none() {
            runs.order.push_back(key);
        }
        while runs.order.len() > MAX_TRACKED_RUNS {
            if let Some(old) = runs.order.pop_front() {
                runs.by_task.remove(&old);
            }
        }
        drop(runs);
    }

    pub(crate) fn run_of(&self, chain: &str, task_id: &str) -> Option<u64> {
        let runs = self
            .runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runs.by_task
            .get(&(chain.to_owned(), task_id.to_owned()))
            .copied()
    }

    pub(crate) fn end_run(&self, chain: &str, task_id: &str) {
        let mut runs = self
            .runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (chain.to_owned(), task_id.to_owned());
        if runs.by_task.remove(&key).is_some() {
            runs.order.retain(|k| k != &key);
        }
    }

    #[cfg(test)]
    pub(crate) fn tracked_runs(&self) -> usize {
        self.runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .by_task
            .len()
    }
}
