// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The resume policy in [`DelegationOptions`] is the one the handle uses, on a
//! fresh start and on a reattach.
//!
//! The policy asked for is `none`: the first lost stream ends the delegation.
//! The default would ask the server again five times, waiting 200 ms and
//! doubling, so what tells the two apart is the number of connections the
//! proxy accepted after the cut, which no timing decides.

use std::time::Duration;

use a2a_protocol_client::delegation::{Delegation, DelegationOptions, Outcome, ResumePolicy};

use super::harness::*;

/// The most connections a delegation that does not resume makes after the
/// cut: the cancel it sends. The default policy makes at least ten (five
/// reopenings and a look at the task after each).
const NO_RESUME: usize = 3;

/// Cuts the link under `d`, keeps it down, and says what the delegation made
/// of that and how many connections it tried.
async fn cut_and_wait(d: Delegation, proxy: &Proxy) -> (Outcome, usize) {
    let waiting = tokio::spawn(d.wait(std::future::pending()));
    proxy.block(true);
    let before = proxy.accepted();
    proxy.cut();
    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("bounded")
        .expect("joined");
    (done.outcome, proxy.accepted() - before)
}

#[tokio::test]
async fn a_start_with_resume_policy_is_the_one_the_handle_uses() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let hold = key();
    let mut d = Delegation::start_with(
        client_for(proxy.addr),
        job(&hold),
        DelegationOptions::new().with_resume(ResumePolicy::none()),
    )
    .await
    .expect("start");
    until_named(&mut d).await;

    let (outcome, tried) = cut_and_wait(d, &proxy).await;
    assert!(matches!(outcome, Outcome::Lost(_)), "{outcome:?}");
    assert!(tried <= NO_RESUME, "{tried} connections after the cut");
    gate(&hold).notify_one();
}

#[tokio::test]
async fn a_reattach_with_resume_policy_is_the_one_the_handle_uses() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let hold = key();
    let mut first = Delegation::start(client_for(upstream), job(&hold))
        .await
        .expect("start");
    let id = until_named(&mut first).await;
    assert_eq!(first.detach().as_deref(), Some(id.as_str()));

    let d = Delegation::reattach_with(
        client_for(proxy.addr),
        None,
        id,
        None,
        DelegationOptions::new().with_resume(ResumePolicy::none()),
    )
    .await
    .expect("reattach");

    let (outcome, tried) = cut_and_wait(d, &proxy).await;
    assert!(matches!(outcome, Outcome::Lost(_)), "{outcome:?}");
    assert!(tried <= NO_RESUME, "{tried} connections after the cut");
    gate(&hold).notify_one();
}

/// A policy that does resume, asked for through the options, resumes: the
/// link comes back and the child finishes.
#[tokio::test]
async fn a_resuming_policy_in_the_options_resumes() {
    let upstream = serve().await;
    let proxy = Proxy::start(upstream).await;
    let hold = key();
    let mut d = Delegation::start_with(
        client_for(proxy.addr),
        job(&hold),
        DelegationOptions::new().with_resume(
            ResumePolicy::default()
                .with_backoff(Duration::from_millis(10), Duration::from_millis(50)),
        ),
    )
    .await
    .expect("start");
    until_named(&mut d).await;
    let waiting = tokio::spawn(d.wait(std::future::pending()));
    proxy.cut();
    gate(&hold).notify_one();
    let done = tokio::time::timeout(GUARD, waiting)
        .await
        .expect("bounded")
        .expect("joined");
    assert!(matches!(done.outcome, Outcome::Completed(_)), "{done:?}");
}
