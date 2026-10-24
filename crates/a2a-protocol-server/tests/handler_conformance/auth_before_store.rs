// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Spec §13.1: "Servers MUST authorize every request" before acting on
//! it. The ordering was right where read (admission, then the store), but
//! nothing would fail if a handler read a store before admitting the call:
//! the existing tests show a refused call wrote nothing and that an
//! out-of-scope task answers like an unknown one, not that nothing was
//! read.
//!
//! Both stores here count every call. Every operation in `Method::ALL` is
//! refused by an interceptor, through the handler (what gRPC and WebSocket
//! call) and through both HTTP bindings, naming a task and a config that
//! exist; neither store may have been touched. The control shows the
//! counters move when the same calls are admitted.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::dispatch::{JsonRpcDispatcher, RestDispatcher};
use a2a_protocol_server::serve::serve_with_addr;
use a2a_protocol_server::{
    EventEmitter, HttpPushSender, InMemoryPushConfigStore, InMemoryTaskStore, PushConfigStore,
    RequestHandler, RequestHandlerBuilder, ServerInterceptor, TaskStore, agent_executor,
};
use a2a_protocol_types::TaskState;
use a2a_protocol_types::error::{A2aError, A2aResult, ErrorCode};
use a2a_protocol_types::method::Method;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};

use crate::required_extensions::{call, route};
use crate::{card, exchange, headers, jsonrpc};

type Fut<'a, T> = Pin<Box<dyn Future<Output = A2aResult<T>> + Send + 'a>>;

/// Every store call, counted.
#[derive(Clone, Default)]
struct Touches(Arc<AtomicUsize>);

impl Touches {
    fn hit(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn get(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

struct CountingTasks(InMemoryTaskStore, Touches);

impl TaskStore for CountingTasks {
    fn save<'a>(&'a self, task: &'a Task) -> Fut<'a, ()> {
        self.1.hit();
        self.0.save(task)
    }
    fn get<'a>(&'a self, id: &'a TaskId) -> Fut<'a, Option<Task>> {
        self.1.hit();
        self.0.get(id)
    }
    fn list<'a>(&'a self, params: &'a ListTasksParams) -> Fut<'a, TaskListResponse> {
        self.1.hit();
        self.0.list(params)
    }
    fn insert_if_absent<'a>(&'a self, task: &'a Task) -> Fut<'a, bool> {
        self.1.hit();
        self.0.insert_if_absent(task)
    }
    fn delete<'a>(&'a self, id: &'a TaskId) -> Fut<'a, ()> {
        self.1.hit();
        self.0.delete(id)
    }
}

struct CountingConfigs(InMemoryPushConfigStore, Touches);

impl PushConfigStore for CountingConfigs {
    fn set<'a>(
        &'a self,
        config: TaskPushNotificationConfig,
    ) -> Fut<'a, TaskPushNotificationConfig> {
        self.1.hit();
        self.0.set(config)
    }
    fn get<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Fut<'a, Option<TaskPushNotificationConfig>> {
        self.1.hit();
        self.0.get(task_id, id)
    }
    fn list<'a>(&'a self, task_id: &'a str) -> Fut<'a, Vec<TaskPushNotificationConfig>> {
        self.1.hit();
        self.0.list(task_id)
    }
    fn delete<'a>(&'a self, task_id: &'a str, id: &'a str) -> Fut<'a, ()> {
        self.1.hit();
        self.0.delete(task_id, id)
    }
    fn count(&self) -> Fut<'_, Option<usize>> {
        self.1.hit();
        self.0.count()
    }
}

/// Admits `Bearer good`, refuses anything else.
struct Gate;

impl ServerInterceptor for Gate {
    fn before<'a>(&'a self, ctx: &'a CallContext) -> Fut<'a, ()> {
        let admitted =
            ctx.http_headers().get("authorization").map(String::as_str) == Some("Bearer good");
        Box::pin(async move {
            if admitted {
                Ok(())
            } else {
                Err(A2aError::unauthenticated(
                    "not authenticated",
                    "Bearer realm=\"a2a\"",
                ))
            }
        })
    }
    fn after<'a>(&'a self, _ctx: &'a CallContext) -> Fut<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

struct Done;
agent_executor!(Done, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    emit.status(TaskState::Completed).await
});

