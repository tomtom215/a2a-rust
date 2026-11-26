// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A delegation continues a child whose turn was handed back, within its
//! policy (ADR 0054). The transport is scripted and the clock is paused.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use a2a_protocol_types::error::{A2aError, ErrorCode};
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageRole, Part};
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskStatus};

use super::*;
use crate::ClientBuilder;
use crate::delegation::{DelegationOptions, Outcome, ResumePolicy};
use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

/// Answers the n-th streaming send with the n-th script (the last one once
/// they run out), records every send's params, and answers `CancelTask` with
/// a canceled task.
struct Scripted {
    scripts: Vec<Vec<StreamResponse>>,
    sends: Arc<Mutex<Vec<serde_json::Value>>>,
    /// The sends, by index, whose response is lost: recorded, then failed.
    lost: Vec<usize>,
    /// The sends, by index, a halted server refuses with its typed detail.
    halted: Vec<usize>,
}

impl Transport for Scripted {
    fn send_request<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        let task = Task::new("t", "c", TaskStatus::new(TaskState::Canceled));
        Box::pin(async move { serde_json::to_value(task).map_err(ClientError::Serialization) })
    }

    fn send_streaming_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        _extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(async move {
            if method != "SendStreamingMessage" {
                return Err(ClientError::Transport("no subscriptions here".into()));
            }
            let n = {
                let mut sends = self.sends.lock().unwrap();
                sends.push(params);
                (sends.len() - 1).min(self.scripts.len() - 1)
            };
            if self.lost.contains(&n) {
                return Err(ClientError::Transport("the response was lost".into()));
            }
            if self.halted.contains(&n) {
                return Err(ClientError::Protocol(A2aError::with_data(
                    ErrorCode::InternalError,
                    "halted: oncall",
                    a2a_protocol_types::halt::halted_detail(),
                )));
            }
            let (tx, rx) = tokio::sync::mpsc::channel(16);
            for ev in self.scripts[n].clone() {
                tx.try_send(Ok(ev)).unwrap();
            }
            Ok(EventStream::from_event_channel(rx))
        })
    }
}

fn status(state: TaskState, handed_back: bool) -> StreamResponse {
    let mut status = TaskStatus::new(state);
    if handed_back {
        let mut note = Message::new(
            a2a_protocol_types::message::MessageId::new("note"),
            MessageRole::Agent,
            vec![Part::text("handed back")],
        );
        note.metadata = Some(serde_json::json!({ HAND_OFF_METADATA_KEY: true }));
        status.message = Some(note);
    }
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t"),
        ContextId::new("c"),
        status,
    ))
}

async fn run(
    scripts: Vec<Vec<StreamResponse>>,
    options: DelegationOptions,
) -> (Outcome, Vec<serde_json::Value>, Duration) {
    Box::pin(run_losing(scripts, options, Vec::new(), None)).await
}

/// [`run`], losing the responses to the sends in `lost`, with the first
/// message keyed `key` when given.
async fn run_losing(
    scripts: Vec<Vec<StreamResponse>>,
    options: DelegationOptions,
    lost: Vec<usize>,
    key: Option<&str>,
) -> (Outcome, Vec<serde_json::Value>, Duration) {
    Box::pin(run_scripted(scripts, options, lost, Vec::new(), key)).await
}

/// [`run_losing`], with the sends in `halted` refused by a halted server.
async fn run_scripted(
    scripts: Vec<Vec<StreamResponse>>,
    options: DelegationOptions,
    lost: Vec<usize>,
    halted: Vec<usize>,
    key: Option<&str>,
) -> (Outcome, Vec<serde_json::Value>, Duration) {
    let sends = Arc::new(Mutex::new(Vec::new()));
    let client = Arc::new(
        ClientBuilder::new("http://stub.invalid")
            .with_custom_transport(Scripted {
                scripts,
                sends: Arc::clone(&sends),
                lost,
                halted,
            })
            .build()
            .unwrap(),
    );
    let mut message = Message::new(
        a2a_protocol_types::message::MessageId::new("first"),
        MessageRole::User,
        vec![Part::text("do it")],
    );
    if let Some(key) = key {
        idempotency::set_key(&mut message, key).unwrap();
    }
    let started = tokio::time::Instant::now();
    let mut params = MessageSendParams::new(message);
    params.tenant = Some("acme".to_owned());
    let child = Delegation::start_with(client, params, options)
        .await
        .unwrap();
    let outcome = Box::pin(child.wait(std::future::pending())).await.outcome;
    let sends = sends.lock().unwrap().clone();
    (outcome, sends, started.elapsed())
}

