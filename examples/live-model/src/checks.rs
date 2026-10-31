// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The checks, each against the live agents. A check passes, fails, or —
//! when the model did not take the path the check needs, such as calling a
//! side-effecting tool twice — reports that it was not exercised, which is
//! not a pass.

use std::sync::Arc;
use std::time::{Duration, Instant};

use a2a_protocol_sdk::client::token_provider::{BearerAuthInterceptor, StaticTokenProvider};
use a2a_protocol_sdk::client::{A2aClient, ClientBuilder, ClientError};
use a2a_protocol_sdk::server::HaltScope;
use a2a_protocol_sdk::server::tool_loop::{ANSWER_ARTIFACT_ID, TRACE_ARTIFACT_ID};
use a2a_protocol_sdk::types::approval::{ApprovalDecision, ApprovalRequest};
use a2a_protocol_sdk::types::budget::Budget;
use a2a_protocol_sdk::types::failure::{FailureClass, class_of};
use a2a_protocol_sdk::types::provenance::ContentSigner;
use a2a_protocol_sdk::types::responses::SendMessageResponse;
use a2a_protocol_sdk::types::signing::Jwks;
use a2a_protocol_sdk::types::{
    Message, MessageSendParams, Part, StreamResponse, Task, TaskQueryParams, TaskState,
};

use crate::agents::{Binding, TOKENS, Worker};

/// How a check ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
    NotExercised,
}

/// One check's result.
#[derive(Debug)]
pub struct Check {
    pub name: String,
    pub verdict: Verdict,
    pub detail: String,
    pub seconds: f64,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Runs `body` as the check `name`, timing it; an error is a failure.
pub async fn run<F>(name: impl Into<String>, body: F) -> Check
where
    F: std::future::Future<Output = Result<(Verdict, String)>>,
{
    let name = name.into();
    let started = Instant::now();
    let (verdict, detail) = match body.await {
        Ok(out) => out,
        Err(e) => (Verdict::Fail, format!("error: {e}")),
    };
    Check {
        name,
        verdict,
        detail,
        seconds: started.elapsed().as_secs_f64(),
    }
}

/// The text of artifact `id` on `task`.
pub fn artifact_text(task: &Task, id: &str) -> String {
    task.artifacts
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|a| a.id.0 == id)
        .and_then(|a| a.parts.iter().find_map(Part::text_content))
        .unwrap_or_default()
        .to_owned()
}

fn short(text: &str) -> String {
    let one_line = text.replace('\n', " | ");
    if one_line.chars().count() > 240 {
        format!("{}…", one_line.chars().take(240).collect::<String>())
    } else {
        one_line
    }
}

/// Streams `message` to `client` and returns the stored task, with the
/// states the stream reported, after `on_working` ran at the first
/// `WORKING`.
async fn stream(
    client: &A2aClient,
    message: Message,
    mut on_working: impl FnMut() -> Option<
        std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    >,
) -> Result<(Task, Vec<TaskState>)> {
    let mut events = client
        .stream_message(MessageSendParams::new(message))
        .await?;
    let mut task_id = None;
    let mut states = Vec::new();
    let mut fired = false;
    while let Some(event) = events.next().await {
        let state = match event? {
            StreamResponse::Task(t) => {
                task_id = Some(t.id.clone());
                Some(t.status.state)
            }
            StreamResponse::StatusUpdate(u) => {
                task_id.get_or_insert(u.task_id.clone());
                Some(u.status.state)
            }
            _ => None,
        };
        if let Some(state) = state {
            states.push(state);
            if state == TaskState::Working && !fired {
                fired = true;
                if let Some(hook) = on_working() {
                    hook.await;
                }
            }
        }
    }
    let id = task_id.ok_or("the stream named no task")?;
    Ok((client.get_task(TaskQueryParams::new(id.0)).await?, states))
}

