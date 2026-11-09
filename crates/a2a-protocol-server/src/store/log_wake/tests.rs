// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use std::time::Duration;

use futures_util::FutureExt as _;

use super::{LogWake, LogWakers, key};

/// A deadline that only a missed wake-up reaches; a pass never waits on it.
const GUARD: Duration = Duration::from_secs(10);

async fn woken(watch: super::LogWatch) -> LogWake {
    tokio::time::timeout(GUARD, watch)
        .await
        .expect("the waiter was woken")
}

fn k(task: &str) -> String {
    key("", task).expect("fits")
}

// ── The key ──────────────────────────────────────────────────────────────────

/// The key is the partition's length, a colon, the partition and the task.
#[test]
fn a_key_is_the_partition_length_then_the_partition_then_the_task() {
    assert_eq!(key("", "t1").as_deref(), Some("0:t1"));
    assert_eq!(key("acme", "t1").as_deref(), Some("4:acmet1"));
    assert_eq!(key("a:b", "c").as_deref(), Some("3:a:bc"));
}

/// Two different (partition, task) pairs never share a key, whatever the
/// characters: the length is what tells where the partition ends.
#[test]
fn different_pairs_never_share_a_key() {
    assert_ne!(key("ab", "c"), key("a", "bc"));
    assert_ne!(key("", "1:x"), key("1", "x"));
    assert_ne!(key("", "ab"), key("a", "b"));
}

/// The key may take 512 bytes and no more: a task id that makes it longer is
/// not announced, and not watched.
#[test]
fn a_key_of_512_bytes_fits_and_one_more_does_not() {
    let prefix = "0:".len();
    let at = "x".repeat(512 - prefix);
    assert_eq!(key("", &at).map(|k| k.len()), Some(512));
    let over = "x".repeat(512 - prefix + 1);
    assert_eq!(key("", &over), None);
    // Bytes, not characters: `é` is two.
    let wide = "é".repeat((512 - prefix) / 2);
    assert_eq!(key("", &wide).map(|k| k.len()), Some(512));
    let wider = "é".repeat((512 - prefix) / 2 + 1);
    assert_eq!(key("", &wider), None);
    // The partition counts too.
    assert_eq!(key(&"p".repeat(300), &"t".repeat(300)), None);
}

// ── The waiters ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_notification_wakes_the_waiter_on_that_key() {
    let wakers = LogWakers::new();
    let watch = wakers.watch(k("a"));
    wakers.notify(&k("a"));
    assert_eq!(woken(watch).await, LogWake::Notified);
}

/// The interest begins when the watch is made, not when it is first polled: a
/// stream watches, reads the store, then waits, and a change in between must
/// wake it.
#[tokio::test]
async fn a_notification_before_the_first_poll_is_not_lost() {
    let wakers = LogWakers::new();
    let watch = wakers.watch(k("a"));
    wakers.notify(&k("a"));
    wakers.notify(&k("a"));
    assert_eq!(watch.now_or_never(), Some(LogWake::Notified));
}

/// A waiter is not woken by another task's notification, nor by one sent
/// before it watched.
#[tokio::test]
async fn only_the_named_task_is_woken() {
    let wakers = LogWakers::new();
    wakers.notify(&k("a"));
    let a = wakers.watch(k("a"));
    let b = wakers.watch(k("b"));
    wakers.notify(&k("b"));
    assert_eq!(woken(b).await, LogWake::Notified);
    assert!(a.now_or_never().is_none(), "a was not named");
}

/// The same task in another partition is another key.
#[tokio::test]
async fn the_same_task_in_another_partition_is_not_woken() {
    let wakers = LogWakers::new();
    let acme = wakers.watch(key("acme", "t").expect("fits"));
    wakers.notify(&key("globex", "t").expect("fits"));
    assert!(acme.now_or_never().is_none());
}

/// Every waiter on a task is woken, not one of them.
#[tokio::test]
async fn every_waiter_on_the_task_is_woken() {
    let wakers = LogWakers::new();
    let first = wakers.watch(k("a"));
    let second = wakers.watch(k("a"));
    wakers.notify(&k("a"));
    assert_eq!(woken(first).await, LogWake::Notified);
    assert_eq!(woken(second).await, LogWake::Notified);
}

/// A resync wakes every waiter, whatever it waits on; a notification that
/// arrives with it is the one reported.
#[tokio::test]
async fn a_resync_wakes_every_waiter() {
    let wakers = LogWakers::new();
    let a = wakers.watch(k("a"));
    let b = wakers.watch(k("b"));
    let c = wakers.watch(k("c"));
    wakers.resync();
    wakers.notify(&k("c"));
    assert_eq!(woken(a).await, LogWake::Resynced);
    assert_eq!(woken(b).await, LogWake::Resynced);
    assert_eq!(woken(c).await, LogWake::Notified);
}

/// A resync before a watch was made does not wake it.
#[tokio::test]
async fn an_earlier_resync_does_not_wake_a_later_watch() {
    let wakers = LogWakers::new();
    wakers.resync();
    assert!(wakers.watch(k("a")).now_or_never().is_none());
}

// ── Cleanup ──────────────────────────────────────────────────────────────────

/// An entry lives while it has a waiter and no longer; a notification for a
/// task nobody waits on makes none.
#[tokio::test]
async fn an_entry_lasts_as_long_as_its_last_waiter() {
    let wakers = LogWakers::new();
    wakers.notify(&k("a"));
    assert_eq!(wakers.watched(), 0, "a notification registers nothing");

    let first = wakers.watch(k("a"));
    let second = wakers.watch(k("a"));
    let other = wakers.watch(k("b"));
    assert_eq!(wakers.watched(), 2);
    drop(first);
    assert_eq!(wakers.watched(), 2, "one waiter on a is left");
    wakers.notify(&k("a"));
    assert_eq!(woken(second).await, LogWake::Notified);
    assert_eq!(
        wakers.watched(),
        1,
        "the last waiter on a ended its interest"
    );
    drop(other);
    assert_eq!(wakers.watched(), 0);
}

/// A watch dropped without ever being polled ends its interest too.
#[tokio::test]
async fn a_watch_dropped_unpolled_ends_its_interest() {
    let wakers = LogWakers::new();
    drop(wakers.watch(k("a")));
    assert_eq!(wakers.watched(), 0);
}
