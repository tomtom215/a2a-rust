// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Test doubles and builders for the idempotency send-path tests.
//!
//! Split from `mod.rs` when the suite crossed the 500-line limit
//! `CONTRIBUTING.md` sets. These are the fixtures — an executor that counts
//! how often it ran, two stores that withhold or break something on purpose,
//! and the builders for a keyed send and a minimal card. The assertions stay
//! next door.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::idempotency::set_key;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::{Task, TaskId};

use crate::builder::RequestHandlerBuilder;
use crate::executor::AgentExecutor;
use crate::handler::RequestHandler;
use crate::request_context::RequestContext;
use crate::streaming::EventQueueWriter;

use super::super::{SendMessageResponse, SendMessageResult};

/// Counts how many times the send path actually ran an agent.
///
/// The whole point of a key is that a retry does not reach this a second
/// time, so the count is the assertion that matters.
pub(super) struct CountingExecutor(pub(super) Arc<AtomicUsize>);

impl AgentExecutor for CountingExecutor {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

pub(super) fn counting_handler() -> (RequestHandler, Arc<AtomicUsize>) {
    let counter = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::clone(&counter)))
        .build()
        .expect("default build should succeed");
    (handler, counter)
}
/// A send whose message id is `msg_id`, carrying `key` when one is given.
pub(super) fn keyed_params(msg_id: &str, key: Option<&str>) -> MessageSendParams {
    let mut message = Message {
        id: MessageId::new(msg_id),
        role: MessageRole::User,
        parts: vec![Part::text("hello")],
        context_id: None,
        task_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    };
    if let Some(key) = key {
        set_key(&mut message, key).expect("test key must be valid");
    }
    MessageSendParams {
        message,
        configuration: None,
        metadata: None,
        tenant: None,
    }
}

pub(super) fn task_of(result: &SendMessageResult) -> &Task {
    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => task,
        other => panic!("expected a Task response, got {other:?}"),
    }
}
pub(super) fn card() -> a2a_protocol_types::AgentCard {
    use a2a_protocol_types::{AgentCapabilities, AgentCard, AgentInterface};
    AgentCard {
        url: None,
        name: "test".into(),
        version: "1.0".into(),
        description: "Test agent".into(),
        supported_interfaces: vec![AgentInterface {
            url: "http://localhost:8080".into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        provider: None,
        icon_url: None,
        documentation_url: None,
        capabilities: AgentCapabilities::none(),
        security_schemes: None,
        security_requirements: None,
        default_input_modes: vec![],
        default_output_modes: vec![],
        skills: vec![],
        signatures: None,
    }
}

pub(super) fn advertised(
    handler: &RequestHandler,
) -> Option<&a2a_protocol_types::extensions::AgentExtension> {
    handler
        .agent_card
        .as_ref()?
        .capabilities
        .extensions
        .as_ref()?
        .iter()
        .find(|e| e.uri == a2a_protocol_types::idempotency::IDEMPOTENCY_EXTENSION_URI)
}
/// A store that does not implement the idempotency index, delegating
/// everything else to the in-memory one.
#[derive(Default)]
pub(super) struct NoIdempotencyStore {
    inner: crate::store::InMemoryTaskStore,
}

#[allow(clippy::manual_async_fn)]
impl crate::store::TaskStore for NoIdempotencyStore {
    fn save<'a>(&'a self, t: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.save(t)
    }
    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        self.inner.get(id)
    }
    fn list<'a>(
        &'a self,
        p: &'a a2a_protocol_types::params::ListTasksParams,
    ) -> Pin<
        Box<
            dyn Future<Output = A2aResult<a2a_protocol_types::responses::TaskListResponse>>
                + Send
                + 'a,
        >,
    > {
        self.inner.list(p)
    }
    fn insert_if_absent<'a>(
        &'a self,
        t: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.inner.insert_if_absent(t)
    }
    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.delete(id)
    }
}

/// Fails the first `save`, then behaves normally. Idempotency delegates to
/// the inner store, so the key machinery is real.
pub(super) struct FailFirstSaveStore {
    inner: crate::store::InMemoryTaskStore,
    failed_once: std::sync::atomic::AtomicBool,
}

