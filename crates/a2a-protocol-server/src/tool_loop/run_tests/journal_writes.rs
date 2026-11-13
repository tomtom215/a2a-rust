// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the step journal does when a write fails or arrives late (adversarial
//! review S1, 2026-11-12): the call it was to record does not run, and a writer
//! that lost its claim cannot overwrite newer progress.

use super::journal::{KEY, journal, looped, model_fails, one_transfer, run_keyed};
use super::strict::mutating;
use super::*;
use crate::tool_loop::journal::{InMemoryStepJournal, StepJournal};
use crate::tool_loop::steering::Side;

/// A journal that fails its `n`th write.
struct FailsAt {
    inner: InMemoryStepJournal,
    writes: AtomicUsize,
    n: usize,
}

impl StepJournal for FailsAt {
    fn load<'a>(
        &'a self,
        key: &'a str,
    ) -> crate::tool_loop::JournalFuture<'a, Option<crate::tool_loop::RunCheckpoint>> {
        self.inner.load(key)
    }
    fn save<'a>(
        &'a self,
        key: &'a str,
        cp: &'a crate::tool_loop::RunCheckpoint,
    ) -> crate::tool_loop::JournalFuture<'a, ()> {
        let nth = self.writes.fetch_add(1, Ordering::SeqCst) + 1;
        if nth == self.n {
            return Box::pin(async {
                Err(crate::tool_loop::JournalError::Store(
                    "disk full".to_owned(),
                ))
            });
        }
        self.inner.save(key, cp)
    }
    fn clear<'a>(&'a self, key: &'a str) -> crate::tool_loop::JournalFuture<'a, ()> {
        self.inner.clear(key)
    }
}

/// A checkpoint the journal cannot write stops the run before the step it was
/// to record: the tool does not run on a promise of a record.
#[tokio::test]
async fn a_call_the_journal_cannot_record_is_not_run() {
    // Write 1 follows the model's reply; write 2 is the receipt, before the call.
    for failing in [1, 2] {
        let (transfer, used) = mutating();
        let (model, _) = Scripted::new(vec![one_transfer(), text("done")]);
        let tool_loop =
            ToolLoop::new(model, tools(vec![transfer])).with_journal(Arc::new(FailsAt {
                inner: InMemoryStepJournal::new(),
                writes: AtomicUsize::new(0),
                n: failing,
            }));
        let r = run_keyed(&tool_loop, KEY, Side::default()).await;
        assert!(
            matches!(&r, Err(LoopError::Journal(w)) if w.contains("disk full")),
            "{failing}: {r:?}"
        );
        assert_eq!(
            used.load(Ordering::SeqCst),
            0,
            "write {failing} failed and the tool ran"
        );
    }
}

/// A write that does not follow the stored revision is refused, so a replica
/// that lost its claim cannot overwrite a newer run.
#[tokio::test]
async fn a_stale_writer_cannot_overwrite_newer_progress() {
    let j = journal();
    let (transfer, _) = mutating();
    let (model, _) = Scripted::new(vec![one_transfer(), model_fails()]);
    let _ = run_keyed(&looped(model, vec![transfer], &j), KEY, Side::default()).await;
    let stored = j.load(KEY).await.unwrap().unwrap();
    // The same revision again (a second writer that read the one before).
    assert_eq!(
        j.save(KEY, &stored).await,
        Err(crate::tool_loop::JournalError::Conflict)
    );
    let mut skipped = stored.clone();
    skipped.revision += 2;
    assert_eq!(
        j.save(KEY, &skipped).await,
        Err(crate::tool_loop::JournalError::Conflict)
    );
    let mut next = stored.clone();
    next.revision += 1;
    assert_eq!(j.save(KEY, &next).await, Ok(()));
    let mut first = stored;
    first.revision = 1;
    assert_eq!(j.save("another", &first).await, Ok(()));
    assert_eq!(
        j.save("another", &first).await,
        Err(crate::tool_loop::JournalError::Conflict)
    );
}