fn message_of(send: &serde_json::Value) -> &serde_json::Value {
    &send["message"]
}

/// A handed-back child is sent its message again, with a new message id, to
/// its task and context, after the first wait; it then completes.
#[tokio::test(start_paused = true)]
async fn a_handed_back_child_is_sent_again_and_completes() {
    let (outcome, sends, took) = Box::pin(run(
        vec![
            vec![
                status(TaskState::Working, false),
                status(TaskState::Working, true),
            ],
            vec![status(TaskState::Completed, false)],
        ],
        DelegationOptions::new(),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Completed(_)), "{outcome:?}");
    assert_eq!(sends.len(), 2);
    let (first, again) = (message_of(&sends[0]), message_of(&sends[1]));
    assert_eq!(again["taskId"], "t");
    assert_eq!(again["contextId"], "c");
    assert_eq!(
        (&sends[0]["tenant"], &sends[1]["tenant"]),
        (&serde_json::json!("acme"), &serde_json::json!("acme")),
        "the continuation goes to the child's tenant"
    );
    assert_ne!(again["messageId"], first["messageId"]);
    assert_eq!(again["parts"], first["parts"]);
    assert_eq!(took, Duration::from_secs(1), "the first wait");
}

/// The bound holds: two continuations, after 1 s and 2 s, and then the
/// child is lost (no resume here) and cancelled.
#[tokio::test(start_paused = true)]
async fn continuations_stop_at_the_bound() {
    let (outcome, sends, took) = Box::pin(run(
        vec![vec![status(TaskState::Working, true)]],
        DelegationOptions::new()
            .with_hand_off(HandOffPolicy::new(
                2,
                Duration::from_secs(1),
                Duration::from_secs(8),
            ))
            .with_resume(ResumePolicy::none()),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Lost(_)), "{outcome:?}");
    assert_eq!(sends.len(), 3, "the start and two continuations");
    assert_eq!(took, Duration::from_secs(3));
}

/// `HandOffPolicy::never` opts out: the mark is a lost stream.
#[tokio::test(start_paused = true)]
async fn never_does_not_continue() {
    let (outcome, sends, took) = Box::pin(run(
        vec![vec![status(TaskState::Working, true)]],
        DelegationOptions::new()
            .with_hand_off(HandOffPolicy::never())
            .with_resume(ResumePolicy::none()),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Lost(_)), "{outcome:?}");
    assert_eq!(sends.len(), 1);
    assert_eq!(took, Duration::ZERO);
}

/// A child that is working without the mark is not continued.
#[tokio::test(start_paused = true)]
async fn an_unmarked_end_is_resumed_not_continued() {
    let (outcome, sends, _) = Box::pin(run(
        vec![vec![status(TaskState::Working, false)]],
        DelegationOptions::new().with_resume(ResumePolicy::none()),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Lost(_)), "{outcome:?}");
    assert_eq!(sends.len(), 1);
}

#[test]
fn the_waits_double_up_to_the_longest() {
    let p = HandOffPolicy::default();
    assert_eq!(
        p,
        HandOffPolicy::new(3, Duration::from_secs(1), Duration::from_secs(8))
    );
    let waits: Vec<_> = (0..6).map(|n| p.wait(n).as_secs()).collect();
    assert_eq!(waits, [1, 2, 4, 8, 8, 8]);
}

