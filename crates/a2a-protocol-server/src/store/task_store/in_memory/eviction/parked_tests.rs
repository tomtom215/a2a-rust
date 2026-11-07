// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Capacity eviction and parked tasks (audit D-18): a task waiting for input or
//! authorization is a person's pending decision, so a capacity sweep never
//! takes one, and a store holding nothing else refuses a new task instead.

use a2a_protocol_types::task::{TaskId, TaskState};

use super::capacity::EVICTION_SCAN_WINDOW;
use super::fixtures::{config, ids, store_of, task};
use super::{EvictionPasses, InMemoryTaskStore, StoreData};
use crate::store::TaskStore;

use TaskState::{AuthRequired, Completed, InputRequired, Working};

fn evict(data: &mut StoreData, max: usize) {
    InMemoryTaskStore::evict(data, &config(Some(max), None, 0), EvictionPasses::all());
}

/// Finished work goes first, then the oldest working tasks, and a parked task
/// survives however old it is: the two oldest and the newest-but-one are
/// parked, so the sweep that has to remove three takes the finished one and the
/// two working ones.
#[test]
fn a_capacity_sweep_takes_finished_then_working_tasks_and_never_a_parked_one() {
    let mut data = store_of(&[InputRequired, Working, Completed, AuthRequired, Working]);
    evict(&mut data, 2);
    assert_eq!(ids(&data), vec!["t0".to_string(), "t3".to_string()]);
    assert_eq!(data.live_evictions, 2, "the two working tasks were lost");
}

/// A run of parked tasks longer than the scan window at the old end does not
/// end the top-up with nothing: the working tasks behind it are still found.
#[test]
fn working_tasks_behind_a_window_of_parked_ones_are_still_evicted() {
    let mut states = vec![InputRequired; EVICTION_SCAN_WINDOW + 6];
    states.extend([Working, Working, Working]);
    let parked = EVICTION_SCAN_WINDOW + 6;
    let mut data = store_of(&states);
    evict(&mut data, states.len() - 2);
    let left = ids(&data);
    assert_eq!(left.len(), states.len() - 2);
    assert!(
        !left.contains(&format!("t{parked}")),
        "the oldest working task goes"
    );
    assert!(!left.contains(&format!("t{}", parked + 1)), "then the next");
    assert!(
        left.contains(&format!("t{}", parked + 2)),
        "the third stays"
    );
    assert_eq!(data.live_evictions, 2);
}

/// With only parked tasks to take, the sweep takes none: the store stays over
/// the cap, and nothing is counted as lost.
#[test]
fn a_store_of_parked_tasks_is_left_as_it_is() {
    let mut data = store_of(&[
        InputRequired,
        AuthRequired,
        InputRequired,
        AuthRequired,
        InputRequired,
    ]);
    evict(&mut data, 2);
    assert_eq!(data.len(), 5);
    assert_eq!(data.live_evictions, 0);
}

/// A finished victim is not a lost task: the count is of working tasks only.
#[test]
fn evicting_finished_tasks_is_not_counted_as_losing_work() {
    let mut data = store_of(&[Completed, Completed, Completed, Working]);
    evict(&mut data, 2);
    assert_eq!(data.live_evictions, 0);
}

/// The refusal's truth table: a new task, a store at its cap, every task parked.
/// Each condition is varied alone from the refusing case.
#[test]
fn a_new_task_is_refused_only_when_the_store_is_full_of_parked_tasks() {
    let all_parked = store_of(&[InputRequired, AuthRequired, InputRequired]);
    let new = TaskId::new("new");
    assert!(
        all_parked.refuses_for_parked_tasks(&new, Some(3), None),
        "at the cap, all parked"
    );
    assert!(
        all_parked.refuses_for_parked_tasks(&new, Some(2), None),
        "over the cap, all parked"
    );
    assert!(
        !all_parked.refuses_for_parked_tasks(&new, Some(4), None),
        "room left: the store is not full"
    );
    assert!(
        !all_parked.refuses_for_parked_tasks(&new, None, None),
        "no cap, nothing to refuse for"
    );
    assert!(
        !all_parked.refuses_for_parked_tasks(&TaskId::new("t1"), Some(3), None),
        "a task the store holds is an update, not a new task"
    );
    for other in [Working, Completed] {
        let mixed = store_of(&[InputRequired, other, AuthRequired]);
        assert!(
            !mixed.refuses_for_parked_tasks(&new, Some(3), None),
            "a {other:?} task can be evicted to make room"
        );
    }
    assert!(
        !store_of(&[]).refuses_for_parked_tasks(&new, Some(0), None),
        "an empty store holds no parked task, so it is not 'full of' them"
    );
}

/// Through the store: a full store of parked tasks refuses `save` and
/// `insert_if_absent` of a new task with a temporary (`503`) error and keeps
/// every parked task; an update of a parked task is not refused; and a parked
/// task that finishes frees room, because the finished one is what the next
/// sweep takes.
#[tokio::test]
async fn a_full_store_of_parked_tasks_refuses_new_ones_and_recovers_when_one_finishes() {
    let store = InMemoryTaskStore::with_config(config(Some(2), None, 0));
    store.save(&task("p1", InputRequired)).await.unwrap();
    store.save(&task("p2", AuthRequired)).await.unwrap();

    let refused = store
        .save(&task("w", Working))
        .await
        .expect_err("full of parked tasks");
    assert!(
        refused.to_string().contains("every task is waiting"),
        "{refused}"
    );
    let refused = store
        .insert_if_absent(&task("w", Working))
        .await
        .expect_err("same");
    assert!(
        refused.to_string().contains("every task is waiting"),
        "{refused}"
    );
    assert_eq!(
        store.count().await.unwrap(),
        2,
        "nothing was inserted, nothing evicted"
    );

    store
        .save(&task("p1", InputRequired))
        .await
        .expect("an update is not a new task");

    store
        .save(&task("p1", Completed))
        .await
        .expect("the parked task finishes");
    store
        .save(&task("w", Working))
        .await
        .expect("there is a finished task to evict");
    assert!(
        store.get(&TaskId::new("p1")).await.unwrap().is_none(),
        "the finished one went"
    );
    assert!(
        store.get(&TaskId::new("p2")).await.unwrap().is_some(),
        "the parked one stayed"
    );
    assert!(store.get(&TaskId::new("w")).await.unwrap().is_some());
}

/// The count is read through the store, and only working victims are in it.
#[tokio::test]
async fn live_evictions_counts_the_working_tasks_capacity_eviction_removed() {
    let store = InMemoryTaskStore::with_config(config(Some(1), None, 0));
    assert_eq!(store.live_evictions().await, 0);
    store.save(&task("a", Working)).await.unwrap();
    store.save(&task("b", Working)).await.unwrap();
    assert_eq!(
        store.live_evictions().await,
        1,
        "a was working when b displaced it"
    );
    store.save(&task("b", Completed)).await.unwrap();
    store.save(&task("c", Working)).await.unwrap();
    assert_eq!(store.live_evictions().await, 1, "b had finished");
}
