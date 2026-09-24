// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The connection statistics the HTTP servers report through
//! [`Metrics::on_connection_pool_stats`] (audit O10).
//!
//! The callback, the [`ConnectionPoolStats`] it carries and the four
//! `a2a.server.pool.*` instruments the book catalogues all existed, and
//! nothing produced a report: no accept loop called it. This is the producer,
//! shared by [`serve`](super::serve), [`serve_with_addr`](super::serve_with_addr)
//! and [`Server::serve_with_shutdown`](super::Server::serve_with_shutdown).
//!
//! A connection is *active* while a request on it is in flight — until its
//! response body is finished or dropped, so a connection streaming SSE is
//! active for as long as the stream runs — and *idle* while it is open with
//! none. A report is sent when a connection opens or closes and when one
//! turns active or idle; each is a snapshot of counters that other
//! connections move concurrently.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use http_body_util::BodyExt as _;

use super::{DispatchResponse, Dispatcher};
use crate::metrics::{ConnectionPoolStats, Metrics};

/// Every connection one server has accepted, and where reports go.
pub struct Connections {
    metrics: Arc<dyn Metrics>,
    open: AtomicU32,
    active: AtomicU32,
    created: AtomicU64,
    failed: AtomicU64,
}

impl Connections {
    /// The tracker for a server in front of `dispatcher`, or `None` when the
    /// dispatcher has no handler and so no metrics to report to.
    pub fn for_dispatcher(dispatcher: &impl Dispatcher) -> Option<Arc<Self>> {
        dispatcher.request_handler().map(|handler| {
            Arc::new(Self {
                metrics: Arc::clone(&handler.metrics),
                open: AtomicU32::new(0),
                active: AtomicU32::new(0),
                created: AtomicU64::new(0),
                failed: AtomicU64::new(0),
            })
        })
    }

    /// Counts a newly accepted connection open.
    pub fn opened(self: &Arc<Self>) -> Connection {
        self.created.fetch_add(1, Ordering::Relaxed);
        self.open.fetch_add(1, Ordering::Relaxed);
        self.report();
        Connection {
            requests: Requests {
                all: Arc::clone(self),
                in_flight: Arc::new(AtomicU32::new(0)),
            },
            failed: false,
        }
    }

    fn report(&self) {
        let open = self.open.load(Ordering::Relaxed);
        let active = self.active.load(Ordering::Relaxed);
        self.metrics.on_connection_pool_stats(&ConnectionPoolStats {
            active_connections: active,
            idle_connections: open.saturating_sub(active),
            total_connections_created: self.created.load(Ordering::Relaxed),
            connections_closed: self.failed.load(Ordering::Relaxed),
        });
    }
}

/// One open connection; counted closed when dropped.
pub struct Connection {
    requests: Requests,
    failed: bool,
}

impl Connection {
    /// The handle the connection's service uses to count its requests.
    pub fn requests(&self) -> Requests {
        self.requests.clone()
    }

    /// Counts the connection closed; `errored` if it ended in an error or a
    /// timeout rather than an orderly close, which is what
    /// [`ConnectionPoolStats::connections_closed`] counts.
    pub fn close(mut self, errored: bool) {
        self.failed = errored;
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let all = &self.requests.all;
        if self.failed {
            all.failed.fetch_add(1, Ordering::Relaxed);
        }
        all.open.fetch_sub(1, Ordering::Relaxed);
        all.report();
    }
}

/// Counts the requests in flight on one connection.
#[derive(Clone)]
pub struct Requests {
    all: Arc<Connections>,
    in_flight: Arc<AtomicU32>,
}

impl Requests {
    /// Dispatches `req`, counting the connection active until the response is
    /// done with.
    ///
    /// A response of known size is done with when `dispatch` returns: its
    /// body is ready and hyper writes it at once. A streaming response — SSE,
    /// the case that matters — is done with when its body is finished or
    /// dropped, so the guard rides in the body, which hyper drops when it has
    /// written the last frame or the peer has gone. Only a streaming body is
    /// wrapped: the wrapper cannot report an exact size, and wrapping a sized
    /// body turned every `Content-Length` response into a chunked one,
    /// measured at +36% on a JSON-RPC send.
    pub async fn dispatch(
        self,
        dispatcher: &impl Dispatcher,
        req: hyper::Request<hyper::body::Incoming>,
    ) -> DispatchResponse {
        let guard = InFlight::start(self);
        let resp = dispatcher.dispatch(req).await;
        if hyper::body::Body::size_hint(resp.body()).exact().is_some() {
            return resp;
        }
        resp.map(|body| {
            body.map_frame(move |frame| {
                let _ = &guard;
                frame
            })
            .boxed()
        })
    }
}

/// One request in flight; the first on a connection turns it active, and the
/// last to finish turns it idle.
struct InFlight(Requests);

impl InFlight {
    fn start(requests: Requests) -> Self {
        if requests.in_flight.fetch_add(1, Ordering::AcqRel) == 0 {
            requests.all.active.fetch_add(1, Ordering::Relaxed);
            requests.all.report();
        }
        Self(requests)
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if self.0.in_flight.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.all.active.fetch_sub(1, Ordering::Relaxed);
            self.0.all.report();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Seen(Mutex<Vec<(u32, u32, u64, u64)>>);

    impl Metrics for Seen {
        fn on_connection_pool_stats(&self, s: &ConnectionPoolStats) {
            self.0.lock().unwrap().push((
                s.active_connections,
                s.idle_connections,
                s.total_connections_created,
                s.connections_closed,
            ));
        }
    }

    fn tracker(seen: &Arc<Seen>) -> Arc<Connections> {
        Arc::new(Connections {
            metrics: Arc::clone(seen) as Arc<dyn Metrics>,
            open: AtomicU32::new(0),
            active: AtomicU32::new(0),
            created: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        })
    }

    fn last(seen: &Seen) -> (u32, u32, u64, u64) {
        *seen.0.lock().unwrap().last().expect("a report")
    }

    #[test]
    fn a_connection_is_active_only_while_a_request_is_in_flight() {
        let seen = Arc::new(Seen::default());
        let all = tracker(&seen);
        let a = all.opened();
        let b = all.opened();
        assert_eq!(last(&seen), (0, 2, 2, 0), "two open, both idle");

        let first = InFlight::start(a.requests());
        assert_eq!(last(&seen), (1, 1, 2, 0));
        // A second request on the same connection does not count it twice.
        let second = InFlight::start(a.requests());
        assert_eq!(all.active.load(Ordering::Relaxed), 1);
        drop(first);
        assert_eq!(all.active.load(Ordering::Relaxed), 1, "one still in flight");
        drop(second);
        assert_eq!(last(&seen), (0, 2, 2, 0));

        a.close(false);
        assert_eq!(last(&seen), (0, 1, 2, 0), "an orderly close is not counted");
        b.close(true);
        assert_eq!(last(&seen), (0, 0, 2, 1), "an errored close is");
    }

    #[test]
    fn a_connection_dropped_without_close_counts_as_closed_cleanly() {
        let seen = Arc::new(Seen::default());
        let all = tracker(&seen);
        drop(all.opened());
        assert_eq!(last(&seen), (0, 0, 1, 0));
    }
}
