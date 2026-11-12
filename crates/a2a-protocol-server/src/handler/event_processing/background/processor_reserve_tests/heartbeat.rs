// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A reservation is held while its writer lives, however long its store takes
//! to answer, and is settled once the writer stops (adversarial review R3,
//! 2026-11-12). The rig and helpers are `processor_reserve_tests`'s.

use super::*;

/// A processor that is only slow (not stopped) keeps its reservation: the hold
/// is extended while its write runs, so no worker settles it, and when the
/// write completes the rows are armed, not recorded afresh (adversarial review
/// R3). Before the heartbeat the hold ran out in the middle of the write, a
/// worker removed the rows as "not stored", and an event that then committed
/// depended on the process living to record it again.
#[tokio::test(start_paused = true)]
async fn a_processor_stalled_past_its_hold_keeps_its_reservation_and_arms_it() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::Before);
    let running = rig.run_on_a(artifact(1));
    parked(&rig).await;

    // Several holds pass with the write still blocked, and then some.
    rig.wait(hold(&rig) * 4).await;
    assert_eq!(
        rig.rows().await,
        1,
        "the reservation of a live writer was settled while its write was in flight"
    );
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 0);
    assert!(rig.a_seen.try_recv().is_err() && rig.b_seen.try_recv().is_err());

    rig.store.resume.notify_one();
    running.await.unwrap();
    let first = tokio::time::timeout(Duration::from_mins(15), async {
        tokio::select! {
            seen = rig.a_seen.recv() => seen.unwrap(),
            seen = rig.b_seen.recv() => seen.unwrap(),
        }
    })
    .await
    .expect("no delivery within 15 minutes of (paused) time");
    assert_eq!(first.state, None, "the artifact update");
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.a_seen.try_recv().is_err() && rig.b_seen.try_recv().is_err());
    assert_eq!(rig.rows().await, 0);
    assert_eq!(
        rig.metrics.count(push_outcome::RESERVATION_SETTLED),
        0,
        "armed by its own writer, not settled by a worker"
    );
    assert_eq!(
        rig.delivery_ids().len(),
        1,
        "delivered once, under the id it was reserved with"
    );
}

/// The writer stops while its write is in flight: it stops renewing, so its
/// hold runs out and a worker settles it (here, as not stored).
#[tokio::test(start_paused = true)]
async fn a_writer_that_stops_stops_renewing_and_its_reservation_is_settled() {
    let rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::Before);
    let running = rig.run_on_a(artifact(1));
    parked(&rig).await;
    rig.wait(hold(&rig) * 2).await;
    assert_eq!(rig.rows().await, 1, "held while it lives");

    rig.kill_a(running).await;
    rig.wait(hold(&rig) * 2).await;
    assert_eq!(rig.rows().await, 0, "settled once nothing renews it");
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 1);
}

/// An outbox that cannot extend a hold (or is down when the beat comes) leaves
/// the hold as it was set: a write outlasting it is settled as not stored, and
/// the processor records the deliveries afresh when its write completes.
#[tokio::test(start_paused = true)]
async fn an_outbox_that_cannot_extend_a_hold_falls_back_to_recording_afresh() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.flaky
        .fail_extend
        .store(true, std::sync::atomic::Ordering::SeqCst);
    rig.store.stop(Stop::Before);
    let running = rig.run_on_a(status(1, TaskState::Working));
    parked(&rig).await;

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(rig.rows().await, 0, "settled as not stored while it waited");

    rig.store.resume.notify_one();
    running.await.unwrap();
    // One of the two replicas' workers delivers the fresh row.
    let first = tokio::time::timeout(Duration::from_mins(15), async {
        tokio::select! {
            seen = rig.a_seen.recv() => seen.unwrap(),
            seen = rig.b_seen.recv() => seen.unwrap(),
        }
    })
    .await
    .expect("no delivery within 15 minutes of (paused) time");
    assert_eq!(first.state, Some(TaskState::Working));
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.a_seen.try_recv().is_err() && rig.b_seen.try_recv().is_err());
    assert_eq!(rig.rows().await, 0);
}
