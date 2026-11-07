// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A store that watches its event log without a database: an in-memory store
//! plus a [`LogWakers`] registry it answers `watch_event_log` from, and the
//! counts a test reads to see what the stream above it did. Test-only.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};

use super::{LogWakers, key};
use crate::store::{InMemoryTaskStore, LogWatch, TaskStore};

type Fut<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

/// An in-memory store that can be watched.
pub struct Watching {
    pub inner: InMemoryTaskStore,
    pub wakers: LogWakers,
    /// Reads of a task: what one poll of a stream is.
    pub reads: Arc<AtomicUsize>,
    /// Watches asked for.
    pub watches: Arc<AtomicUsize>,
    /// Whether it refuses to watch (the answer a store that cannot push gives).
    pub mute: bool,
    /// A key to notify from inside a read, as an append landing between a
    /// stream's read and its wait would.
    pub notify_during_read: std::sync::Mutex<Option<String>>,
}

impl Watching {
    pub(crate) fn new() -> Self {
        Self {
            inner: InMemoryTaskStore::new(),
            wakers: LogWakers::new(),
            reads: Arc::default(),
            watches: Arc::default(),
            mute: false,
            notify_during_read: std::sync::Mutex::default(),
        }
    }

    pub(crate) fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    pub(crate) fn watches(&self) -> usize {
        self.watches.load(Ordering::SeqCst)
    }
}

impl TaskStore for Watching {
    fn save<'a>(&'a self, task: &'a Task) -> Fut<'a, ()> {
        self.inner.save(task)
    }
    fn get<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<Task>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if let Some(key) = self.notify_during_read.lock().unwrap().as_ref() {
            self.wakers.notify(key);
        }
        self.inner.get(id)
    }
    fn list<'a>(&'a self, params: &'a ListTasksParams) -> Fut<'a, TaskListResponse> {
        self.inner.list(params)
    }
    fn insert_if_absent<'a>(&'a self, task: &'a Task) -> Fut<'a, bool> {
        self.inner.insert_if_absent(task)
    }
    fn delete<'a>(&'a self, id: &'a TaskId) -> Fut<'a, ()> {
        self.inner.delete(id)
    }
    fn watch_event_log(&self, task_id: &TaskId) -> Option<LogWatch> {
        self.watches.fetch_add(1, Ordering::SeqCst);
        if self.mute {
            return None;
        }
        Some(self.wakers.watch(key("", &task_id.0)?))
    }
}
