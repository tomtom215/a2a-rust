// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use a2a::event::StreamResponse;
use a2a::*;
use a2a_server::*;
use futures::stream::BoxStream;
struct Echo;
impl AgentExecutor for Echo {
    fn execute(&self, _c: ExecutorContext) -> BoxStream<'static, Result<StreamResponse, A2AError>> { Box::pin(futures::stream::empty()) }
    fn cancel(&self, _c: ExecutorContext) -> BoxStream<'static, Result<StreamResponse, A2AError>> { Box::pin(futures::stream::empty()) }
}
#[tokio::main]
async fn main() {
    let h = DefaultRequestHandler::new(Echo, InMemoryTaskStore::new());
    let req = SendMessageRequest { message: Message::new(Role::User, vec![Part::text("hello")]), configuration: None, metadata: None, tenant: None };
    let params = ServiceParams::default();
    let f = h.send_message(&params, req);
    println!("a2a-rs send_message future (outer, boxed by async_trait): {} bytes", std::mem::size_of_val(&f));
    println!("a2a-rs send_message future (inner heap allocation): {} bytes", std::mem::size_of_val(&*f));
    println!("a2a-rs StreamResponse: {} bytes", std::mem::size_of::<StreamResponse>());
}