impl Default for FailFirstSaveStore {
    fn default() -> Self {
        Self {
            inner: crate::store::InMemoryTaskStore::new(),
            failed_once: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl crate::store::TaskStore for FailFirstSaveStore {
    fn save<'a>(&'a self, t: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            if !self.failed_once.swap(true, Ordering::SeqCst) {
                return Err(a2a_protocol_types::error::A2aError::internal(
                    "injected store failure",
                ));
            }
            self.inner.save(t).await
        })
    }
    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        self.inner.get(id)
    }
    fn list<'a>(
        &'a self,
        p: &'a a2a_protocol_types::params::ListTasksParams,
    ) -> Pin<
        Box<
            dyn Future<Output = A2aResult<a2a_protocol_types::responses::TaskListResponse>>
                + Send
                + 'a,
        >,
    > {
        self.inner.list(p)
    }
    fn insert_if_absent<'a>(
        &'a self,
        t: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.inner.insert_if_absent(t)
    }
    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.delete(id)
    }
    fn supports_idempotency(&self) -> bool {
        self.inner.supports_idempotency()
    }
    fn claim_idempotency_key<'a>(
        &'a self,
        key: &'a str,
        message_id: &'a MessageId,
        task_id: &'a TaskId,
    ) -> Pin<
        Box<dyn Future<Output = A2aResult<crate::store::task_store::IdempotencyClaim>> + Send + 'a>,
    > {
        self.inner.claim_idempotency_key(key, message_id, task_id)
    }
    fn release_idempotency_key<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.release_idempotency_key(key)
    }
}

/// Reports a chosen task as absent for the next `n` reads, then delegates.
///
/// Reproduces the window the claim opens. `claim_idempotency_key` commits
/// before `persist_initial_task` writes the row — it has to, or two racing
/// duplicates would each get past the claim and both execute — so a duplicate
/// that reaches the replay path in that window asks for a task that does not
/// exist yet. The window is short and timing-dependent, which is why it is
/// injected here rather than raced for: a test that raced it would be the
/// kind that passes on a fast machine and fails in CI.
///
/// Targeted at one id rather than counting every read, so an unrelated `get`
/// elsewhere in the send path cannot consume the budget and leave the test
/// asserting nothing.
pub(super) struct WithholdingStore {
    inner: crate::store::InMemoryTaskStore,
    withheld: std::sync::Mutex<Option<(TaskId, usize)>>,
}

impl Default for WithholdingStore {
    fn default() -> Self {
        Self {
            inner: crate::store::InMemoryTaskStore::new(),
            withheld: std::sync::Mutex::new(None),
        }
    }
}

impl WithholdingStore {
    /// Arms the next `n` reads of `id` to report it absent.
    pub(super) fn withhold(&self, id: &TaskId, n: usize) {
        *self.withheld.lock().expect("test mutex") = Some((id.clone(), n));
    }

    /// Consumes one withheld read of `id`, if one is armed.
    fn take_withheld(&self, id: &TaskId) -> bool {
        let mut armed = self.withheld.lock().expect("test mutex");
        match armed.as_mut() {
            Some((target, remaining)) if target == id && *remaining > 0 => {
                *remaining -= 1;
                true
            }
            _ => false,
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl crate::store::TaskStore for WithholdingStore {
    fn save<'a>(&'a self, t: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.save(t)
    }
    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        Box::pin(async move {
            if self.take_withheld(id) {
                return Ok(None);
            }
            self.inner.get(id).await
        })
    }
    fn list<'a>(
        &'a self,
        p: &'a a2a_protocol_types::params::ListTasksParams,
    ) -> Pin<
        Box<
            dyn Future<Output = A2aResult<a2a_protocol_types::responses::TaskListResponse>>
                + Send
                + 'a,
        >,
    > {
        self.inner.list(p)
    }
    fn insert_if_absent<'a>(
        &'a self,
        t: &'a Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        self.inner.insert_if_absent(t)
    }
    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.delete(id)
    }
    fn supports_idempotency(&self) -> bool {
        self.inner.supports_idempotency()
    }
    fn claim_idempotency_key<'a>(
        &'a self,
        key: &'a str,
        message_id: &'a MessageId,
        task_id: &'a TaskId,
    ) -> Pin<
        Box<dyn Future<Output = A2aResult<crate::store::task_store::IdempotencyClaim>> + Send + 'a>,
    > {
        self.inner.claim_idempotency_key(key, message_id, task_id)
    }
    fn release_idempotency_key<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        self.inner.release_idempotency_key(key)
    }
}

/// A counting handler over a caller-held [`WithholdingStore`].
pub(super) fn withholding_handler(
    store: &Arc<WithholdingStore>,
) -> (RequestHandler, Arc<AtomicUsize>) {
    let counter = Arc::new(AtomicUsize::new(0));
    let handler = RequestHandlerBuilder::new(CountingExecutor(Arc::clone(&counter)))
        .with_task_store_arc(Arc::clone(store) as Arc<dyn crate::store::TaskStore>)
        .build()
        .expect("withholding-store build should succeed");
    (handler, counter)
}
