// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The future a call runs as inside its span: it records the call when it
//! finishes, or as cancelled when it is dropped first.

use std::borrow::Cow;
use std::future::Future;
use std::time::Instant;

use super::{CANCELLED, RpcSystem, ServerSpan};

/// The future inside a call's span, as the call's future is wrapped when
/// tracing is compiled in.
#[cfg(feature = "tracing")]
type Inner<F> = tracing::instrument::Instrumented<tokio::task::futures::TaskLocalFuture<bool, F>>;
#[cfg(not(feature = "tracing"))]
type Inner<F> = F;

pin_project_lite::pin_project! {
    /// A call running in its span; when it finishes, it is recorded and its
    /// outcome handed on. See [`ServerSpan::run`].
    pub struct Call<F, C, T>
    where
        F: Future,
    {
        #[pin]
        pub(super) inner: Inner<F>,
        pub(super) pending: Pending,
        pub(super) finish: Option<(C, T)>,
    }
}

impl<F, C, T, R> Future for Call<F, C, T>
where
    F: Future,
    C: FnOnce(&F::Output, RpcSystem) -> Option<Cow<'static, str>>,
    T: FnOnce(F::Output) -> R,
{
    type Output = R;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<R> {
        let this = self.project();
        let out = std::task::ready!(this.inner.poll(cx));
        // Both are taken only here, on the poll that returns `Ready`, so they
        // are missing only if the future is polled again after completing —
        // which `Future`'s contract lets an implementation panic on, and
        // which every `async` block does. No caller in this crate re-polls.
        let (classify, then) = this.finish.take().expect("a call polled after it finished");
        let call = this
            .pending
            .call
            .take()
            .expect("a call polled after it finished");
        let status = classify(&out, call.system);
        call.finish(this.pending.started.elapsed(), status.as_deref());
        call.enter(|| then(out)).into()
    }
}

/// A call in flight; records it as cancelled if dropped unfinished.
pub(super) struct Pending {
    pub(super) call: Option<ServerSpan>,
    pub(super) started: Instant,
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(call) = self.call.take() {
            call.finish(self.started.elapsed(), Some(CANCELLED));
        }
    }
}