/// The handler, its two store counters, and a task and config that exist.
async fn setup() -> (Arc<RequestHandler>, Touches, Touches, String, String) {
    let (tasks, configs) = (Touches::default(), Touches::default());
    let handler = Arc::new(
        RequestHandlerBuilder::new(Done)
            .with_agent_card(card(&[]))
            .with_extended_agent_card(card(&[]))
            .allow_unauthenticated_extended_card()
            .with_push_sender(HttpPushSender::new())
            .with_task_store(CountingTasks(InMemoryTaskStore::new(), tasks.clone()))
            .with_push_config_store(CountingConfigs(
                InMemoryPushConfigStore::new(),
                configs.clone(),
            ))
            .with_interceptor(Gate)
            .build()
            .expect("handler"),
    );
    let good = headers(&[("Authorization", "Bearer good")]);
    let task = match handler
        .on_send_message(
            a2a_protocol_types::params::MessageSendParams::new(
                a2a_protocol_types::Message::user_text("seed", "hi"),
            ),
            false,
            Some(&good),
        )
        .await
        .expect("seed")
    {
        a2a_protocol_server::handler::SendMessageResult::Response(
            a2a_protocol_types::responses::SendMessageResponse::Task(t),
        ) => t.id.0,
        other => panic!("expected a task, got {other:?}"),
    };
    let config = handler
        .on_set_push_config(
            TaskPushNotificationConfig::new(task.as_str(), "https://hook.example.com/a2a"),
            Some(&good),
        )
        .await
        .expect("config")
        .id
        .expect("config id");
    (handler, tasks, configs, task, config)
}

#[tokio::test]
async fn a_refused_call_reads_no_store_through_the_handler() {
    let (handler, tasks, configs, task, config) = setup().await;
    let bad = headers(&[("Authorization", "Bearer bad")]);
    for (n, method) in Method::ALL.iter().copied().enumerate() {
        let before = (tasks.get(), configs.get());
        for h in [None, Some(&bad)] {
            let err = call(&handler, method, h, &task, &config, n)
                .await
                .expect_err("refused");
            // The gate's refusal, not anything the handler decided.
            assert_eq!(err.code, ErrorCode::InvalidRequest, "{method:?}: {err:?}");
            assert_eq!(err.message, "not authenticated", "{method:?}: {err:?}");
        }
        assert_eq!(
            (tasks.get(), configs.get()),
            before,
            "{method:?} touched a store"
        );
    }

    // The control: admitted, the same calls read the stores.
    let good = headers(&[("Authorization", "Bearer good")]);
    let before = (tasks.get(), configs.get());
    for (n, method) in Method::ALL.iter().copied().enumerate() {
        let _ = call(&handler, method, Some(&good), &task, &config, n).await;
    }
    assert!(tasks.get() > before.0, "the task counter counts");
    assert!(configs.get() > before.1, "the config counter counts");
}

#[tokio::test]
async fn a_refused_call_reads_no_store_on_either_http_binding() {
    let (handler, tasks, configs, task, config) = setup().await;
    let rpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("JSON-RPC");
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("HTTP+JSON");
    // JSON-RPC carries a refused credential as Invalid Request.
    let unauthenticated = i64::from(ErrorCode::InvalidRequest.as_i32());
    for method in Method::ALL.iter().copied() {
        let (params, verb, path, body) = route(method, &task, &config);
        for extra in [&[][..], &[("Authorization", "Bearer bad")][..]] {
            let before = (tasks.get(), configs.get());
            let answer = jsonrpc(rpc, method.wire_name(), &params, extra).await;
            assert_eq!(
                answer.json()["error"]["code"].as_i64(),
                Some(unauthenticated),
                "JSON-RPC {method:?}: {answer:?}"
            );
            let answer = exchange(rest, verb, &path, body.as_ref(), extra).await;
            assert_eq!(answer.status, 401, "HTTP+JSON {method:?}: {answer:?}");
            assert_eq!(
                (tasks.get(), configs.get()),
                before,
                "{method:?} touched a store"
            );
        }
    }
}
