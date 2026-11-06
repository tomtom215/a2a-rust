// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The gate's two bounds added in 0.16.0: per partition (audit NEW-f-2) and
//! per resolved address (audit NEW-d-3).
//!
//! The clock is paused, as in `tests.rs`; a latched sender holds its permit
//! until the test releases it, so "at once" is exact, not a timing guess.

use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};
use tokio::sync::Semaphore;

use super::*;
use crate::metrics::NoopMetrics;

fn event() -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t1"),
        ContextId::new("c1"),
        TaskStatus::with_timestamp(TaskState::Working),
    ))
}

fn config(url: &str) -> TaskPushNotificationConfig {
    TaskPushNotificationConfig::new("t1".to_owned(), url.to_owned()).with_id("cfg".to_owned())
}

/// A sender that holds every delivery until [`Latched::release`], counts how
/// many run at once, and, when `resolves_to` is set, first holds that address
/// the way `HttpPushSender` does once it has resolved a name.
#[derive(Default)]
struct Latched {
    now: AtomicUsize,
    max: AtomicUsize,
    done: AtomicUsize,
    resolves_to: Option<IpAddr>,
    latches: Mutex<std::collections::HashMap<String, Arc<Semaphore>>>,
}

impl Latched {
    fn resolving_to(ip: &str) -> Arc<Self> {
        Arc::new(Self {
            resolves_to: Some(ip.parse().unwrap()),
            ..Self::default()
        })
    }

    fn latch(&self, url: &str) -> Arc<Semaphore> {
        Arc::clone(
            self.latches
                .lock()
                .unwrap()
                .entry(url.to_owned())
                .or_insert_with(|| Arc::new(Semaphore::new(0))),
        )
    }

    fn release(&self, url: &str) {
        self.latch(url).add_permits(1);
    }

    fn running(&self) -> usize {
        self.now.load(Ordering::SeqCst)
    }
}

impl PushSender for Latched {
    fn send<'a>(
        &'a self,
        url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let _hold = match self.resolves_to {
                Some(ip) => admission::hold_addresses(&[ip]).await,
                None => None,
            };
            let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.max.fetch_max(n, Ordering::SeqCst);
            self.latch(url).acquire_owned().await.unwrap().forget();
            self.now.fetch_sub(1, Ordering::SeqCst);
            self.done.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

fn gate(sender: &Arc<Latched>, limits: &HandlerLimits) -> Arc<PushGate> {
    Arc::new(PushGate::new(
        Arc::clone(sender) as Arc<dyn PushSender>,
        limits,
        Arc::new(NoopMetrics),
    ))
}

/// Starts a delivery of `url` in `tenant` and returns at once.
fn start(gate: &Arc<PushGate>, tenant: &str, url: &str) -> tokio::task::JoinHandle<()> {
    let (gate, url, tenant) = (Arc::clone(gate), url.to_owned(), tenant.to_owned());
    tokio::spawn(TenantContext::scope(tenant, async move {
        gate.send(&url, &event(), &config(&url)).await.unwrap();
    }))
}

async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

/// One caller fills its share of the gate with webhooks that never answer;
/// another caller's delivery still runs at once. Before the partition bound
/// the first caller held every overall permit and the second waited for one
/// of its tarpits to give up (audit NEW-f-2).
#[tokio::test(start_paused = true)]
async fn one_partition_cannot_hold_every_overall_permit() {
    let sender = Arc::new(Latched::default());
    let limits = HandlerLimits::default()
        .with_push_max_in_flight(4)
        .with_push_max_in_flight_per_partition(2);
    let gate = gate(&sender, &limits);
    let tarpit = |i: usize| format!("http://tarpit{i}.example.com/hook");
    for i in 0..4 {
        start(&gate, "hog", &tarpit(i));
    }
    settle().await;
    assert_eq!(sender.running(), 2, "the hog runs its share and no more");

    let legit = "http://legit.example.com/hook";
    sender.release(legit);
    tokio::time::timeout(Duration::from_secs(1), start(&gate, "other", legit))
        .await
        .expect("the other partition's delivery was held behind the hog's")
        .unwrap();

    for i in 0..4 {
        sender.release(&tarpit(i));
    }
    settle().await;
    assert_eq!(sender.done.load(Ordering::SeqCst), 5, "every delivery ran");
}

/// The partition bound is a share, not a reservation: with the hog's share
/// full, the hog's next delivery waits, while the overall bound still has
/// room for others.
#[tokio::test(start_paused = true)]
async fn a_partition_over_its_share_waits_and_runs_when_one_of_its_own_ends() {
    let sender = Arc::new(Latched::default());
    let limits = HandlerLimits::default()
        .with_push_max_in_flight(10)
        .with_push_max_in_flight_per_partition(1);
    let gate = gate(&sender, &limits);
    let (a, b) = ("http://a.example.com/1", "http://b.example.com/2");
    start(&gate, "hog", a);
    let second = start(&gate, "hog", b);
    settle().await;
    assert_eq!(sender.running(), 1);
    sender.release(b);
    sender.release(a);
    tokio::time::timeout(Duration::from_secs(1), second)
        .await
        .expect("the second ran once the first left")
        .unwrap();
    assert_eq!(gate.shared.partitions.len(), 0, "nothing left behind");
}

/// A wildcard record: many names, one address. Before the address bound each
/// name was its own destination and a victim took `push_max_in_flight`
/// connections; now it takes `push_max_in_flight_per_host` (audit NEW-d-3).
#[tokio::test(start_paused = true)]
async fn names_that_resolve_to_one_address_share_its_bound() {
    let sender = Latched::resolving_to("203.0.113.9");
    let limits = HandlerLimits::default()
        .with_push_max_in_flight(100)
        .with_push_max_in_flight_per_host(2);
    let gate = gate(&sender, &limits);
    let name = |i: usize| format!("http://a{i}.evil.example/hook");
    for i in 0..6 {
        start(&gate, "", &name(i));
    }
    settle().await;
    assert_eq!(sender.max.load(Ordering::SeqCst), 2);
    for i in 0..6 {
        sender.release(&name(i));
        settle().await;
    }
    assert_eq!(sender.done.load(Ordering::SeqCst), 6, "every delivery ran");
    assert_eq!(gate.shared.addresses.len(), 0, "nothing left behind");
}

/// A delivery waiting for a full address gives back its overall permit while
/// it waits, so one victim's queue cannot hold every overall permit; it takes
/// one again when the address frees.
#[tokio::test(start_paused = true)]
async fn a_delivery_waiting_for_an_address_holds_no_overall_permit() {
    let victim = Latched::resolving_to("203.0.113.9");
    let limits = HandlerLimits::default()
        .with_push_max_in_flight(2)
        .with_push_max_in_flight_per_host(1);
    let gate = gate(&victim, &limits);
    let (v1, v2) = ("http://v1.evil.example/hook", "http://v2.evil.example/hook");
    start(&gate, "", v1);
    let waiter = start(&gate, "", v2);
    settle().await;
    assert_eq!(victim.running(), 1, "one per address");
    assert_eq!(
        gate.shared.overall.available_permits(),
        1,
        "the waiter gave its overall permit back"
    );

    victim.release(v2);
    victim.release(v1);
    tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("the waiter ran once the address freed")
        .unwrap();
    assert_eq!(gate.shared.overall.available_permits(), 2);
}

/// The bundled sender holds the address it connects to: two receivers on one
/// address, different ports, so two destinations by name and one by address.
/// With a bound of one, the second request arrives only after the first was
/// answered. Without `hold_addresses` in `HttpPushSender::send` it arrived
/// while the first was held.
#[tokio::test]
async fn the_bundled_sender_holds_the_address_it_connects_to() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::{Notify, mpsc};

