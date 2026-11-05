// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which replica a call goes to: the fewest calls in flight, ties to the one
//! chosen longest ago.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_client::balance::{
    BalancePolicy, Endpoint, EndpointSource, EndpointsFuture, StaticEndpoints,
};
use a2a_protocol_client::{ClientBuilder, ClientError};
use a2a_protocol_types::{AgentCard, AgentInterface};

use super::harness::{Mode, Replica, counts, get, three, urls};

#[tokio::test]
async fn calls_one_after_another_go_round_the_set() {
    let replicas = three().await;
    let client = ClientBuilder::new(replicas[0].url())
        .with_balancing(StaticEndpoints::new(urls(&replicas)), BalancePolicy::new())
        .build()
        .unwrap();
    let mut order = Vec::new();
    for _ in 0..6 {
        order.push(get(&client).await.expect("answered"));
    }
    assert_eq!(order, ["a", "b", "c", "a", "b", "c"]);
    for r in &replicas {
        assert_eq!(r.connections(), 1, "{}: its connection is reused", r.name);
        assert_eq!(r.hosts(), [r.addr.to_string(), r.addr.to_string()]);
    }
}

/// A replica holding a call is not given the next one while another is free.
#[tokio::test]
async fn a_busy_replica_is_passed_over() {
    let replicas = three().await;
    replicas[0].set(Mode::Stall);
    let client = Arc::new(
        ClientBuilder::new(replicas[0].url())
            .with_balancing(StaticEndpoints::new(urls(&replicas)), BalancePolicy::new())
            .build()
            .unwrap(),
    );
    let held = {
        let client = Arc::clone(&client);
        tokio::spawn(async move { get(&client).await })
    };
    while replicas[0].requests() == 0 {
        tokio::task::yield_now().await;
    }
    let mut order = Vec::new();
    for _ in 0..4 {
        order.push(get(&client).await.expect("answered"));
    }
    assert_eq!(order, ["b", "c", "b", "c"]);
    assert_eq!(counts(&replicas), vec![1, 2, 2]);
    held.abort();
}

/// The HTTP+JSON binding is balanced as JSON-RPC is.
#[tokio::test]
async fn the_rest_binding_is_balanced_too() {
    let replicas = three().await;
    let client = ClientBuilder::new(replicas[0].url())
        .with_protocol_binding("HTTP+JSON")
        .with_balancing(StaticEndpoints::new(urls(&replicas)), BalancePolicy::new())
        .build()
        .unwrap();
    for _ in 0..3 {
        get(&client).await.expect("answered");
    }
    assert_eq!(counts(&replicas), vec![1, 1, 1]);
    assert_eq!(replicas[1].methods(), ["GET /tasks/t"]);
}

/// A card that lists one interface per replica.
#[tokio::test]
async fn a_cards_interfaces_of_one_binding_are_the_set() {
    let replicas = three().await;
    let mut card = AgentCard::new("x", "1.0.0", AgentInterface::jsonrpc(replicas[0].url()));
    card.supported_interfaces
        .push(AgentInterface::jsonrpc(replicas[1].url()));
    card.supported_interfaces
        .push(AgentInterface::jsonrpc(replicas[2].url()).with_tenant("other"));
    card.supported_interfaces
        .push(AgentInterface::new(replicas[2].url(), "HTTP+JSON"));
    let source = StaticEndpoints::from_card(&card, "jsonrpc").unwrap();
    assert_eq!(
        source.endpoints(),
        [
            Endpoint::new(replicas[0].url()),
            Endpoint::new(replicas[1].url())
        ]
    );
    let err = StaticEndpoints::from_card(&card, "GRPC").expect_err("none");
    assert!(matches!(err, ClientError::Config(_)), "{err:?}");
}

/// Endpoints of another scheme than the client's are refused: the
/// plaintext check is armed against the client's endpoint.
#[tokio::test]
async fn an_endpoint_of_another_scheme_is_not_used() {
    let replicas = three().await;
    let client = ClientBuilder::new(replicas[0].url())
        .with_balancing(
            StaticEndpoints::new(["https://elsewhere.example".to_owned(), replicas[1].url()]),
            BalancePolicy::new(),
        )
        .build()
        .unwrap();
    for _ in 0..2 {
        assert_eq!(get(&client).await.expect("answered"), "b");
    }
    let only_wrong = ClientBuilder::new(replicas[0].url())
        .with_balancing(
            StaticEndpoints::new(["https://elsewhere.example"]),
            BalancePolicy::new(),
        )
        .build()
        .unwrap();
    let err = get(&only_wrong).await.expect_err("nothing to use");
    assert!(
        matches!(&err, ClientError::Config(m) if m.contains("scheme")),
        "{err:?}"
    );
}

