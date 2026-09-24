// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `on_complete` contract, hook by hook: which interceptors are told,
//! in what order, with what outcome.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_types::error::{A2aError, A2aResult};

use super::{CallOutcome, ServerInterceptor, ServerInterceptorChain};
use crate::call_context::CallContext;
use crate::error::{ServerError, ServerResult};

type Log = Arc<Mutex<Vec<String>>>;

/// Records every hook it sees as `"<name>.<hook>"`, and `on_complete` with
/// its outcome.
struct Recorder {
    name: &'static str,
    log: Log,
    refuse: bool,
    fail_after: bool,
}

fn recorder(name: &'static str, log: &Log) -> Recorder {
    Recorder {
        name,
        log: Arc::clone(log),
        refuse: false,
        fail_after: false,
    }
}

impl Recorder {
    fn push(&self, entry: String) {
        self.log.lock().unwrap().push(entry);
    }
}

impl ServerInterceptor for Recorder {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.push(format!("{}.before", self.name));
            if self.refuse {
                return Err(A2aError::internal(format!("{} refused", self.name)));
            }
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.push(format!("{}.after", self.name));
            if self.fail_after {
                return Err(A2aError::internal(format!("{} after failed", self.name)));
            }
            Ok(())
        })
    }

    fn on_complete<'a>(
        &'a self,
        _ctx: &'a CallContext,
        outcome: CallOutcome<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        let outcome = match outcome {
            CallOutcome::Succeeded => "succeeded".to_owned(),
            CallOutcome::Failed(e) => format!("failed({})", e.to_a2a_error().message),
            CallOutcome::Cancelled => "cancelled".to_owned(),
        };
        Box::pin(async move { self.push(format!("{}.complete:{outcome}", self.name)) })
    }
}

fn chain_of(interceptors: Vec<Recorder>) -> ServerInterceptorChain {
    let mut chain = ServerInterceptorChain::new();
    for i in interceptors {
        chain.push(Arc::new(i));
    }
    chain
}

/// A call run as every `RequestHandler` method runs one.
async fn intercept<T>(
    chain: &ServerInterceptorChain,
    ctx: &CallContext,
    body: impl Future<Output = ServerResult<T>>,
) -> ServerResult<T> {
    let mut call = chain.begin(ctx);
    let result = async {
        call.before().await?;
        body.await
    }
    .await;
    call.finish(result).await
}

