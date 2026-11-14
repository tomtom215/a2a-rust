// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`StepJournal`] in this process's memory.

use std::collections::HashMap;
use std::sync::Mutex;

use super::{JournalEntry, JournalError, JournalFuture, RunSummary, StepJournal};

/// A journal in this process's memory, for tests and for a deployment that
/// keeps a run on one replica and accepts losing it with the process.
#[derive(Debug, Default)]
pub struct InMemoryStepJournal {
    runs: Mutex<HashMap<String, JournalEntry>>,
}

impl InMemoryStepJournal {
    /// An empty journal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many runs it holds, finished ones included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many of its runs have no final answer yet.
    #[must_use]
    pub fn unfinished(&self) -> usize {
        self.lock().values().filter(|e| !e.completed).count()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, JournalEntry>> {
        self.runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl StepJournal for InMemoryStepJournal {
    fn load<'a>(&'a self, key: &'a str) -> JournalFuture<'a, Option<JournalEntry>> {
        Box::pin(async move { Ok(self.lock().get(key).cloned()) })
    }

    fn save<'a>(&'a self, key: &'a str, entry: &'a JournalEntry) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let mut runs = self.lock();
            let stored = runs.get(key).map_or(0, |e| e.revision);
            if stored.checked_add(1) != Some(entry.revision) {
                return Err(JournalError::Conflict);
            }
            runs.insert(key.to_owned(), entry.clone());
            drop(runs);
            Ok(())
        })
    }

    fn clear<'a>(&'a self, key: &'a str, revision: u64) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let mut runs = self.lock();
            match runs.get(key).map(|e| e.revision) {
                None => Ok(()),
                Some(stored) if stored == revision => {
                    runs.remove(key);
                    drop(runs);
                    Ok(())
                }
                Some(_) => Err(JournalError::Conflict),
            }
        })
    }

    fn sweep(&self, completed_before_ms: i64, unfinished_before_ms: i64) -> JournalFuture<'_, u64> {
        Box::pin(async move {
            let mut runs = self.lock();
            let before = runs.len();
            runs.retain(|_, e| {
                let cutoff = if e.completed {
                    completed_before_ms
                } else {
                    unfinished_before_ms
                };
                e.updated_at_ms >= cutoff
            });
            let gone = before - runs.len();
            drop(runs);
            Ok(gone as u64)
        })
    }

    fn runs_of<'a>(
        &'a self,
        principal: &'a str,
        tenant: Option<&'a str>,
    ) -> JournalFuture<'a, Vec<RunSummary>> {
        Box::pin(async move {
            let mut out: Vec<RunSummary> = self
                .lock()
                .iter()
                .filter(|(_, e)| e.owner.is_of(principal, tenant))
                .map(|(k, e)| e.summary(k))
                .collect();
            out.sort_by(|a, b| a.key.cmp(&b.key));
            Ok(out)
        })
    }

    fn erase<'a>(&'a self, principal: &'a str, tenant: Option<&'a str>) -> JournalFuture<'a, u64> {
        Box::pin(async move {
            let mut runs = self.lock();
            let before = runs.len();
            runs.retain(|_, e| !e.owner.is_of(principal, tenant));
            let gone = before - runs.len();
            drop(runs);
            Ok(gone as u64)
        })
    }
}
