// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The module's own parts: the marks on an error, and what the wrapper passes on.

use super::*;

// -- the module's own parts --------------------------------------------------

#[test]
fn a_refusal_carries_its_status_and_a_decision_is_marked_on_the_error() {
    let err = rejected("refused".to_owned(), 410);
    assert_eq!(status_of(&err), Some(410));
    assert_eq!(outcome_of(&err), None, "a bare refusal decides nothing");
    assert_eq!(status_of(&A2aError::internal("no data")), None);

    let gone = marked(err, push_outcome::GONE);
    assert_eq!(outcome_of(&gone), Some(push_outcome::GONE));
    assert_eq!(status_of(&gone), Some(410), "the status survives the mark");
    let suspended = marked(A2aError::internal("skipped"), push_outcome::SUSPENDED);
    assert_eq!(outcome_of(&suspended), Some(push_outcome::SUSPENDED));

    let mut unknown = A2aError::internal("x");
    unknown.data = Some(serde_json::json!({ OUTCOME_KEY: "something-else" }));
    assert_eq!(outcome_of(&unknown), None);
}

#[test]
fn both_rules_off_leaves_the_sender_as_it_was() {
    let (inner, _) = Scripted::always(failing);
    let inner: Arc<dyn PushSender> = inner;
    let off = watched(Arc::clone(&inner), store(), &limits(0, false));
    assert!(Arc::ptr_eq(&inner, &off), "nothing to watch for");
    for (after, remove) in [(1, false), (0, true), (5, true)] {
        let on = watched(Arc::clone(&inner), store(), &limits(after, remove));
        assert!(!Arc::ptr_eq(&inner, &on), "after={after} remove={remove}");
    }
}

#[test]
fn what_the_wrapped_sender_says_about_urls_is_passed_on() {
    let (inner, _) = Scripted::always(failing);
    let health = health(inner, store(), 5, true);
    assert!(health.allows_private_urls());
    assert!(health.allows_webhook_url("https://x/a"));
    assert!(!health.allows_webhook_url("https://x/b"));
    assert!(health.requires_https());
    assert_eq!(
        health.max_delivery_duration(),
        Some(Duration::from_secs(38))
    );
}

#[test]
fn the_debug_form_names_the_policy_and_not_the_state() {
    let (inner, _) = Scripted::always(failing);
    let shown = format!("{:?}", health(inner, store(), 7, false));
    assert!(shown.starts_with("PushHealth"), "{shown}");
    assert!(shown.contains("suspend_after: 7"), "{shown}");
    assert!(shown.contains("remove_gone: false"), "{shown}");
    assert!(shown.contains("suspend_for: 10s"), "{shown}");
}
