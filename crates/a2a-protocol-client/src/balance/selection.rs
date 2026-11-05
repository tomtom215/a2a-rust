// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Which endpoint a call goes to (ADR 0050 decisions 2 and 4).
//!
//! The fewest calls in flight wins; a tie goes to the endpoint chosen
//! longest ago, so calls one after another go round the set in turn. The
//! count in flight is the origin's: on a pool it counts every client's calls.
//!
//! Ejected endpoints are out of rotation, but no more than
//! `max_ejected_percent` of the set (rounded down) at once: past that, the
//! ejected endpoints whose ejection ends soonest come back as *forced*
//! choices.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::time::Instant;

use super::members::Member;
use crate::error::ClientError;
use crate::origin::{Standing, Ticket};

/// A chosen endpoint and what the call to it is for its ejection state.
#[derive(Debug)]
pub struct Choice {
    pub member: Arc<Member>,
    pub ticket: Ticket,
}

/// How many of `n` endpoints may be out of rotation at once.
pub const fn max_out(n: usize, percent: u8) -> usize {
    n * percent as usize / 100
}

/// Chooses an endpoint of `set` other than `exclude`.
///
/// # Errors
///
/// [`ClientError::CircuitOpen`] when every endpoint other than `exclude` is
/// out of rotation and none may come back (`name` names the set); with
/// `exclude` set and nothing else to choose, the same.
pub fn choose(
    set: &[Arc<Member>],
    exclude: Option<&Arc<Member>>,
    max_ejected_percent: u8,
    name: &str,
    now: Instant,
) -> Result<Choice, ClientError> {
    let others = |m: &&Arc<Member>| exclude.is_none_or(|x| !Arc::ptr_eq(m, x));
    let mut ejected: Vec<&Arc<Member>> = set.iter().filter(|m| m.state.is_ejected(now)).collect();
    let allowed_out = max_out(set.len(), max_ejected_percent);
    let back = ejected.len().saturating_sub(allowed_out);
    // Soonest end first; an ended ejection whose probe is running counts as
    // ending now.
    ejected.sort_by_key(|m| m.state.ejected_until().unwrap_or(now).max(now));
    let forced: Vec<&Arc<Member>> = ejected.iter().take(back).copied().collect();

    let mut candidates: Vec<(&Arc<Member>, bool)> = set
        .iter()
        .filter(others)
        .filter_map(|m| {
            if forced.iter().any(|f| Arc::ptr_eq(f, m)) {
                Some((m, true))
            } else if ejected.iter().any(|e| Arc::ptr_eq(e, m)) {
                None
            } else {
                Some((m, false))
            }
        })
        .collect();

    while !candidates.is_empty() {
        let best = candidates
            .iter()
            .enumerate()
            .min_by_key(|(_, (m, _))| (m.state.in_flight(), m.last_pick.load(Ordering::Relaxed)))
            .map_or(0, |(i, _)| i);
        let (member, is_forced) = candidates.swap_remove(best);
        if is_forced {
            return Ok(Choice {
                member: Arc::clone(member),
                ticket: Ticket::Forced,
            });
        }
        // Another caller may have taken the probe slot, or ejected it, since
        // the look above: then try the next.
        if let Standing::Available(ticket) = member.state.standing(now) {
            return Ok(Choice {
                member: Arc::clone(member),
                ticket,
            });
        }
    }

    let retry_after = set
        .iter()
        .filter(others)
        .filter_map(|m| m.state.ejected_until())
        .min()
        .map(|until| until.saturating_duration_since(now));
    Err(ClientError::CircuitOpen {
        origin: name.to_owned(),
        retry_after,
    })
}