    let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release = Arc::new(Notify::new());
    let (arrived_tx, mut arrived) = mpsc::unbounded_channel::<bool>();
    let mut urls = Vec::new();
    for _ in 0..2 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        urls.push(format!("http://{}/hook", listener.local_addr().unwrap()));
        let (released, release, arrived_tx) = (
            Arc::clone(&released),
            Arc::clone(&release),
            arrived_tx.clone(),
        );
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = vec![0u8; 8192];
                let _ = stream.read(&mut buf).await;
                let first = !released.load(Ordering::SeqCst);
                arrived_tx.send(first).unwrap();
                if first {
                    release.notified().await;
                }
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            }
        });
    }
    let http = crate::push::HttpPushSender::new()
        .allow_private_urls()
        .with_retry_policy(crate::push::PushRetryPolicy::default().with_max_attempts(1));
    let gate = Arc::new(PushGate::new(
        Arc::new(http),
        &HandlerLimits::default().with_push_max_in_flight_per_host(1),
        Arc::new(NoopMetrics),
    ));

    let a = start(&gate, "", &urls[0]);
    assert!(arrived.recv().await.unwrap(), "the first request is held");
    let b = start(&gate, "", &urls[1]);
    // While the first request holds the address the second must not arrive.
    // Give it the time to: it arrives within that when nothing holds the
    // address (these are IP literals, so the address comes from `literal_ip`).
    assert!(
        tokio::time::timeout(Duration::from_millis(500), arrived.recv())
            .await
            .is_err(),
        "the second request arrived while the first held the address"
    );
    released.store(true, Ordering::SeqCst);
    release.notify_one();
    a.await.unwrap();
    assert!(
        !arrived.recv().await.unwrap(),
        "the second request arrived while the first held the address"
    );
    b.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn an_admission_and_an_address_hold_show_what_they_hold() {
    let admission = admission::Admission::new(Duration::from_secs(3));
    assert_eq!(
        format!("{admission:?}"),
        "Admission { timeout: 3s, waiting: false, .. }"
    );
    let gate = gate(&Arc::new(Latched::default()), &HandlerLimits::default());
    admission.enter(
        Arc::clone(&gate.shared),
        Arc::new(Mutex::new(admission::Held::default())),
    );
    let hold = admission
        .scope(admission::hold_addresses(&[
            "192.0.2.1".parse().unwrap(),
            "192.0.2.2".parse().unwrap(),
            "192.0.2.1".parse().unwrap(),
        ]))
        .await
        .expect("in a gate, with addresses");
    assert_eq!(format!("{hold:?}"), "AddressHold { addresses: 2 }");
    assert_eq!(
        gate.shared.addresses.len(),
        2,
        "one slot per distinct address"
    );
    drop(hold);
    assert_eq!(gate.shared.addresses.len(), 0);
    assert!(
        admission
            .scope(admission::hold_addresses(&[]))
            .await
            .is_none(),
        "no addresses, no hold"
    );
    admission.leave();
    assert!(
        admission
            .scope(admission::hold_addresses(&["192.0.2.1".parse().unwrap()]))
            .await
            .is_none(),
        "not in a gate, no hold"
    );
}
