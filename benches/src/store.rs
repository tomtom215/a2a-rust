// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Task stores used to attribute cost, not to be realistic.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};

use a2a_protocol_server::store::TaskStore;

/// A [`TaskStore`] that keeps nothing and returns nothing.
///
/// Exists for one purpose: to answer "how much of the streaming cost is the
/// store?" by removing the store from the path entirely. Every operation is
/// O(1) and allocation-free, so a benchmark that stays slow against this store
/// is slow for some other reason. It is not a plausible deployment — `get`
/// always misses — so it belongs in benchmarks and nowhere else.
pub struct DiscardTaskStore;

impl TaskStore for DiscardTaskStore {
    fn save<'a>(
        &'a self,
        _task: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn get<'a>(
        &'a self,
        _id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        Box::pin(async { Ok(None) })
    }

    fn list<'a>(
        &'a self,
        _params: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> {
        Box::pin(async { Ok(TaskListResponse::new(vec![])) })
    }

    // Reports "inserted" so the handler's create path proceeds exactly as it
    // does against a real store; nothing is retained.
    fn insert_if_absent<'a>(
        &'a self,
        _task: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        Box::pin(async { Ok(true) })
    }

    fn delete<'a>(
        &'a self,
        _id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}
