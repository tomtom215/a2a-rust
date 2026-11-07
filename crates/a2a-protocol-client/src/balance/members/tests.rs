// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The member set and its refresh, against a scripted source under a paused
//! clock.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Factory, Members, next_pick};
use crate::balance::{Endpoint, EndpointSource, EndpointsFuture};
use crate::error::{ClientError, ClientResult};
use crate::origin::Origins;
use crate::transport::{JsonRpcTransport, Transport};

const EVERY: Duration = Duration::from_secs(10);

/// Answers with the next of `answers` each time it is asked.
struct Script {
    answers: Mutex<Vec<ClientResult<Vec<Endpoint>>>>,
    asked: AtomicUsize,
}

impl EndpointSource for Arc<Script> {
    fn endpoints(&self) -> EndpointsFuture<'_> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let next = self.answers.lock().unwrap().remove(0);
        Box::pin(async move { next })
    }

    fn refresh_every(&self) -> Option<Duration> {
        Some(EVERY)
    }
}

fn ep(name: &str) -> Endpoint {
    Endpoint::new(format!("http://{name}.test"))
}

/// Members over `answers`, and how many times the factory built one.
fn members(
    answers: Vec<ClientResult<Vec<Endpoint>>>,
) -> (Arc<Members>, Arc<Script>, Arc<AtomicUsize>) {
    let script = Arc::new(Script {
        answers: Mutex::new(answers),
        asked: AtomicUsize::new(0),
    });
    let built = Arc::new(AtomicUsize::new(0));
    let origins = Arc::new(Origins::new(None, None, None));
    let count = Arc::clone(&built);
    let factory: Factory = Arc::new(move |endpoint: &Endpoint| {
        count.fetch_add(1, Ordering::SeqCst);
        let transport: Box<dyn Transport> = Box::new(JsonRpcTransport::new(endpoint.url())?);
        Ok((transport, origins.get(endpoint.url())))
    });
    (
        Arc::new(Members::new(Arc::new(Arc::clone(&script)), factory)),
        script,
        built,
    )
}

fn urls(set: &super::Set) -> Vec<String> {
    set.iter().map(|m| m.endpoint.url().to_owned()).collect()
}

#[tokio::test(start_paused = true)]
async fn a_repeated_endpoint_is_one_member() {
    let (m, _, built) = members(vec![Ok(vec![ep("a"), ep("a"), ep("b")])]);
    assert_eq!(
        urls(&m.get().await.unwrap()),
        ["http://a.test", "http://b.test"]
    );
    assert_eq!(built.load(Ordering::SeqCst), 2);
}

/// An endpoint still in the set keeps its member, and with it its state; a
/// new one is built, and one gone is dropped.
#[tokio::test(start_paused = true)]
async fn a_refresh_keeps_the_members_still_there() {
    let (m, _, built) = members(vec![Ok(vec![ep("a"), ep("b")]), Ok(vec![ep("b"), ep("c")])]);
    let first = m.get().await.unwrap();
    let second = m.refresh().await.unwrap();
    assert_eq!(urls(&second), ["http://b.test", "http://c.test"]);
    assert!(Arc::ptr_eq(&first[1], &second[0]), "b is the same member");
    assert_eq!(built.load(Ordering::SeqCst), 3);
}

/// A failed or empty answer keeps the set there was; with no set yet, it is
/// the error.
#[tokio::test(start_paused = true)]
async fn a_bad_answer_keeps_the_set() {
    let (m, _, _) = members(vec![
        Err(ClientError::HttpClient("down".into())),
        Ok(vec![ep("a")]),
        Err(ClientError::HttpClient("down".into())),
        Ok(Vec::new()),
    ]);
    let err = m.get().await.expect_err("no set yet");
    assert!(
        matches!(&err, ClientError::HttpClient(t) if t == "down"),
        "{err:?}"
    );
    assert_eq!(urls(&m.refresh().await.unwrap()), ["http://a.test"]);
    assert_eq!(urls(&m.refresh().await.unwrap()), ["http://a.test"]);
    assert_eq!(urls(&m.refresh().await.unwrap()), ["http://a.test"]);
    let (empty, _, _) = members(vec![Ok(Vec::new())]);
    let err = empty.get().await.expect_err("nothing to keep");
    assert!(
        matches!(&err, ClientError::Config(t) if t.contains("no endpoints")),
        "{err:?}"
    );
}

/// Due exactly once the interval has passed, and refreshed once however
/// many callers see it due.
#[tokio::test(start_paused = true)]
async fn one_refresh_when_due() {
    let (m, script, _) = members(vec![
        Ok(vec![ep("a")]),
        Ok(vec![ep("b")]),
        Ok(vec![ep("c")]),
    ]);
    let asked = || script.asked.load(Ordering::SeqCst);
    m.get().await.unwrap();
    tokio::time::advance(Duration::from_nanos(9_999_999_999)).await;
    m.get().await.unwrap();
    tokio::task::yield_now().await;
    assert_eq!(asked(), 1, "not due yet");
    tokio::time::advance(Duration::from_nanos(1)).await;
    // Three callers see it due before the refresh has run.
    for _ in 0..3 {
        assert_eq!(urls(&m.get().await.unwrap()), ["http://a.test"]);
    }
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(asked(), 2, "one refresh");
    assert_eq!(urls(&m.current()), ["http://b.test"]);
}

#[test]
fn picks_count_up() {
    let counter = AtomicU64::new(0);
    assert_eq!((next_pick(&counter), next_pick(&counter)), (1, 2));
}

/// An endpoint that cannot be built is left out and the log says so, once; a
/// set in which every endpoint builds says nothing. The warning is the only
/// way an operator learns the set is smaller than the source gave.
#[cfg(feature = "tracing")]
#[test]
fn an_endpoint_that_cannot_be_built_is_skipped_with_a_warning() {
    let refresh = |endpoints: Vec<Endpoint>| {
        let (m, _, _) = members(vec![Ok(endpoints)]);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .expect("runtime");
        let mut got = Vec::new();
        let warnings = crate::test_tracing::warnings_in(|| {
            got = urls(&rt.block_on(m.get()).expect("a set"));
        });
        (warnings, got)
    };
    let (warnings, got) = refresh(vec![ep("a"), Endpoint::new("not a url")]);
    assert_eq!(got, ["http://a.test"]);
    assert_eq!(warnings, 1, "the endpoint left out is warned about");
    let (warnings, got) = refresh(vec![ep("a"), ep("b")]);
    assert_eq!(got, ["http://a.test", "http://b.test"]);
    assert_eq!(warnings, 0, "nothing was left out");
}
