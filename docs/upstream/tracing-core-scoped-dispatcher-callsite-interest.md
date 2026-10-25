<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# tracing-core: a scoped subscriber misses an event whose callsite another thread reached first

**Target:** `tokio-rs/tracing`.
**Status:** draft, not sent. Not checked whether it is already tracked: a
search on 2026-10-24 found tokio-rs/tracing#2938 (a fix for a race in
tracing-core, issue #2743, between a callsite's first interest computation
and a subscriber's registration), which is a different window from the one
below; nothing that names `has_just_one`.
**Found by:** `push_delivery::tests::log_redaction::a_failed_or_timed_out_delivery_logs_the_origin_not_the_secret`
in this repository, which captured with `tracing::subscriber::with_default`
and missed a `warn!` about once in 20 parallel runs. See
`docs/handoff.md`, Lessons.

## What happens

With `tracing-core` 0.1.36 (`tracing` 0.1.44, `tracing-subscriber` 0.3.23), a
subscriber installed with `with_default` can miss an event that its own thread
emits, because another thread, which has no subscriber, reached the same
callsite first. Two threads and two barriers make it happen every time:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Default)]
struct Count(Arc<AtomicUsize>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Count {
    fn on_event(&self, _: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn emit() {
    tracing::warn!("one callsite");
}

fn main() {
    let count = Count::default();
    let subscriber = tracing_subscriber::registry().with(count.clone());
    let barrier = Arc::new(Barrier::new(2));
    let other = {
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait(); // the scoped subscriber exists and is registered
            emit(); // first reach of the callsite, on a thread with no subscriber
            barrier.wait();
        })
    };
    tracing::subscriber::with_default(subscriber, || {
        barrier.wait();
        barrier.wait();
        emit();
    });
    other.join().unwrap();
    println!("the scoped subscriber saw {} of 1 events", count.0.load(Ordering::SeqCst));
}
```

It prints `the scoped subscriber saw 0 of 1 events`. (Run on 2026-10-24 in a
fresh crate, `cargo run --offline`, Linux x86-64, Rust 1.99.1.)

## Why, from the source

In `tracing-core-0.1.36/src/callsite.rs`, `Dispatchers::register_dispatch`
sets `has_just_one` to `dispatchers.len() <= 1`, and `Dispatchers::rebuilder`
returns `Rebuilder::JustOne` while it is true. `Rebuilder::JustOne` computes a
callsite's interest with `dispatcher::get_default`, that is, from the
*calling thread's* default dispatcher. A scoped subscriber that is the only
one registered sets `has_just_one`, so a thread with no default that first
reaches a callsite computes the interest from "no subscriber", which is
"never", and that is cached for every thread. The scoped thread's own event
then never reaches `enabled`.

I did not read further than that, and did not test other versions.

## Impact

Test suites that capture logs with `with_default` while running tests in
parallel in one process lose events intermittently, and an assertion that a
line was *not* emitted passes for the wrong reason. Installing one global
subscriber for the life of the process avoids it (the second dispatcher
clears `has_just_one`, and a later `rebuild_interest_cache` repairs a callsite
cached before it).

## Not proposed

A fix. The fast path is an optimisation for the case where the one dispatcher
is the global default; whether a scoped dispatcher should clear it, or the
documentation of `with_default` should say that a callsite's interest is
global, is the maintainers' call.
