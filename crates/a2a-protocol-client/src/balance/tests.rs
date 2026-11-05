// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The balancer against in-memory endpoints under a paused clock: selection,
//! hedging to the millisecond, and what is never hedged. The same against
//! real sockets is `tests/balancing/`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;

use super::members::Factory;
use super::selection::{choose, max_out};
use super::{BalancePolicy, BalancedTransport, Ejection, Endpoint, Hedge, StaticEndpoints};
use crate::error::{ClientError, ClientResult};
use crate::origin::{Origins, origin_key};
use crate::retry::RetryBudget;
use crate::streaming::EventStream;
use crate::transport::Transport;

const MS: Duration = Duration::from_millis(1);

/// What a fake endpoint does with a call.
#[derive(Clone, Debug)]
enum Act {
    /// Answers with its name after this long.
    After(Duration),
    /// Fails with a `503` after this long.
    FailAfter(Duration),
}

struct Fake {
    name: String,
    act: Arc<Mutex<Act>>,
    calls: Arc<AtomicUsize>,
}

impl Transport for Fake {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let act = self.act.lock().unwrap().clone();
        let name = self.name.clone();
        Box::pin(async move {
            match act {
                Act::After(d) => {
                    tokio::time::sleep(d).await;
                    Ok(serde_json::Value::String(name))
                }
                Act::FailAfter(d) => {
                    tokio::time::sleep(d).await;
                    Err(ClientError::UnexpectedStatus {
                        status: 503,
                        body: name,
                        retry_after: None,
                    })
                }
            }
        })
    }

    fn send_streaming_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(EventStream::from_event_channel(rx))
        })
    }
}

/// Fake endpoints `a`, `b`, ...; each with its act and call count.
struct Fleet {
    acts: Vec<Arc<Mutex<Act>>>,
    calls: Vec<Arc<AtomicUsize>>,
    origins: Arc<Origins>,
}

impl Fleet {
    fn new(acts: &[Act], ejection: Option<Ejection>) -> Self {
        Self {
            acts: acts
                .iter()
                .map(|a| Arc::new(Mutex::new(a.clone())))
                .collect(),
            calls: acts.iter().map(|_| Arc::new(AtomicUsize::new(0))).collect(),
            origins: Arc::new(Origins::new(ejection, None, None)),
        }
    }

    fn url(i: usize) -> String {
        format!(
            "http://{}.test",
            char::from(b'a' + u8::try_from(i).unwrap())
        )
    }

    fn factory(&self) -> Factory {
        let acts = self.acts.clone();
        let calls = self.calls.clone();
        let origins = Arc::clone(&self.origins);
        Arc::new(move |endpoint: &Endpoint| {
            let i = (0..acts.len())
                .find(|&i| Self::url(i) == endpoint.url())
                .expect("a fleet endpoint");
            let transport: Box<dyn Transport> = Box::new(Fake {
                name: endpoint.url()[7..8].to_owned(),
                act: Arc::clone(&acts[i]),
                calls: Arc::clone(&calls[i]),
            });
            Ok((
                transport,
                origins.get(&origin_key(endpoint.url(), None).unwrap()),
            ))
        })
    }

    fn balancer(&self, policy: &BalancePolicy, honours: bool) -> BalancedTransport {
        let urls: Vec<String> = (0..self.acts.len()).map(Self::url).collect();
        BalancedTransport::new(
            Self::url(0),
            Arc::new(StaticEndpoints::new(urls)),
            self.factory(),
            policy,
            honours,
        )
    }

    fn calls(&self) -> Vec<usize> {
        self.calls
            .iter()
            .map(|c| c.load(Ordering::SeqCst))
            .collect()
    }
}

async fn call(
    b: &BalancedTransport,
    method: &str,
    params: serde_json::Value,
) -> ClientResult<String> {
    b.send_request(method, params, &HashMap::new())
        .await
        .map(|v| v.as_str().unwrap_or_default().to_owned())
}

async fn get_task(b: &BalancedTransport) -> ClientResult<String> {
    call(b, "GetTask", serde_json::json!({"id": "t"})).await
}

