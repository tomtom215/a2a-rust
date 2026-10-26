// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A stand-in HTTP proxy that records every request it is asked to carry,
//! and carries it. Absolute-form requests are forwarded, `CONNECT` is
//! tunnelled, and every destination host is mapped to `127.0.0.1` — so an
//! endpoint no resolver knows (`collector.invalid`) is reachable only through
//! it, which is what makes "the export went through the proxy" something a
//! test can prove rather than assume.
//!
//! Like the collectors, it runs on a thread and runtime of its own.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt as _, Empty};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

/// One request the proxy was asked to carry.
#[derive(Clone, Debug)]
pub struct Proxied {
    /// `POST` for a forwarded export, `CONNECT` for a tunnel.
    pub method: String,
    /// The request target as the proxy received it: an absolute URI for a
    /// forward, `host:port` for a tunnel.
    pub target: String,
    /// Lower-cased header names.
    pub headers: HashMap<String, String>,
}

type Seen = Arc<Mutex<Vec<Proxied>>>;
type Body = BoxBody<Bytes, hyper::Error>;

pub struct ForwardProxy {
    addr: SocketAddr,
    seen: Seen,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl ForwardProxy {
    pub fn start() -> Self {
        let seen: Seen = Arc::default();
        let log = Arc::clone(&seen);
        let (stop, stopped) = oneshot::channel();
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("proxy runtime");
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind");
                addr_tx
                    .send(listener.local_addr().expect("addr"))
                    .expect("send");
                serve(listener, log, stopped).await;
            });
        });
        Self {
            addr: addr_rx.recv().expect("proxy address"),
            seen,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    /// The proxy's URL, as `HTTP_PROXY` would hold it.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The proxy's URL carrying a login in its userinfo.
    pub fn url_with_login(&self, user: &str, password: &str) -> String {
        format!("http://{user}:{password}@{}", self.addr)
    }

    pub fn seen(&self) -> Vec<Proxied> {
        self.seen.lock().expect("lock").clone()
    }

    /// The targets of every request of `method`, in order.
    pub fn targets(&self, method: &str) -> Vec<String> {
        self.seen()
            .into_iter()
            .filter(|p| p.method == method)
            .map(|p| p.target)
            .collect()
    }
}

impl Drop for ForwardProxy {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

async fn serve(listener: tokio::net::TcpListener, seen: Seen, mut stopped: oneshot::Receiver<()>) {
    loop {
        let (stream, _) = tokio::select! {
            accepted = listener.accept() => match accepted { Ok(a) => a, Err(_) => return },
            _ = &mut stopped => return,
        };
        let seen = Arc::clone(&seen);
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| handle(req, Arc::clone(&seen)));
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
                .await;
        });
    }
}

fn empty(status: StatusCode) -> Response<Body> {
    let mut r = Response::new(Empty::new().map_err(|never| match never {}).boxed());
    *r.status_mut() = status;
    r
}

async fn handle(req: Request<Incoming>, seen: Seen) -> Result<Response<Body>, Infallible> {
    seen.lock().expect("lock").push(Proxied {
        method: req.method().to_string(),
        target: req.uri().to_string(),
        headers: req
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
            .collect(),
    });
    let Some(port) = req.uri().port_u16().or_else(|| {
        // An absolute-form `http://host/...` with no port is port 80.
        req.uri().scheme_str().is_some().then_some(80)
    }) else {
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    if req.method() == Method::CONNECT {
        tokio::spawn(async move {
            if let Ok(upgraded) = hyper::upgrade::on(req).await
                && let Ok(mut upstream) = TcpStream::connect(("127.0.0.1", port)).await
            {
                let _ =
                    tokio::io::copy_bidirectional(&mut TokioIo::new(upgraded), &mut upstream).await;
            }
        });
        return Ok(empty(StatusCode::OK));
    }
    let path = req
        .uri()
        .path_and_query()
        .map_or_else(|| "/".to_owned(), ToString::to_string);
    let (parts, body) = req.into_parts();
    let mut forwarded = Request::builder()
        .method(parts.method)
        .uri(format!("http://127.0.0.1:{port}{path}"));
    for (name, value) in &parts.headers {
        if !name.as_str().starts_with("proxy-") {
            forwarded = forwarded.header(name, value);
        }
    }
    let Ok(forwarded) = forwarded.body(body) else {
        return Ok(empty(StatusCode::BAD_REQUEST));
    };
    let client =
        hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build_http::<Incoming>();
    Ok(match client.request(forwarded).await {
        Ok(response) => response.map(|b| b.boxed()),
        Err(_) => empty(StatusCode::BAD_GATEWAY),
    })
}
