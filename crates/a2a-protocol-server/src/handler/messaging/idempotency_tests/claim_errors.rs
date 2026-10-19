// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a failed claim reports. Only a temporary refusal is `Overloaded`,
//! which tells the client that retrying is safe; any other failure of the
//! store is reported as the store's error.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::message::MessageId;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};

use super::KEY;
use super::fixtures::{CountingExecutor, keyed_params};
use crate::builder::RequestHandlerBuilder;
use crate::error::ServerError;
use crate::store::task_store::IdempotencyClaim;
use crate::store::{InMemoryTaskStore, TaskStore};

/// Every claim fails with an error that is not a temporary refusal.
#[derive(Default)]
struct BrokenClaims(InMemoryTaskStore);

impl TaskStore for BrokenClaims {
    fn save<'a>(&'a self, t: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.save(t)
    }
    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        self.0.get(id)
    }
    fn list<'a>(
        &'a self,
        p: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> {
        self.0.list(p)
    }
    fn insert_if_absent<'a>(
        &'a self,
        t: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.0.insert_if_absent(t)
    }
    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.delete(id)
    }
    fn supports_idempotency(&self) -> bool {
        true
    }
    fn claim_idempotency_key<'a>(
        &'a self,
        _key: &'a str,
        _message_id: &'a MessageId,
        _task_id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<IdempotencyClaim>> + Send + 'a>> {
        Box::pin(async { Err(A2aError::internal("the key index is corrupt")) })
    }
}

#[tokio::test]
async fn a_claim_that_fails_for_good_is_not_reported_as_overloaded() {
    let executions = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::clone(&executions)))
        .without_agent_card()
        .with_task_store(BrokenClaims::default())
        .build()
        .expect("handler");
    let err = handler
        .on_send_message(keyed_params("msg-1", Some(KEY)), false, None)
        .await
        .expect_err("the claim fails");
    assert!(
        !matches!(err, ServerError::Overloaded(_)),
        "a store failure is not a temporary refusal: {err:?}"
    );
    assert!(
        err.to_string().contains("the key index is corrupt"),
        "{err}"
    );
    assert_eq!(executions.load(Ordering::SeqCst), 0, "nothing ran");
}