/// A source that answers differently each time it is asked.
struct Changing {
    answers: std::sync::Mutex<Vec<Result<Vec<Endpoint>, ClientError>>>,
    asked: std::sync::atomic::AtomicUsize,
}

impl EndpointSource for Changing {
    fn endpoints(&self) -> EndpointsFuture<'_> {
        self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let next = self.answers.lock().unwrap().remove(0);
        Box::pin(async move { next })
    }

    fn refresh_every(&self) -> Option<Duration> {
        Some(Duration::from_secs(10))
    }
}

/// The source is asked again once its interval has passed; an answer that
/// fails, or is empty, keeps the set there was.
#[tokio::test]
async fn the_set_follows_its_source_and_survives_its_failures() {
    let [a, b, _c] = three().await;
    let source = Arc::new(Changing {
        answers: std::sync::Mutex::new(vec![
            Ok(vec![Endpoint::new(a.url())]),
            Ok(vec![Endpoint::new(b.url())]),
            Err(ClientError::HttpClient("resolver down".into())),
            Ok(Vec::new()),
        ]),
        asked: std::sync::atomic::AtomicUsize::new(0),
    });
    struct Shared(Arc<Changing>);
    impl EndpointSource for Shared {
        fn endpoints(&self) -> EndpointsFuture<'_> {
            self.0.endpoints()
        }
        fn refresh_every(&self) -> Option<Duration> {
            self.0.refresh_every()
        }
    }
    let client = ClientBuilder::new(a.url())
        .with_balancing(Shared(Arc::clone(&source)), BalancePolicy::new())
        .build()
        .unwrap();
    let asked = || source.asked.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(get(&client).await.unwrap(), "a");
    assert_eq!(asked(), 1);
    super::harness::elapse(Duration::from_secs(10)).await;
    // This call starts the refresh and uses the set it has.
    assert_eq!(get(&client).await.unwrap(), "a");
    while asked() < 2 {
        tokio::task::yield_now().await;
    }
    // On this one-thread runtime the refresh, once it has asked, has also
    // swapped the set: the source answers without waiting.
    assert_eq!(get(&client).await.unwrap(), "b");
    for _ in 0..2 {
        super::harness::elapse(Duration::from_secs(10)).await;
        let before = asked();
        assert_eq!(get(&client).await.unwrap(), "b");
        while asked() == before {
            tokio::task::yield_now().await;
        }
        assert_eq!(get(&client).await.unwrap(), "b", "kept after a bad answer");
    }
    assert_eq!(asked(), 4);
}

/// A source that fails before it ever answered fails the call.
#[tokio::test]
async fn a_source_that_never_answered_fails_the_call() {
    let a = Replica::start("a").await;
    let source = Changing {
        answers: std::sync::Mutex::new(vec![Err(ClientError::HttpClient("resolver down".into()))]),
        asked: std::sync::atomic::AtomicUsize::new(0),
    };
    let client = ClientBuilder::new(a.url())
        .with_balancing(source, BalancePolicy::new())
        .build()
        .unwrap();
    let err = get(&client).await.expect_err("no set");
    assert!(
        matches!(&err, ClientError::HttpClient(m) if m == "resolver down"),
        "{err:?}"
    );
}

/// What the builder refuses.
#[tokio::test]
async fn balancing_with_a_custom_transport_is_refused() {
    let a = Replica::start("a").await;
    let custom = a2a_protocol_client::JsonRpcTransport::new(a.url()).unwrap();
    let err = ClientBuilder::new(a.url())
        .with_custom_transport(custom)
        .with_balancing(StaticEndpoints::new([a.url()]), BalancePolicy::new())
        .build()
        .expect_err("conflict");
    assert!(
        matches!(&err, ClientError::Config(m) if m.contains("with_balancing") && m.contains("with_custom_transport")),
        "{err:?}"
    );
}