#[test]
fn only_a_working_marked_status_is_handed_back() {
    let marked = |state, mark| match status(state, mark) {
        StreamResponse::StatusUpdate(u) => handed_back(&u.status),
        _ => unreachable!(),
    };
    assert!(marked(TaskState::Working, true));
    assert!(!marked(TaskState::Working, false));
    assert!(!marked(TaskState::Completed, true));
}

/// A status update that hands the turn back (or not) and reports `spent`
/// tokens.
fn reporting(state: TaskState, handed_back: bool, spent: u64) -> StreamResponse {
    let StreamResponse::StatusUpdate(mut update) = status(state, handed_back) else {
        unreachable!()
    };
    let report = a2a_protocol_types::budget::BudgetStatus::new(
        None,
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::from([("tokens".to_owned(), spent)]),
    );
    report.attach_to(&mut update.metadata);
    StreamResponse::StatusUpdate(update)
}

/// COR-03: a continuation carries what the child has left, not its grant
/// again, and the parent is charged what every turn spent. Before the fix
/// the continuation carried the whole granted time and 100 tokens, and the parent was
/// charged the last turn's 20 only.
#[tokio::test(start_paused = true)]
async fn a_continuation_carries_what_is_left_and_every_turn_is_charged() {
    use a2a_protocol_types::budget::{Budget, BudgetHandle};
    let parent = BudgetHandle::start(
        &Budget::new()
            .with_time(Duration::from_secs(60))
            .with_meter("tokens", 1000)
            .unwrap(),
    );
    let grant = parent
        .grant(
            &Budget::new()
                .with_time(Duration::from_secs(60))
                .with_meter("tokens", 100)
                .unwrap(),
        )
        .unwrap();
    let granted_ms = grant.budget().remaining_ms.expect("the grant has a time");
    let (outcome, sends, _) = Box::pin(run(
        vec![
            vec![
                status(TaskState::Working, false),
                reporting(TaskState::Working, true, 30),
            ],
            vec![reporting(TaskState::Completed, false, 20)],
        ],
        DelegationOptions::new().with_budget(grant),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Completed(_)), "{outcome:?}");
    let carried = a2a_protocol_types::budget::Budget::read(
        &serde_json::from_value(message_of(&sends[1]).clone()).unwrap(),
    )
    .unwrap()
    .expect("the continuation carries a budget");
    assert_eq!(
        carried.remaining_ms,
        Some(granted_ms - 1000),
        "the grant less the 1 s wait"
    );
    assert_eq!(
        carried.meters.get("tokens"),
        Some(&70),
        "100 less the 30 reported"
    );
    assert_eq!(parent.remaining("tokens"), Some(950), "charged 30 + 20");
}

/// COR-03's property: over k hand-offs, the time each continuation gives the
/// child, added to the time already gone when it is sent, never exceeds the
/// grant: every turn ends by the grant's one deadline.
#[tokio::test(start_paused = true)]
async fn no_continuation_reaches_past_the_grant() {
    use a2a_protocol_types::budget::{Budget, BudgetHandle};
    let parent = BudgetHandle::start(&Budget::new().with_time(Duration::from_secs(60)));
    let grant = parent
        .grant(&Budget::new().with_time(Duration::from_secs(60)))
        .unwrap();
    let granted_ms = grant.budget().remaining_ms.expect("the grant has a time");
    let handed_back = || vec![status(TaskState::Working, true)];
    let (outcome, sends, _) = Box::pin(run(
        vec![
            handed_back(),
            handed_back(),
            handed_back(),
            vec![status(TaskState::Completed, false)],
        ],
        DelegationOptions::new()
            .with_budget(grant)
            .with_hand_off(HandOffPolicy::new(
                3,
                Duration::from_secs(1),
                Duration::from_secs(8),
            )),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Completed(_)), "{outcome:?}");
    // The waits are 1, 2 and 4 s: the continuations are sent at 1, 3 and 7 s.
    for (send, sent_at_ms) in sends[1..].iter().zip([1000, 3000, 7000]) {
        let carried = Budget::read(&serde_json::from_value(message_of(send).clone()).unwrap())
            .unwrap()
            .expect("a continuation carries a budget");
        assert_eq!(carried.remaining_ms, Some(granted_ms - sent_at_ms));
    }
}

