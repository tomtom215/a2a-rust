// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Claims: state validation, executor ergonomics (agent_executor!, boxed_future),
//! client CallInterceptor, on_complete timing for streams, REST path traversal
//! and query-length limits, undeclared input modes refused. Ports 7950-7999.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use a2a_protocol_sdk::client::{CallInterceptor, ClientRequest, ClientResponse};
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::EventQueueWriter;
use claims_suite::common::*;
use serde_json::json;

fn port() -> u16 {
    port_in(7960, 40)
}

// Executor that tries an illegal transition: Completed -> Working.
struct Rogue;
impl AgentExecutor for Rogue {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, q: &'a dyn EventQueueWriter) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        boxed_future(async move {
            let e = EventEmitter::new(ctx, q);
            e.status(TaskState::Working).await?;
            e.status(TaskState::Completed).await?;
            let r = e.status(TaskState::Working).await;
            println!("executor: emitting Working after Completed -> {:?}", r.as_ref().map_err(|e| e.to_string()));
            Ok(())
        })
    }
}

// Macro form
struct Macro;
agent_executor!(Macro, |ctx, queue| async {
    let e = EventEmitter::new(ctx, queue);
    e.status(TaskState::Completed).await?;
    Ok(())
});

#[tokio::test(flavor = "multi_thread")]
async fn state_validation_and_ergonomics() {
    use TaskState::*;
    println!("can_transition_to: Working->Completed={} Completed->Working={} Submitted->Working={} Failed->Completed={}",
        Working.can_transition_to(Completed), Completed.can_transition_to(Working), Submitted.can_transition_to(Working), Failed.can_transition_to(Completed));
    assert!(Working.can_transition_to(Completed));
    assert!(!Completed.can_transition_to(Working));
    assert!(!Failed.can_transition_to(Completed));

    let h = Arc::new(RequestHandlerBuilder::new(Rogue).build().unwrap());
    let p = port();
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap();
    let t = task_of(c.send_message(msg(&uid(), "x")).await.unwrap());
    tokio::time::sleep(Duration::from_millis(200)).await;
    let g = c.get_task(TaskQueryParams::new(t.id.to_string())).await.unwrap();
    println!("server: returned state={:?}, stored state={:?}", t.status.state, g.status.state);
    assert_eq!(g.status.state, Completed);

    let h = Arc::new(RequestHandlerBuilder::new(Macro).build().unwrap());
    let p = port();
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).build().unwrap();
    let t = task_of(c.send_message(msg(&uid(), "x")).await.unwrap());
    assert_eq!(t.status.state, Completed);
}

struct AddRequestId;
impl CallInterceptor for AddRequestId {
    fn before<'a>(&'a self, req: &'a mut ClientRequest) -> impl Future<Output = ClientResult<()>> + Send + 'a {
        async move {
            req.extra_headers.insert("X-Request-ID".into(), "from-client-interceptor".into());
            Ok(())
        }
    }
    fn after<'a>(&'a self, _resp: &'a ClientResponse) -> impl Future<Output = ClientResult<()>> + Send + 'a {
        async { Ok(()) }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn client_interceptor_and_stream_on_complete_timing() {
    let (agent, probe) = CtlAgent::new();
    let rec = RecInterceptor::default();
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_interceptor(rec.clone()).build().unwrap());
    let p = port();
    serve_with_addr(format!("127.0.0.1:{p}"), JsonRpcDispatcher::new(h)).await.unwrap();
    let c = ClientBuilder::new(format!("http://127.0.0.1:{p}")).with_interceptor(AddRequestId).build().unwrap();
    c.send_message(msg(&uid(), "x")).await.unwrap();
    let ids = probe.request_ids.lock().unwrap().clone();
    println!("server saw request ids via client CallInterceptor: {ids:?}");
    assert_eq!(ids, vec![Some("from-client-interceptor".to_string())]);

    // When does on_complete fire for a stream that lasts ~2 s?
    let t0 = Instant::now();
    let mut s = c.stream_message(msg(&uid(), "chunks:4:500")).await.unwrap();
    let _first = s.next().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let early = rec.events().iter().any(|e| e.starts_with("complete:SendStreamingMessage"));
    while s.next().await.is_some() {}
    println!("stream lasted {:?}; on_complete already fired right after first event: {early}; events={:?}", t0.elapsed(), rec.events());
}

#[tokio::test(flavor = "multi_thread")]
async fn rest_traversal_query_limit_and_input_modes() {
    let (agent, _) = CtlAgent::new();
    let p = port();
    let card = AgentCard::new("m", "1.0.0", AgentInterface::rest(format!("http://127.0.0.1:{p}")))
        .with_input_modes(["text/plain"]);
    let h = Arc::new(RequestHandlerBuilder::new(agent).with_agent_card(card).build().unwrap());
    serve_with_addr(format!("127.0.0.1:{p}"), RestDispatcher::new(h)).await.unwrap();
    let rc = reqwest::Client::new();
    let tr = std::process::Command::new("curl").args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--path-as-is", "-H", "A2A-Version: 1.0", &format!("http://127.0.0.1:{p}/tasks/%2e%2e/%2e%2e/etc/passwd")]).output().unwrap();
    let tr = String::from_utf8_lossy(&tr.stdout).to_string();
    let q = rc.get(format!("http://127.0.0.1:{p}/tasks?x={}", "a".repeat(5000))).header("A2A-Version", "1.0").send().await.unwrap().status().as_u16();
    let body = json!({"message":{"messageId": uid(), "role":"ROLE_USER","parts":[{"raw":"iVBORw0KGgo=","mediaType":"image/png"}]}});
    let r = rc.post(format!("http://127.0.0.1:{p}/message:send")).header("content-type", "application/json").header("A2A-Version", "1.0").json(&body).send().await.unwrap();
    let rs = r.status().as_u16();
    let rb = r.text().await.unwrap();
    println!("path traversal %2e%2e -> {tr}; 5000-byte query -> {q}; image/png part vs card text/plain -> {rs} {}", &rb[..rb.len().min(160)]);
    assert_eq!(tr, "400");
    assert_eq!(q, 414);
    assert!(rb.contains("CONTENT_TYPE_NOT_SUPPORTED") || rb.to_lowercase().contains("media"));
}
