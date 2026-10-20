// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! a2a.proto `ListTasksRequest.page_size`: "The service may return fewer
//! than this value." A store configured with a smaller maximum page returns
//! that many, with a token for the rest, and following the tokens lists
//! every task once.

use std::collections::HashSet;

use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder, TaskStoreConfig, agent_executor};
use a2a_protocol_types::Message;
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams};
use a2a_protocol_types::task::TaskState;

struct Done;
agent_executor!(Done, |ctx, queue| async {
    EventEmitter::new(ctx, queue)
        .status(TaskState::Completed)
        .await
});

#[tokio::test]
async fn list_tasks_may_return_fewer_than_the_page_size_asked_for() {
    let handler = RequestHandlerBuilder::new(Done)
        .without_agent_card()
        .with_task_store_config(TaskStoreConfig::default().with_max_page_size(2))
        .build()
        .unwrap();
    for i in 0..5 {
        handler
            .on_send_message(
                MessageSendParams::new(Message::user_text(format!("m-{i}"), "go")),
                false,
                None,
            )
            .await
            .unwrap();
    }

    let mut seen = HashSet::new();
    let mut token: Option<String> = None;
    let mut pages = Vec::new();
    loop {
        let page = handler
            .on_list_tasks(
                ListTasksParams {
                    page_size: Some(4),
                    page_token: token.clone(),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        pages.push(page.tasks.len());
        for task in &page.tasks {
            assert!(seen.insert(task.id.0.clone()), "{} listed twice", task.id.0);
        }
        if page.next_page_token.is_empty() {
            break;
        }
        token = Some(page.next_page_token);
        assert!(pages.len() < 10, "the listing does not end: {pages:?}");
    }

    assert_eq!(
        pages,
        [2, 2, 1],
        "four were asked for each time; the store's limit of two is lower"
    );
    assert_eq!(seen.len(), 5, "every task is listed once");
}