fn hedged(delay: Duration) -> BalancePolicy {
    BalancePolicy::new().with_hedge(Hedge::after(delay))
}

// ── Hedging ──────────────────────────────────────────────────────────────────

/// `a` takes 1 s, `b` 10 ms; with a 100 ms hedge the answer is `b`'s, at
/// 110 ms, and `a`'s call is dropped.
#[tokio::test(start_paused = true)]
async fn a_slow_idempotent_call_is_answered_by_its_hedge() {
    let fleet = Fleet::new(&[Act::After(1000 * MS), Act::After(10 * MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    let start = Instant::now();
    assert_eq!(get_task(&b).await.unwrap(), "b");
    assert_eq!(start.elapsed(), 110 * MS);
    assert_eq!(fleet.calls(), vec![1, 1]);
    for m in b.members().current().iter() {
        assert_eq!(
            m.state.in_flight(),
            0,
            "{:?}: the loser was dropped",
            m.endpoint
        );
    }
}

/// An answer before the delay sends no copy.
#[tokio::test(start_paused = true)]
async fn an_answer_within_the_delay_is_not_hedged() {
    let fleet = Fleet::new(&[Act::After(99 * MS), Act::After(MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    assert_eq!(get_task(&b).await.unwrap(), "a");
    assert_eq!(fleet.calls(), vec![1, 0]);
}

/// A send without an idempotency key is never hedged, nor a keyed one to a
/// peer not known to honour keys; a keyed one to a peer that does is.
#[tokio::test(start_paused = true)]
async fn only_sends_that_carry_an_honoured_key_are_hedged() {
    let key = a2a_protocol_types::idempotency::IDEMPOTENCY_METADATA_KEY;
    let keyed = serde_json::json!({"message": {"metadata": {key: "k-0123456789abcdef"}}});
    let unkeyed = serde_json::json!({"message": {}});
    for (params, honours, copies) in [
        (unkeyed.clone(), true, 0),
        (keyed.clone(), false, 0),
        (keyed, true, 1),
    ] {
        let fleet = Fleet::new(&[Act::After(1000 * MS), Act::After(10 * MS)], None);
        let b = fleet.balancer(&hedged(100 * MS), honours);
        call(&b, "SendMessage", params).await.unwrap();
        assert_eq!(fleet.calls()[1], copies, "honours {honours}");
    }
}

/// When the first call fails after its copy was sent, the copy's answer is
/// returned; when both fail, the first call's error.
#[tokio::test(start_paused = true)]
async fn a_failure_waits_for_the_other_call() {
    let fleet = Fleet::new(&[Act::FailAfter(200 * MS), Act::After(300 * MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    let start = Instant::now();
    assert_eq!(get_task(&b).await.unwrap(), "b");
    assert_eq!(start.elapsed(), 400 * MS);

    let fleet = Fleet::new(&[Act::FailAfter(500 * MS), Act::FailAfter(MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    let err = get_task(&b).await.expect_err("both failed");
    assert!(
        matches!(&err, ClientError::UnexpectedStatus { body, .. } if body == "a"),
        "{err:?}"
    );
    let fleet = Fleet::new(&[Act::After(500 * MS), Act::FailAfter(MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    let start = Instant::now();
    assert_eq!(get_task(&b).await.unwrap(), "a");
    assert_eq!(start.elapsed(), 500 * MS);
}

/// A budget with one hedge in reserve and nothing earned pays for one copy.
#[tokio::test(start_paused = true)]
async fn hedges_are_paid_from_the_budget() {
    let fleet = Fleet::new(&[Act::After(1000 * MS), Act::After(1000 * MS)], None);
    let policy = BalancePolicy::new().with_hedge(
        Hedge::after(100 * MS).with_budget(RetryBudget::new(0.0, 0.0).with_reserve(1.0)),
    );
    let b = fleet.balancer(&policy, false);
    get_task(&b).await.unwrap();
    assert_eq!(fleet.calls(), vec![1, 1]);
    get_task(&b).await.unwrap();
    get_task(&b).await.unwrap();
    assert_eq!(fleet.calls().iter().sum::<usize>(), 4, "one copy only");
}

/// Each call earns its share of a copy: at half a copy per call with a
/// reserve of one, the first slow call is copied (the reserve), the second
/// is not (half earned), the third is (a whole one).
#[tokio::test(start_paused = true)]
async fn calls_earn_their_copies() {
    let fleet = Fleet::new(&[Act::After(1000 * MS), Act::After(1000 * MS)], None);
    let policy = BalancePolicy::new().with_hedge(
        Hedge::after(100 * MS).with_budget(RetryBudget::new(0.5, 0.0).with_reserve(1.0)),
    );
    let b = fleet.balancer(&policy, false);
    let mut sent = Vec::new();
    for _ in 0..3 {
        get_task(&b).await.unwrap();
        sent.push(fleet.calls().iter().sum::<usize>());
    }
    assert_eq!(sent, [2, 3, 5]);
}

/// The copy goes to another endpoint even when the first is still the
/// least loaded.
#[tokio::test(start_paused = true)]
async fn the_copy_goes_elsewhere() {
    let fleet = Fleet::new(&[Act::After(MS), Act::After(MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    get_task(&b).await.unwrap();
    for act in &fleet.acts {
        *act.lock().unwrap() = Act::After(1000 * MS);
    }
    let set = b.members().current();
    let held = [
        set[0].state.enter(crate::origin::Ticket::Normal).unwrap(),
        set[0].state.enter(crate::origin::Ticket::Normal).unwrap(),
    ];
    // a has two calls in flight, b none: the call goes to b, its copy to a.
    get_task(&b).await.unwrap();
    assert_eq!(fleet.calls(), vec![2, 1]);
    drop(held);
}

/// One endpoint has nowhere to send a copy, and a stream is never hedged.
#[tokio::test(start_paused = true)]
async fn no_copy_without_a_second_endpoint_or_for_a_stream() {
    let fleet = Fleet::new(&[Act::After(1000 * MS)], None);
    let b = fleet.balancer(&hedged(100 * MS), false);
    let start = Instant::now();
    get_task(&b).await.unwrap();
    assert_eq!(start.elapsed(), 1000 * MS);
    assert_eq!(fleet.calls(), vec![1]);

    let fleet = Fleet::new(&[Act::After(MS), Act::After(MS)], None);
    let b = fleet.balancer(&hedged(MS), false);
    let stream = b
        .send_streaming_request("SubscribeToTask", serde_json::json!({}), &HashMap::new())
        .await
        .unwrap();
    assert_eq!(fleet.calls(), vec![1, 0]);
    // The open stream counts in flight until it is dropped.
    let set = b.members().current();
    assert_eq!(set[0].state.in_flight(), 1);
    drop(stream);
    assert_eq!(set[0].state.in_flight(), 0);
}

// ── Selection ────────────────────────────────────────────────────────────────

#[test]
fn the_share_allowed_out_rounds_down() {
    assert_eq!(max_out(1, 50), 0);
    assert_eq!(max_out(2, 50), 1);
    assert_eq!(max_out(3, 50), 1);
    assert_eq!(max_out(4, 50), 2);
    assert_eq!(max_out(3, 100), 3);
    assert_eq!(max_out(3, 0), 0);
    assert_eq!(max_out(10, 33), 3);
}

/// Ejects endpoint `i` of `b` by failing it `n` times through the balancer's
/// own origin state.
fn eject(b: &BalancedTransport, i: usize, n: u32) {
    let set = b.members().current();
    for _ in 0..n {
        let mut guard = set[i].state.enter(crate::origin::Ticket::Normal).unwrap();
        guard.record(crate::origin::Outcome::Failure);
    }
}

/// With every endpoint out and 100% allowed out, the set is a circuit
/// breaker: refused, with the time to the soonest end.
#[tokio::test(start_paused = true)]
async fn an_all_ejected_set_fails_fast_with_the_soonest_end() {
    let fleet = Fleet::new(
        &[Act::After(MS), Act::After(MS)],
        Some(Ejection::new(1, 30 * SECOND, 300 * SECOND)),
    );
    let b = fleet.balancer(&BalancePolicy::new().with_max_ejected_percent(100), false);
    get_task(&b).await.unwrap();
    eject(&b, 0, 1);
    tokio::time::advance(10 * SECOND).await;
    eject(&b, 1, 1);
    tokio::time::advance(5 * SECOND).await;
    let err = get_task(&b).await.expect_err("all out");
    match err {
        ClientError::CircuitOpen {
            origin,
            retry_after,
        } => {
            assert_eq!(origin, "http://a.test");
            assert_eq!(retry_after, Some(15 * SECOND));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fleet.calls(), vec![1, 0]);
}

const SECOND: Duration = Duration::from_secs(1);

/// Past the allowed share, the endpoint whose ejection ends soonest is used,
/// and its success brings it back.
#[tokio::test(start_paused = true)]
async fn the_soonest_ending_ejection_comes_back_first() {
    let fleet = Fleet::new(
        &[Act::After(MS), Act::After(MS)],
        Some(Ejection::new(1, 30 * SECOND, 300 * SECOND)),
    );
    let b = fleet.balancer(&BalancePolicy::new(), false);
    get_task(&b).await.unwrap();
    eject(&b, 1, 1);
    tokio::time::advance(SECOND).await;
    eject(&b, 0, 1);
    // b's ends first (29 s left against a's 30 s): it is the one used.
    assert_eq!(get_task(&b).await.unwrap(), "b");
    let set = b.members().current();
    assert!(
        !set[1].state.is_ejected(Instant::now()),
        "a forced success closes it"
    );
    assert!(set[0].state.is_ejected(Instant::now()));
    assert_eq!(get_task(&b).await.unwrap(), "b");
}

/// `choose` with `exclude` has nothing to offer when the set is one.
#[tokio::test(start_paused = true)]
async fn excluding_the_only_endpoint_leaves_nothing() {
    let fleet = Fleet::new(&[Act::After(MS)], None);
    let b = fleet.balancer(&BalancePolicy::new(), false);
    get_task(&b).await.unwrap();
    let set = b.members().current();
    let err = choose(&set, Some(&set[0]), 50, "x", Instant::now()).expect_err("none");
    assert!(
        matches!(&err, ClientError::CircuitOpen { origin, retry_after: None } if origin == "x"),
        "{err:?}"
    );
}

/// A probe slot is one call: while it runs the endpoint stays out.
#[tokio::test(start_paused = true)]
async fn one_probe_at_a_time() {
    let fleet = Fleet::new(
        &[Act::After(MS), Act::After(1000 * MS)],
        Some(Ejection::new(1, 30 * SECOND, 300 * SECOND)),
    );
    let b = Arc::new(fleet.balancer(&BalancePolicy::new(), false));
    get_task(&b).await.unwrap();
    eject(&b, 1, 1);
    tokio::time::advance(30 * SECOND).await;
    // The first call after the end is b's probe (a was chosen last).
    let probe = {
        let b = Arc::clone(&b);
        tokio::spawn(async move { get_task(&b).await })
    };
    while fleet.calls()[1] == 0 {
        tokio::task::yield_now().await;
    }
    for _ in 0..3 {
        assert_eq!(get_task(&b).await.unwrap(), "a");
    }
    assert_eq!(probe.await.unwrap().unwrap(), "b");
    assert_eq!(fleet.calls(), vec![4, 1]);
}

// ── Debug ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn debug_output_names_endpoints_and_state_not_secrets() {
    let fleet = Fleet::new(&[Act::After(MS)], None);
    let b = fleet.balancer(&hedged(5 * MS), false);
    get_task(&b).await.unwrap();
    let text = format!("{b:?}");
    assert!(
        text.starts_with("BalancedTransport { name: \"http://a.test\""),
        "{text}"
    );
    assert!(
        text.contains("Member { endpoint: Endpoint { url: \"http://a.test\", addr: None }"),
        "{text}"
    );
    assert!(text.contains("hedge_delay: Some(5ms)"), "{text}");
}

#[test]
fn policies_have_their_documented_defaults() {
    let p = BalancePolicy::default();
    assert_eq!(p.ejection, Some(Ejection::default()));
    assert_eq!(p.max_ejected_percent, 50);
    assert_eq!(p.hedge, None);
    assert!(!p.ejection_set());
    assert!(p.with_ejection(Ejection::default()).ejection_set());
    let off = BalancePolicy::new().without_ejection();
    assert!(off.ejection_set());
    assert_eq!(off.ejection, None);
    assert_eq!(
        BalancePolicy::new()
            .with_max_ejected_percent(200)
            .max_ejected_percent,
        100
    );
    let e = Ejection::default();
    assert_eq!(
        (e.consecutive_failures, e.base, e.max),
        (5, 30 * SECOND, 300 * SECOND)
    );
    let e = Ejection::new(0, 10 * SECOND, SECOND);
    assert_eq!(
        (e.consecutive_failures, e.base, e.max),
        (1, 10 * SECOND, 10 * SECOND)
    );
    let h = Hedge::after(7 * MS);
    assert_eq!(h.delay, 7 * MS);
    assert_eq!(h.budget, RetryBudget::new(0.05, 0.0).with_reserve(1.0));
}

#[test]
fn endpoints_keep_their_url_and_address() {
    let addr: std::net::SocketAddr = "192.0.2.1:443".parse().unwrap();
    let pinned = Endpoint::pinned("https://x.test", addr);
    assert_eq!(
        (pinned.url(), pinned.addr()),
        ("https://x.test", Some(addr))
    );
    let plain = Endpoint::new("https://x.test");
    assert_eq!(plain.addr(), None);
    assert_ne!(pinned, plain);
    let source = StaticEndpoints::from_endpoints(vec![pinned.clone()]);
    assert_eq!(source.endpoints(), [pinned]);
}

#[tokio::test]
async fn dns_endpoints_pin_each_address_of_the_name() {
    use super::{DnsEndpoints, EndpointSource};
    let dns = DnsEndpoints::new("http://localhost:4321/a2a").unwrap();
    assert_eq!(dns.refresh_every(), Some(super::DEFAULT_DNS_REFRESH));
    let found = dns.endpoints().await.unwrap();
    assert_ne!(found.len(), 0);
    for e in &found {
        assert_eq!(e.url(), "http://localhost:4321/a2a");
        let addr = e.addr().expect("pinned");
        assert!(addr.ip().is_loopback() && addr.port() == 4321, "{addr}");
    }
    // An address is not resolved: one endpoint, not pinned.
    let literal = DnsEndpoints::new("https://127.0.0.1").unwrap();
    assert_eq!(
        literal.endpoints().await.unwrap(),
        [Endpoint::new("https://127.0.0.1")]
    );
    let every = DnsEndpoints::new("https://h.test")
        .unwrap()
        .with_refresh_every(SECOND);
    assert_eq!(every.refresh_every(), Some(SECOND));
    assert!(DnsEndpoints::new("not a url").is_err());
    assert!(DnsEndpoints::new("/path-only").is_err());
    // No port in the URL: the scheme's.
    for (url, port) in [("https://localhost/a2a", 443), ("http://localhost/a2a", 80)] {
        let found = DnsEndpoints::new(url).unwrap().endpoints().await.unwrap();
        assert_ne!(found.len(), 0);
        assert!(
            found.iter().all(|e| e.addr().unwrap().port() == port),
            "{found:?}"
        );
    }
}

/// A card's interface for another protocol major is not an endpoint.
#[test]
fn a_cards_interface_for_another_major_is_left_out() {
    use a2a_protocol_types::{AgentCard, AgentInterface};
    let mut card = AgentCard::new("x", "1.0.0", AgentInterface::jsonrpc("http://a.test"));
    let mut next = AgentInterface::jsonrpc("http://b.test");
    next.protocol_version = "2.0".into();
    card.supported_interfaces.push(next);
    let source = StaticEndpoints::from_card(&card, "JSONRPC").unwrap();
    assert_eq!(source.endpoints(), [Endpoint::new("http://a.test")]);
}
