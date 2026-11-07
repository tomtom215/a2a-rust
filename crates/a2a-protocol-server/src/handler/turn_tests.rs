// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a turn says when its executor returns with steering messages it never
//! read (ADR 0036): one warning naming the task and the count, and none when
//! nothing was left. The counter is checked with the handler, in
//! `messaging::steer_tests`; this is the log line beside it.

#![cfg(feature = "tracing")]

use std::sync::{Arc, Mutex};

use a2a_protocol_types::message::Message;
use a2a_protocol_types::task::TaskId;
use tracing_subscriber::layer::SubscriberExt as _;

use super::ExecutorTurn;
use crate::metrics::NoopMetrics;
use crate::steering::{Inbox, Steered};

/// Every `WARN` event, as `message field=value ...`, in order.
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Warnings {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        #[derive(Default)]
        struct Fields {
            message: String,
            rest: Vec<String>,
        }
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.message = format!("{v:?}");
                } else {
                    self.rest.push(format!("{}={v:?}", f.name()));
                }
            }
        }
        if *event.metadata().level() == tracing::Level::WARN {
            let mut fields = Fields::default();
            event.record(&mut fields);
            let line = format!("{} {}", fields.message, fields.rest.join(" "));
            self.0.lock().expect("lock").push(line);
        }
    }
}

/// Closes a turn whose inbox holds `unread` messages, and returns the warnings
/// that said so.
fn warnings_on_closing_with(unread: usize) -> Vec<String> {
    let warnings = Warnings::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(warnings.clone()));
    let inbox = Arc::new(Inbox::new(4));
    for n in 0..unread {
        let message = Message::user_text(format!("m{n}"), "steer");
        inbox
            .try_push(Steered::new(message, None))
            .expect("room for it");
    }
    ExecutorTurn::with_inbox(Some(inbox)).close_inbox(&NoopMetrics, &TaskId::new("task-9"));
    warnings.0.lock().expect("lock").clone()
}

/// Nothing left, one left, two left: no warning, then one that counts them.
#[test]
fn the_warning_is_said_when_something_was_left_and_not_otherwise() {
    assert_eq!(warnings_on_closing_with(0), Vec::<String>::new());

    for unread in [1_usize, 2] {
        let said = warnings_on_closing_with(unread);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with("the executor returned with steering messages it never read"),
            "{said:?}"
        );
        assert!(said[0].contains("task_id=task-9"), "{said:?}");
        assert!(said[0].contains(&format!("unread={unread}")), "{said:?}");
    }
}

/// A turn that takes no steering has no inbox to close and says nothing.
#[test]
fn a_turn_without_an_inbox_closes_quietly() {
    let warnings = Warnings::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(warnings.clone()));
    ExecutorTurn::with_inbox(None).close_inbox(&NoopMetrics, &TaskId::new("task-9"));
    assert_eq!(
        warnings.0.lock().expect("lock").clone(),
        Vec::<String>::new()
    );
}