fn entries(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

#[tokio::test]
async fn a_successful_call_completes_every_interceptor_in_reverse_order() {
    let log = Log::default();
    let chain = chain_of(vec![recorder("a", &log), recorder("b", &log)]);
    let ctx = CallContext::new("GetTask");

    let body_log = Arc::clone(&log);
    let out = intercept(&chain, &ctx, async move {
        body_log.lock().unwrap().push("body".to_owned());
        Ok::<_, ServerError>(7)
    })
    .await;

    assert_eq!(out.unwrap(), 7);
    assert_eq!(
        entries(&log),
        [
            "a.before",
            "b.before",
            "body",
            "b.after",
            "a.after",
            "b.complete:succeeded",
            "a.complete:succeeded",
        ]
    );
}

#[tokio::test]
async fn a_refusing_before_completes_itself_and_those_before_it_only() {
    let log = Log::default();
    let mut b = recorder("b", &log);
    b.refuse = true;
    let chain = chain_of(vec![recorder("a", &log), b, recorder("c", &log)]);
    let ctx = CallContext::new("GetTask");

    let body_log = Arc::clone(&log);
    let out = intercept(&chain, &ctx, async move {
        body_log.lock().unwrap().push("body".to_owned());
        Ok::<_, ServerError>(())
    })
    .await;

    assert!(out.is_err());
    assert_eq!(
        entries(&log),
        [
            "a.before",
            "b.before",
            "b.complete:failed(b refused)",
            "a.complete:failed(b refused)",
        ],
        "the body must not run, `c` never entered so it is not completed, \
         and `after` runs on none"
    );
}

#[tokio::test]
async fn a_failing_body_completes_every_interceptor_with_its_error() {
    let log = Log::default();
    let chain = chain_of(vec![recorder("a", &log), recorder("b", &log)]);
    let ctx = CallContext::new("GetTask");

    let out: Result<(), _> = intercept(&chain, &ctx, async {
        Err(ServerError::Protocol(A2aError::internal("handler broke")))
    })
    .await;

    assert!(out.is_err());
    assert_eq!(
        entries(&log),
        [
            "a.before",
            "b.before",
            "b.complete:failed(handler broke)",
            "a.complete:failed(handler broke)",
        ]
    );
}

#[tokio::test]
async fn a_failing_after_is_the_outcome_every_interceptor_is_told() {
    let log = Log::default();
    let mut b = recorder("b", &log);
    b.fail_after = true;
    let chain = chain_of(vec![recorder("a", &log), b]);
    let ctx = CallContext::new("GetTask");

    let out = intercept(&chain, &ctx, async { Ok::<_, ServerError>(()) }).await;

    assert!(out.is_err());
    assert_eq!(
        entries(&log),
        [
            "a.before",
            "b.before",
            "b.after",
            "b.complete:failed(b after failed)",
            "a.complete:failed(b after failed)",
        ],
        "`b`'s after fails first (reverse order), so `a`'s after is not run"
    );
}

#[tokio::test]
async fn a_dropped_call_completes_every_entered_interceptor_as_cancelled() {
    let log = Log::default();
    let chain = Arc::new(chain_of(vec![recorder("a", &log), recorder("b", &log)]));
    let ctx = CallContext::new("SendMessage");

    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let call = {
        let chain = Arc::clone(&chain);
        tokio::spawn(async move {
            intercept(&chain, &ctx, async move {
                let _ = entered_tx.send(());
                std::future::pending::<ServerResult<()>>().await
            })
            .await
        })
    };
    entered_rx.await.expect("the body starts");
    call.abort();
    assert!(call.await.unwrap_err().is_cancelled());

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while entries(&log).len() < 4 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        entries(&log),
        [
            "a.before",
            "b.before",
            "b.complete:cancelled",
            "a.complete:cancelled",
        ]
    );
}

/// An `on_complete` that never finishes, then is dropped with the call: it
/// is not started again, and the interceptor before it is still told.
struct Stuck(Log);

impl ServerInterceptor for Stuck {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn on_complete<'a>(
        &'a self,
        _ctx: &'a CallContext,
        _outcome: CallOutcome<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            self.0.lock().unwrap().push("stuck.complete".to_owned());
            std::future::pending::<()>().await;
        })
    }
}

#[tokio::test]
async fn a_call_dropped_inside_on_complete_starts_no_hook_twice() {
    let log = Log::default();
    let mut chain = ServerInterceptorChain::new();
    chain.push(Arc::new(recorder("a", &log)));
    chain.push(Arc::new(Stuck(Arc::clone(&log))));
    let chain = Arc::new(chain);
    let ctx = CallContext::new("GetTask");

    let call = {
        let chain = Arc::clone(&chain);
        tokio::spawn(
            async move { intercept(&chain, &ctx, async { Ok::<_, ServerError>(()) }).await },
        )
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !entries(&log).iter().any(|e| e == "stuck.complete")
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    call.abort();
    assert!(call.await.unwrap_err().is_cancelled());

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while entries(&log).len() < 4 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // Let a wrongly re-started hook have its chance to show up.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        entries(&log),
        [
            "a.before",
            "a.after",
            "stuck.complete",
            "a.complete:cancelled",
        ],
        "the stuck hook is started once; `a`, not yet started when the call \
         was dropped, is told the call was cancelled — its response was \
         never returned"
    );
}

#[tokio::test]
async fn the_default_on_complete_does_nothing() {
    struct Plain;
    impl ServerInterceptor for Plain {
        fn before<'a>(
            &'a self,
            _ctx: &'a CallContext,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
        fn after<'a>(
            &'a self,
            _ctx: &'a CallContext,
        ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
    }
    let ctx = CallContext::new("GetTask");
    Plain.on_complete(&ctx, CallOutcome::Cancelled).await;
    let mut chain = ServerInterceptorChain::new();
    chain.push(Arc::new(Plain));
    assert_eq!(
        intercept(&chain, &ctx, async { Ok::<_, ServerError>(1) })
            .await
            .unwrap(),
        1
    );
}
