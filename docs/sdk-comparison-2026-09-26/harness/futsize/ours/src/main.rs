// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use a2a_protocol_sdk::prelude::*;
use std::sync::Arc;
struct Echo;
agent_executor!(Echo, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Completed).await
});
#[tokio::main]
async fn main() {
    let card = AgentCard::new("x", "1", AgentInterface::jsonrpc("http://127.0.0.1:1"));
    let h = Arc::new(RequestHandlerBuilder::new(Echo).with_agent_card(card).build().unwrap());
    let p = MessageSendParams::new(Message::user_text("m", "hello"));
    println!("a2a-rust on_send_message future: {} bytes", std::mem::size_of_val(&h.on_send_message(p, false, None)));
    println!("a2a-rust StreamResponse: {} bytes", std::mem::size_of::<StreamResponse>());
    let jr = Arc::new(JsonRpcDispatcher::new(Arc::clone(&h)));
    let rest = Arc::new(RestDispatcher::new(Arc::clone(&h)));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = l.accept().await.unwrap();
            let (jr, rest) = (Arc::clone(&jr), Arc::clone(&rest));
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let (jr, rest) = (Arc::clone(&jr), Arc::clone(&rest));
                    async move {
                        let resp = if req.uri().path() == "/" {
                            let f = jr.dispatch(req);
                            println!("a2a-rust JsonRpcDispatcher::dispatch future: {} bytes", std::mem::size_of_val(&f));
                            f.await
                        } else {
                            let f = rest.dispatch(req);
                            println!("a2a-rust RestDispatcher::dispatch future: {} bytes", std::mem::size_of_val(&f));
                            f.await
                        };
                        Ok::<_, std::convert::Infallible>(resp)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new().serve_connection(hyper_util::rt::TokioIo::new(s), svc).await;
            });
        }
    });
    for (path, body) in [("/", r#"{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m","role":"ROLE_USER","parts":[{"text":"hi"}]}}}"#),
                         ("/message:send", r#"{"message":{"messageId":"m2","role":"ROLE_USER","parts":[{"text":"hi"}]}}"#)] {
        let out = std::process::Command::new("curl").args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "-H", "content-type: application/json", "-H", "A2A-Version: 1.0", "-d", body, &format!("http://{addr}{path}")]).output().unwrap();
        println!("  ({path} -> HTTP {})", String::from_utf8_lossy(&out.stdout));
    }
}
