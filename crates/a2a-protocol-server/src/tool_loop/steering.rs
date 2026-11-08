// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Reading a turn's steering inbox between model turns (ADR 0036): each
//! message's text becomes a user turn in the conversation.

use a2a_protocol_types::budget::BudgetHandle;

use super::model::{ChatMessage, TextSink};
use super::run::LoopError;
use super::run_support::out_of_time;
use crate::steering::Inbox;

/// What a run reads and writes beside its prompt: the drafts it streams out,
/// the inbox it reads from and the sink its trace lines are copied to.
#[derive(Clone, Copy, Default)]
pub(super) struct Side<'a> {
    /// Each turn's text, as it arrives, with the turn's number.
    pub(super) drafts: Option<&'a tokio::sync::mpsc::UnboundedSender<(usize, String)>>,
    /// Where steering messages wait; `None` reads none.
    pub(super) inbox: Option<&'a Inbox>,
    /// Where each trace line is also put as it is made, for a run that fails.
    pub(super) trace: Option<&'a super::trace::Sink>,
}

impl Side<'_> {
    /// The sink for the text of turn number `turn`, when drafts are on.
    pub(super) fn sink(&self, turn: usize) -> Option<TextSink> {
        self.drafts.map(|tx| TextSink::new(turn, tx.clone()))
    }
}

/// What a run does before each model turn: refuses it when the budget is
/// spent (nothing is read then), and otherwise adds what the caller said
/// since the last turn to `messages`.
///
/// # Errors
///
/// [`LoopError::Budget`], as [`out_of_time`] reports it.
pub(super) fn before_turn(
    budget: Option<&BudgetHandle>,
    inbox: Option<&Inbox>,
    messages: &mut Vec<ChatMessage>,
) -> Result<(), LoopError> {
    out_of_time(budget)?;
    messages.extend(pending(inbox));
    Ok(())
}