/// Streams the billing question on `binding`, verifying every signed part
/// against the worker's key: the tool loop, streaming and provenance. A
/// completed run whose trace has no tool call is not exercised.
pub async fn stream_on(worker: &Worker, binding: Binding) -> Result<(Verdict, String)> {
    let client = worker.client(binding, worker.keys.clone()).await?;
    let ask = Message::user_text(
        uuid::Uuid::new_v4().to_string(),
        "How is the billing service doing?",
    );
    let (task, states) = stream(&client, ask, || None).await?;
    let answer = artifact_text(&task, ANSWER_ARTIFACT_ID);
    let trace = artifact_text(&task, TRACE_ARTIFACT_ID);
    // A model that answers without calling a tool has not exercised the
    // loop, whatever it said.
    let verdict = match (
        task.status.state,
        answer.trim().is_empty(),
        trace.trim().is_empty(),
    ) {
        (TaskState::Completed, false, false) => Verdict::Pass,
        (TaskState::Completed, false, true) => Verdict::NotExercised,
        _ => Verdict::Fail,
    };
    Ok((
        verdict,
        format!(
            "{}; streamed {:?}; answer: {}; trace: {}",
            task.status.state,
            states,
            short(&answer),
            short(&trace)
        ),
    ))
}

/// Streams a question and watches the artifacts: each turn's text arrives
/// in a draft before the answer, and the last turn's draft is the answer
/// (audit L5). A model that sent no piece has not exercised it.
pub async fn drafts(worker: &Worker) -> Result<(Verdict, String)> {
    let client = worker.client(Binding::JsonRpc, worker.keys.clone()).await?;
    let ask = Message::user_text(
        uuid::Uuid::new_v4().to_string(),
        "How is the billing service doing?",
    );
    let mut events = client.stream_message(MessageSendParams::new(ask)).await?;
    let mut drafts: Vec<(String, String)> = Vec::new();
    let mut answer = None;
    let mut before = 0;
    while let Some(event) = events.next().await {
        if let StreamResponse::ArtifactUpdate(a) = event? {
            let text: String = a
                .artifact
                .parts
                .iter()
                .filter_map(Part::text_content)
                .collect();
            if a.artifact.id.0 == ANSWER_ARTIFACT_ID {
                answer = Some(text);
                before = drafts.len();
            } else if a.artifact.id.0.starts_with("draft-") {
                drafts.push((a.artifact.id.0, text));
            }
        }
    }
    let answer = answer.ok_or("no answer artifact")?;
    let last = drafts.last().map(|(id, _)| id.clone()).unwrap_or_default();
    let last_text: String = drafts
        .iter()
        .filter(|(id, _)| *id == last)
        .map(|(_, t)| t.as_str())
        .collect();
    let verdict = if drafts.is_empty() {
        Verdict::NotExercised
    } else if before == drafts.len() && last_text == answer {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    let turns: std::collections::BTreeSet<&str> =
        drafts.iter().map(|(id, _)| id.as_str()).collect();
    Ok((
        verdict,
        format!(
            "{} draft events over {:?} before the answer; last draft is the answer: {}; answer: {}",
            drafts.len(),
            turns,
            last_text == answer,
            short(&answer)
        ),
    ))
}

/// A client holding a key the worker did not sign with refuses its signed
/// content.
pub async fn provenance_refused(worker: &Worker) -> Result<(Verdict, String)> {
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .map_err(|_| "could not generate a key")?;
    let other = ContentSigner::ed25519("someone-else", pkcs8.as_ref())?;
    let client = worker
        .client(Binding::JsonRpc, Jwks::new(vec![other.public_jwk()]))
        .await?;
    let ask = Message::user_text(uuid::Uuid::new_v4().to_string(), "List the services.");
    match stream(&client, ask, || None).await {
        Err(e) => match e.downcast_ref::<ClientError>() {
            Some(ClientError::ContentNotVerified(why)) => {
                Ok((Verdict::Pass, format!("refused: {}", short(why))))
            }
            _ => Ok((Verdict::Fail, format!("failed otherwise: {e}"))),
        },
        Ok((task, _)) => Ok((
            Verdict::Fail,
            format!(
                "accepted content signed by another key: {}",
                task.status.state
            ),
        )),
    }
}

/// Asks for the same restart twice: the loop runs a side-effecting call once
/// and answers the repeat from the first, with an idempotency key on it.
pub async fn idempotency(worker: &Worker) -> Result<(Verdict, String)> {
    let client = worker.client(Binding::JsonRpc, worker.keys.clone()).await?;
    let ask = Message::user_text(
        uuid::Uuid::new_v4().to_string(),
        "Restart the checkout service. Then call restart_service for checkout a second time \
         to make sure the restart took.",
    );
    let (task, _) = stream(&client, ask, || None).await?;
    let trace = artifact_text(&task, TRACE_ARTIFACT_ID);
    let requested = trace.matches("restart_service(").count();
    let executed = worker.restarts.count("checkout");
    let keys = worker.restarts.keys("checkout");
    let keyed = keys
        .iter()
        .all(|k| k.len() == 64 && k.bytes().all(|b| b.is_ascii_hexdigit()));
    let detail = format!(
        "{}; model asked for {requested} restart(s), {executed} ran; keys: {keys:?}; trace: {}",
        task.status.state,
        short(&trace)
    );
    let verdict = match (requested, executed, keyed) {
        (r, 1, true) if r >= 2 => Verdict::Pass,
        (1, 1, true) => Verdict::NotExercised,
        _ => Verdict::Fail,
    };
    Ok((verdict, detail))
}

/// A budget of one token: the first turn's reported usage spends it, and the
/// task fails `budget-exhausted`.
pub async fn budget_exhausted(worker: &Worker) -> Result<(Verdict, String)> {
    let client = worker.client(Binding::JsonRpc, worker.keys.clone()).await?;
    let mut ask = Message::user_text(uuid::Uuid::new_v4().to_string(), "List the services.");
    Budget::new().with_meter("tokens", 1)?.attach(&mut ask);
    let (task, _) = stream(&client, ask, || None).await?;
    let class = task.status.message.as_ref().and_then(class_of);
    let reason = task
        .status
        .message
        .as_ref()
        .and_then(Message::text)
        .unwrap_or_default()
        .to_owned();
    let verdict =
        if task.status.state == TaskState::Failed && class == Some(FailureClass::BudgetExhausted) {
            Verdict::Pass
        } else {
            Verdict::Fail
        };
    Ok((
        verdict,
        format!("{}; class {class:?}; {}", task.status.state, short(&reason)),
    ))
}

/// The coordinator delegates to the worker under a budget, and relays its
/// answer.
pub async fn delegation(coordinator: &str) -> Result<(Verdict, String)> {
    let client = ClientBuilder::new(coordinator)
        .with_timeout(Duration::from_secs(120))
        .build()?;
    let mut ask = Message::user_text(
        uuid::Uuid::new_v4().to_string(),
        "What version of the search service is deployed?",
    );
    Budget::new()
        .with_time(Duration::from_secs(900))
        .with_meter("tokens", 200_000)?
        .attach(&mut ask);
    let (task, _) = stream(&client, ask, || None).await?;
    let relay = artifact_text(&task, "relay");
    let verdict = if task.status.state == TaskState::Completed && relay.starts_with("worker task ")
    {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    Ok((
        verdict,
        format!(
            "{}; relay: {}{}",
            task.status.state,
            short(&relay),
            status_text(&task)
        ),
    ))
}

/// Halts the worker while a run is working: the run ends `CANCELED`, a new
/// send is refused, and resuming lets sends through again.
pub async fn halt(worker: &Worker) -> Result<(Verdict, String)> {
    let client = worker.client(Binding::JsonRpc, worker.keys.clone()).await?;
    let ask = Message::user_text(
        uuid::Uuid::new_v4().to_string(),
        "Check the status of every service in the inventory, one at a time, and report each.",
    );
    let handler = Arc::clone(&worker.handler);
    let (task, states) = stream(&client, ask, move || {
        let handler = Arc::clone(&handler);
        Some(Box::pin(async move {
            handler.halt(HaltScope::All, "live-model", "drill").await;
        }))
    })
    .await?;
    let refused = client
        .send_message(MessageSendParams::new(Message::user_text(
            uuid::Uuid::new_v4().to_string(),
            "List the services.",
        )))
        .await;
    let resumed = worker.handler.resume(HaltScope::All, "live-model").await?;
    let detail = format!(
        "run {} (streamed {states:?}); send while halted: {}; resumed: {resumed}",
        task.status.state,
        match &refused {
            Ok(_) => "accepted".to_owned(),
            Err(e) => format!("refused ({e})"),
        }
    );
    let verdict = match (task.status.state, refused.is_err(), resumed) {
        (TaskState::Canceled, true, true) => Verdict::Pass,
        (TaskState::Completed, _, _) => Verdict::NotExercised,
        _ => Verdict::Fail,
    };
    Ok((verdict, detail))
}

fn as_user(url: &str, token: &str) -> Result<A2aClient> {
    Ok(ClientBuilder::new(url)
        .with_timeout(Duration::from_secs(120))
        .with_interceptor(BearerAuthInterceptor::new(Arc::new(
            StaticTokenProvider::new(token),
        )))
        .build()?)
}

async fn send_task(client: &A2aClient, message: Message) -> Result<Task> {
    match client.send_message(MessageSendParams::new(message)).await? {
        SendMessageResponse::Task(task) => Ok(task),
        other => Err(format!("expected a task, got {other:?}").into()),
    }
}

/// The approver's model writes a plan; the task waits for two distinct
/// approvers, refusing the requester's own approval, and completes naming
/// both.
pub async fn quorum(approver: &str) -> Result<(Verdict, String)> {
    let requester = as_user(approver, TOKENS[0].0)?;
    let bob = as_user(approver, TOKENS[1].0)?;
    let carol = as_user(approver, TOKENS[2].0)?;
    let asked = send_task(
        &requester,
        Message::user_text(uuid::Uuid::new_v4().to_string(), "Prepare the restart."),
    )
    .await?;
    let request = asked
        .status
        .message
        .as_ref()
        .map(ApprovalRequest::read)
        .transpose()?
        .flatten()
        .ok_or("the task asked for no approval")?;
    let answer = |id: &str| {
        let mut m = Message::user_text(id, "approved");
        m.task_id = Some(asked.id.clone());
        m.context_id = Some(asked.context_id.clone());
        ApprovalDecision::approve(&request).attach(&mut m);
        m
    };
    let own = send_task(&requester, answer("own")).await;
    let first = send_task(&bob, answer("bob")).await?;
    let second = send_task(&carol, answer("carol")).await?;
    let approved = artifact_text(&second, "approved");
    let detail = format!(
        "asked: {} ({}); requester's own approval: {}; after bob: {}; after carol: {}: {}",
        asked.status.state,
        short(&request.summary),
        if own.is_err() { "refused" } else { "accepted" },
        first.status.state,
        second.status.state,
        short(&approved)
    );
    let verdict = if asked.status.state == TaskState::InputRequired
        && own.is_err()
        && first.status.state == TaskState::InputRequired
        && second.status.state == TaskState::Completed
        && approved.contains("bearer:bob")
        && approved.contains("bearer:carol")
    {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    Ok((verdict, detail))
}

/// The task's status message, if it has one, for a detail line.
fn status_text(task: &Task) -> String {
    task.status
        .message
        .as_ref()
        .and_then(Message::text)
        .map(|t| format!("; status: {}", short(t)))
        .unwrap_or_default()
}