/// A continuation due after the grant's deadline is not sent: the server
/// would refuse a budget with no time left.
#[tokio::test(start_paused = true)]
async fn no_continuation_is_sent_after_the_grant_runs_out() {
    use a2a_protocol_types::budget::{Budget, BudgetHandle};
    let parent = BudgetHandle::start(&Budget::new().with_time(Duration::from_secs(60)));
    let grant = parent
        .grant(&Budget::new().with_time(Duration::from_secs(2)))
        .unwrap();
    assert_eq!(grant.budget().remaining_ms, Some(2000));
    let (outcome, sends, _) = Box::pin(run(
        vec![
            vec![status(TaskState::Working, true)],
            vec![status(TaskState::Working, true)],
            vec![status(TaskState::Completed, false)],
        ],
        DelegationOptions::new()
            .with_budget(grant)
            .with_hand_off(HandOffPolicy::new(
                3,
                Duration::from_secs(1),
                Duration::from_secs(8),
            )),
    ))
    .await;
    // Sent at 0 s and 1 s; the next is due at 3 s, past the 2 s deadline.
    assert_eq!(sends.len(), 2, "{outcome:?}");
    // The delegation falls to resume, which reads the task: the harness's
    // `GetTask` answers `canceled`.
    assert!(matches!(outcome, Outcome::Canceled(_)), "{outcome:?}");
}

/// X2-1: a continuation whose response was lost is sent again as the same
/// request, its message id and idempotency key unchanged, so a server that
/// took the first is not asked to run the turn twice; the next hand-off is a
/// new continuation. Before the fix each retry had a new id and key.
#[tokio::test(start_paused = true)]
async fn a_continuation_sent_again_is_the_same_request() {
    let first_key = "k".repeat(64);
    let (outcome, sends, _) = Box::pin(run_losing(
        vec![
            vec![status(TaskState::Working, true)],
            vec![],
            vec![status(TaskState::Working, true)],
            vec![status(TaskState::Completed, false)],
        ],
        DelegationOptions::new(),
        vec![1],
        Some(&first_key),
    ))
    .await;
    assert!(matches!(outcome, Outcome::Completed(_)), "{outcome:?}");
    assert_eq!(sends.len(), 4);
    let id = |n: usize| message_of(&sends[n])["messageId"].clone();
    let key = |n: usize| {
        let m: Message = serde_json::from_value(message_of(&sends[n]).clone()).unwrap();
        idempotency::key_of(&m).unwrap().unwrap().to_owned()
    };
    assert_eq!(key(0), first_key);
    assert_eq!(
        (id(1), key(1)),
        (id(2), key(2)),
        "the lost send, sent again"
    );
    assert_ne!(key(1), first_key);
    assert_ne!((id(3), key(3)), (id(2), key(2)), "the next hand-off");
}

/// N26-03: a halt is final (COR-05), so a continuation the server refuses as
/// halted is not sent again. Before the fix every continuation the policy
/// allowed was sent (4 sends where 2 are due), and the halt was logged and
/// dropped. Nothing terminal is made up: with no resume the child is lost.
#[tokio::test(start_paused = true)]
async fn a_halted_continuation_is_not_sent_again() {
    let (outcome, sends, took) = Box::pin(run_scripted(
        vec![vec![status(TaskState::Working, true)], vec![]],
        DelegationOptions::new()
            .with_hand_off(HandOffPolicy::new(
                3,
                Duration::from_secs(1),
                Duration::from_secs(8),
            ))
            .with_resume(ResumePolicy::none()),
        Vec::new(),
        vec![1, 2, 3],
        None,
    ))
    .await;
    assert_eq!(sends.len(), 2, "the start and one continuation");
    assert!(matches!(outcome, Outcome::Lost(_)), "{outcome:?}");
    assert_eq!(took, Duration::from_secs(1), "the first wait, and no other");
}
