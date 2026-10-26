// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `shutdown_signal` ends on the signals a platform sends, and a process that
//! waits on it exits through `main` (so a `Telemetry` guard there is
//! dropped and flushes) instead of being killed.
//!
//! Signals are process state, so each test runs this binary again as a child
//! and signals it from outside, with `kill(1)` (this crate forbids `unsafe`,
//! and so `libc::kill`).

#![cfg(unix)]

use std::io::{BufRead as _, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

const CHILD: &str = "A2A_SHUTDOWN_SIGNAL_CHILD";

/// Runs only as the child: registers the signal future, says so, waits for the
/// signal, then runs what a server's `main` runs after it (the "flush").
#[tokio::test]
async fn wait_as_the_child_process() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let signal = a2a_protocol_server::shutdown_signal();
    println!("registered");
    signal.await;
    // What follows `serve_with_shutdown` in `main`: reached only if the
    // process was not killed by the signal.
    println!("flushing after the signal");
}

/// Starts the child, waits until it has registered, sends `signal`, and
/// returns its exit status success flag, its stdout, and how long it took to
/// end after being signalled.
fn signalled(signal: &str) -> (bool, String, Duration) {
    let mut child = Command::new(std::env::current_exe().expect("exe"))
        .args(["wait_as_the_child_process", "--exact", "--nocapture"])
        .env(CHILD, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("child starts");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();
    let mut seen = String::new();
    for line in lines.by_ref() {
        let line = line.expect("line");
        seen.push_str(&line);
        seen.push('\n');
        if line == "registered" {
            break;
        }
    }
    assert!(
        seen.contains("registered"),
        "child never registered: {seen}"
    );
    let sent = Command::new("kill")
        .args([&format!("-{signal}"), &child.id().to_string()])
        .status()
        .expect("kill(1) runs");
    assert!(sent.success(), "kill -{signal} failed");
    let at = std::time::Instant::now();
    for line in lines {
        seen.push_str(&line.expect("line"));
        seen.push('\n');
    }
    let status = child.wait().expect("child ends");
    (status.success(), seen, at.elapsed())
}

#[test]
fn sigterm_ends_the_wait_and_the_process_goes_on_to_flush() {
    let (ok, out, took) = signalled("TERM");
    assert!(
        ok,
        "the child was killed or failed instead of returning: {out}"
    );
    assert!(out.contains("flushing after the signal"), "{out}");
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn sigint_does_too() {
    let (ok, out, _) = signalled("INT");
    assert!(ok, "{out}");
    assert!(out.contains("flushing after the signal"), "{out}");
}

/// The negative control: with no handler registered, the same signal ends the
/// process before anything after the wait runs — which is what `hello-agent`
/// did, and why its telemetry was lost.
#[test]
fn without_the_handler_sigterm_kills_the_process_before_it_flushes() {
    let mut child = Command::new(std::env::current_exe().expect("exe"))
        .args([
            "wait_forever_as_the_child_process",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("child starts");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();
    for line in lines.by_ref() {
        if line.expect("line") == "waiting" {
            break;
        }
    }
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .expect("kill runs")
            .success()
    );
    let rest: String = lines.map(|l| l.expect("line")).collect();
    let status = child.wait().expect("child ends");
    assert!(!status.success(), "ended by the signal, not by returning");
    assert!(!rest.contains("flushing"), "{rest}");
}

/// Runs only as the child: waits with no signal handling at all.
#[tokio::test]
async fn wait_forever_as_the_child_process() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    println!("waiting");
    std::future::pending::<()>().await;
    println!("flushing");
}
