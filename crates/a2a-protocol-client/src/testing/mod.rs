// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Scripted hostile peers, for testing whatever calls an agent.
//!
//! A coordinator that delegates has to survive the worker it calls: one that
//! stalls mid-stream, drops the connection before the final event, sends a
//! frame that does not parse, or refuses the caller's credentials. Those are
//! the failures a real network produces and a well-behaved test server never
//! does, so they go untested unless something produces them on purpose.
//!
//! [`ScriptedPeer`] is that something. It listens on a loopback port, speaks
//! one binding — JSON-RPC, HTTP+JSON, WebSocket (feature `websocket`) or gRPC
//! (feature `grpc`) — and answers every call according to one script:
//!
//! | Script | Streaming calls | Unary calls |
//! |---|---|---|
//! | [`stall_after(n)`](ScriptedPeer::stall_after) | `n` events, then silence with the connection held open | no answer, connection held open |
//! | [`cut_off_after(n)`](ScriptedPeer::cut_off_after) | `n` events, then the connection or stream is dropped | the connection is dropped |
//! | [`misframe_after(n)`](ScriptedPeer::misframe_after) | `n` events, then one that does not parse, then silence | a body that does not parse |
//! | [`unauthorized()`](ScriptedPeer::unauthorized) | refused: HTTP 401, gRPC `UNAUTHENTICATED`, or a WebSocket handshake answered 401 | the same |
//!
//! Every event is a `TASK_STATE_WORKING` status update for task `t`, so no
//! stream ever reaches a terminal state on its own: what the caller sees next
//! is decided by the script alone.
//!
//! ```no_run
//! use a2a_protocol_client::testing::{Binding, ScriptedPeer};
//!
//! # async fn example() -> std::io::Result<()> {
//! let peer = ScriptedPeer::new().stall_after(1).on(Binding::Rest).start().await?;
//! // Point the code under test at `peer.url()`, and assert it gives up.
//! # let _ = peer.url();
//! # Ok(())
//! # }
//! ```
//!
//! This repository's own client tests use it (`tests/scripted_peer_tests.rs`),
//! which is how it is known to produce each failure on each binding.
//! Enabled by the `testing` feature; it is for tests, and nothing in a
//! production build should depend on it.

mod http1;

#[cfg(feature = "grpc")]
mod grpc;
#[cfg(feature = "websocket")]
mod ws;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The binding a [`ScriptedPeer`] speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Binding {
    /// JSON-RPC 2.0 over HTTP/1.1, streaming as server-sent events.
    JsonRpc,
    /// HTTP+JSON (the REST binding), streaming as server-sent events.
    Rest,
    /// JSON-RPC 2.0 frames over a WebSocket.
    #[cfg(feature = "websocket")]
    WebSocket,
    /// `lf.a2a.v1.A2AService` over HTTP/2 gRPC.
    #[cfg(feature = "grpc")]
    Grpc,
}

/// What a [`ScriptedPeer`] does with each call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Script {
    /// Send this many events, then fall silent and hold the connection.
    Stall {
        /// Events sent first.
        after: usize,
    },
    /// Send this many events, then drop the connection or stream.
    CutOff {
        /// Events sent first.
        after: usize,
    },
    /// Send this many events, then one that does not parse, then fall silent.
    MisFrame {
        /// Events sent first.
        after: usize,
    },
    /// Refuse every call as unauthenticated.
    Unauthorized,
}

/// A hostile A2A peer on a loopback port. See the [module docs](self).
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct ScriptedPeer {
    binding: Binding,
    script: Script,
}

impl Default for ScriptedPeer {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptedPeer {
    /// A JSON-RPC peer that stalls after one event; the methods below change
    /// either half.
    pub const fn new() -> Self {
        Self {
            binding: Binding::JsonRpc,
            script: Script::Stall { after: 1 },
        }
    }

    /// The binding to speak.
    pub const fn on(mut self, binding: Binding) -> Self {
        self.binding = binding;
        self
    }

    /// Send `events`, then fall silent with the connection held open.
    pub const fn stall_after(mut self, events: usize) -> Self {
        self.script = Script::Stall { after: events };
        self
    }

    /// Send `events`, then drop the connection (or, on gRPC, reset the stream).
    pub const fn cut_off_after(mut self, events: usize) -> Self {
        self.script = Script::CutOff { after: events };
        self
    }

    /// Send `events`, then one that does not parse, then fall silent.
    pub const fn misframe_after(mut self, events: usize) -> Self {
        self.script = Script::MisFrame { after: events };
        self
    }

    /// Refuse every call as unauthenticated.
    pub const fn unauthorized(mut self) -> Self {
        self.script = Script::Unauthorized;
        self
    }

    /// Binds `127.0.0.1:0` and starts answering.
    ///
    /// # Errors
    ///
    /// Returns the error from binding the listener.
    pub async fn start(self) -> std::io::Result<RunningPeer> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&accepted);
        let script = self.script;
        let binding = self.binding;
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = stream.set_nodelay(true);
                tokio::spawn(serve(binding, script, stream));
            }
        });
        let url = match binding {
            #[cfg(feature = "websocket")]
            Binding::WebSocket => format!("ws://{addr}"),
            _ => format!("http://{addr}"),
        };
        Ok(RunningPeer {
            url,
            accepted,
            task,
        })
    }
}

async fn serve(binding: Binding, script: Script, stream: tokio::net::TcpStream) {
    match binding {
        Binding::JsonRpc => http1::serve(http1::Flavour::JsonRpc, script, stream).await,
        Binding::Rest => http1::serve(http1::Flavour::Rest, script, stream).await,
        #[cfg(feature = "websocket")]
        Binding::WebSocket => ws::serve(script, stream).await,
        #[cfg(feature = "grpc")]
        Binding::Grpc => grpc::serve(script, stream).await,
    }
}

/// A started [`ScriptedPeer`]. Dropping it stops the listener; connections
/// already accepted keep whatever state their script left them in until the
/// runtime ends.
#[derive(Debug)]
pub struct RunningPeer {
    url: String,
    accepted: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl RunningPeer {
    /// The URL to call: `http://127.0.0.1:PORT`, or `ws://…` for WebSocket.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// How many connections the peer has accepted.
    #[must_use]
    pub fn connections(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

impl Drop for RunningPeer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The one event every script sends: task `t` is working.
fn working_event() -> serde_json::Value {
    serde_json::json!({
        "statusUpdate": {
            "taskId": "t",
            "contextId": "c",
            "status": { "state": "TASK_STATE_WORKING" }
        }
    })
}

/// Holds the current task forever: the "silence" every stalling script ends
/// in. The connection it owns stays open because the task never returns.
async fn hold_open() {
    std::future::pending::<()>().await;
}