/// The user turns `inbox` holds now, oldest first, each the text parts of one
/// message joined by newlines. A message with no text (a data part alone) is
/// taken from the inbox and adds nothing: there is nothing to say to a model.
pub(super) fn pending(inbox: Option<&Inbox>) -> Vec<ChatMessage> {
    let Some(inbox) = inbox else {
        return Vec::new();
    };
    std::iter::from_fn(|| inbox.try_recv())
        .map(|steered| steered.message.texts().collect::<Vec<_>>().join("\n"))
        .filter(|text| !text.trim().is_empty())
        .map(ChatMessage::User)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use a2a_protocol_types::message::{Message, Part};
    use serde_json::Value;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::steering::Steered;
    use crate::tool_loop::model::{ChatRequest, ModelError, ModelTurn, ToolCall, ToolModel};
    use crate::tool_loop::run::ToolLoop;
    use crate::tool_loop::tool::{Tool, ToolError, ToolInvocation, ToolSet};

    fn steer(id: &str, text: &str) -> Steered {
        Steered::new(Message::user_text(id, text), Some("alice".to_owned()))
    }

    #[test]
    fn no_inbox_and_an_empty_one_add_nothing() {
        assert_eq!(pending(None), Vec::new());
        assert_eq!(pending(Some(&Inbox::new(2))), Vec::new());
    }

    #[test]
    fn each_message_is_a_user_turn_in_the_order_sent_and_is_taken() {
        let inbox = Inbox::new(4);
        inbox.try_push(steer("a", "first")).expect("room");
        inbox.try_push(steer("b", "second")).expect("room");
        assert_eq!(
            pending(Some(&inbox)),
            [
                ChatMessage::User("first".to_owned()),
                ChatMessage::User("second".to_owned())
            ]
        );
        assert!(inbox.is_empty(), "what was read is not read again");
        assert_eq!(pending(Some(&inbox)), Vec::new());
    }

    #[test]
    fn a_message_of_several_text_parts_is_one_turn_and_data_is_not_text() {
        let mut message = Message::user_text("m", "one");
        message.parts.push(Part::data(serde_json::json!({"k": 1})));
        message.parts.push(Part::text("two"));
        let inbox = Inbox::new(4);
        inbox.try_push(Steered::new(message, None)).expect("room");
        assert_eq!(
            pending(Some(&inbox)),
            [ChatMessage::User("one\ntwo".to_owned())]
        );
    }

    #[test]
    fn a_message_with_no_text_is_taken_and_adds_no_turn() {
        let mut data_only = Message::user_text("m", "x");
        data_only.parts = vec![Part::data(serde_json::json!({"k": 1}))];
        let blank = Message::user_text("n", "  \n ");
        let inbox = Inbox::new(4);
        inbox.try_push(Steered::new(data_only, None)).expect("room");
        inbox.try_push(Steered::new(blank, None)).expect("room");
        inbox.try_push(steer("o", "real")).expect("room");
        assert_eq!(
            pending(Some(&inbox)),
            [ChatMessage::User("real".to_owned())]
        );
        assert!(inbox.is_empty());
    }

    /// What the model was sent on each turn, and the replies it gives.
    struct Replies {
        replies: Mutex<Vec<ModelTurn>>,
        seen: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
    }

    impl ToolModel for Replies {
        fn model_name(&self) -> &'static str {
            "scripted"
        }
        fn provider_name(&self) -> &'static str {
            "test"
        }
        fn complete<'a>(
            &'a self,
            request: ChatRequest<'a>,
        ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
            self.seen.lock().unwrap().push(request.messages.to_vec());
            let reply = self.replies.lock().unwrap().remove(0);
            Box::pin(async move { Ok(reply) })
        }
    }

    /// A tool that, when called, puts a steering message in the inbox: so the
    /// message arrives between two model turns, at a moment the test fixes.
    struct Steering(Arc<Inbox>);

    impl Tool for Steering {
        fn name(&self) -> &'static str {
            "work"
        }
        fn description(&self) -> &'static str {
            "does some work"
        }
        fn parameters(&self) -> Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        fn call<'a>(
            &'a self,
            _call: ToolInvocation,
        ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
            Box::pin(async move {
                self.0
                    .try_push(steer("late", "use the cheaper index"))
                    .expect("room");
                Ok("done".to_owned())
            })
        }
    }

    fn reply(text: &str, calls: Vec<ToolCall>) -> ModelTurn {
        ModelTurn::new(text, calls)
    }

    #[tokio::test]
    async fn a_message_that_arrives_during_a_tool_call_is_the_next_turns_last_user_turn() {
        let inbox = Arc::new(Inbox::new(4));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let model = Replies {
            replies: Mutex::new(vec![
                reply("", vec![ToolCall::new("c1", "work", "{}")]),
                reply("answered", Vec::new()),
            ]),
            seen: Arc::clone(&seen),
        };
        let tool_loop = ToolLoop::new(
            model,
            ToolSet::new().with(Steering(Arc::clone(&inbox))).unwrap(),
        );
        // A message that was already waiting when the run began is read
        // before the first turn.
        inbox
            .try_push(steer("early", "start with the cheap one"))
            .expect("room");
        let side = Side {
            drafts: None,
            inbox: Some(&inbox),
            trace: None,
        };
        let answer = tool_loop
            .run_drafting("scope", "do it", &CancellationToken::new(), None, side)
            .await
            .expect("the run answers");
        assert_eq!(answer.text, "answered");

        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            seen[0],
            [
                ChatMessage::User("do it".to_owned()),
                ChatMessage::User("start with the cheap one".to_owned()),
            ],
            "the message that was waiting is read before the first turn"
        );
        assert_eq!(
            seen[1].last(),
            Some(&ChatMessage::User("use the cheaper index".to_owned())),
            "the message that arrived during the call is read before the next turn, \
             after the call's result: {:?}",
            seen[1]
        );
        assert!(matches!(
            seen[1][seen[1].len() - 2],
            ChatMessage::ToolResult { .. }
        ));
        assert!(inbox.is_empty());
    }

    #[test]
    fn a_turn_reads_the_inbox_unless_the_budget_is_spent_and_then_reads_nothing() {
        let inbox = Inbox::new(4);
        inbox.try_push(steer("a", "first")).expect("room");
        let mut messages = vec![ChatMessage::User("prompt".to_owned())];
        before_turn(None, Some(&inbox), &mut messages).expect("no budget to spend");
        assert_eq!(
            messages,
            [
                ChatMessage::User("prompt".to_owned()),
                ChatMessage::User("first".to_owned())
            ]
        );

        // A spent deadline stops the turn before it reads anything.
        inbox.try_push(steer("b", "second")).expect("room");
        let spent = BudgetHandle::start_at(
            &a2a_protocol_types::budget::Budget::new().with_time(Duration::ZERO),
            std::time::Instant::now(),
        );
        let refused = before_turn(Some(&spent), Some(&inbox), &mut messages);
        assert!(matches!(refused, Err(LoopError::Budget(_))), "{refused:?}");
        assert_eq!(messages.len(), 2, "nothing was added");
        assert_eq!(inbox.len(), 1, "and nothing was taken");
    }

    #[test]
    fn a_sink_is_made_for_the_turn_only_when_drafts_are_on() {
        assert!(Side::default().sink(3).is_none());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let side = Side {
            drafts: Some(&tx),
            inbox: None,
            trace: None,
        };
        let sink = side.sink(3).expect("drafts are on");
        sink.push("hello");
        assert_eq!(
            rx.try_recv().expect("the sink sends to the channel"),
            (3, "hello".to_owned())
        );
    }

    #[tokio::test]
    async fn a_run_with_no_inbox_never_reads_one() {
        let inbox = Arc::new(Inbox::new(4));
        inbox.try_push(steer("a", "ignored")).expect("room");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let model = Replies {
            replies: Mutex::new(vec![reply("answered", Vec::new())]),
            seen: Arc::clone(&seen),
        };
        let tool_loop = ToolLoop::new(model, ToolSet::new());
        tool_loop
            .run_drafting(
                "scope",
                "do it",
                &CancellationToken::new(),
                None,
                Side::default(),
            )
            .await
            .expect("answers");
        assert_eq!(
            seen.lock().unwrap().clone(),
            [vec![ChatMessage::User("do it".to_owned())]]
        );
        assert_eq!(
            inbox.len(),
            1,
            "an inbox nobody gave the run is not drained"
        );
    }
}
