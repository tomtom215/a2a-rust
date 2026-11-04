// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How a delegation gets a lost stream back (audit H4, ADR 0030): the policy
//! that bounds it. The loop that follows it is in [`resume`](super::resume).

use std::time::Duration;

/// How a [`Delegation`](super::Delegation) tries to get its stream back before it gives up on
/// the child. The default is five attempts, 200 ms apart at first and
/// doubling to 5 s, inside 30 s in all.
///
/// An attempt that gets a stream and then loses it without the position
/// moving counts as spent: a server that accepts a subscription and drops it
/// at once exhausts the policy rather than being asked for ever. A stream
/// that delivered events (the position advanced) starts the count again.
///
/// Positions come from the agent's event ids. An agent that sends none (gRPC
/// and WebSocket streams carry none) shows no progress that way, so a stream
/// also counts as having worked when it delivered at least one event and
/// stayed open for [`healthy_after`](Self::healthy_after): a long, quiet
/// child cut again and again by an idle timeout is not given up on, while a
/// server that accepts a subscription, sends its snapshot and drops it at
/// once still exhausts the policy.
///
/// The waits between attempts are spread by up to half
/// ([`with_jitter`](Self::with_jitter)), so that a fleet whose streams a
/// proxy rollout cut in the same instant does not come back in the same
/// instant.
///
/// [`ResumePolicy::none`] restores the behaviour before 2026-10-31: the first
/// loss cancels the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResumePolicy {
    /// How many times in a row to reopen the stream. Zero never resumes.
    pub attempts: u32,
    /// The wait before the second attempt; each later wait doubles.
    pub backoff: Duration,
    /// The longest a wait between attempts grows to.
    pub max_backoff: Duration,
    /// The longest one loss is worked on, from the loss to giving up.
    pub deadline: Duration,
    /// How long a reopened stream must stay open, having delivered an event,
    /// to count as one that worked and start the attempt count again.
    pub healthy_after: Duration,
    /// Whether each wait between attempts is shortened by a random fraction
    /// of up to half.
    pub jitter: bool,
}

impl Default for ResumePolicy {
    fn default() -> Self {
        Self {
            attempts: 5,
            backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(5),
            deadline: Duration::from_secs(30),
            healthy_after: Duration::from_secs(5),
            jitter: true,
        }
    }
}

impl ResumePolicy {
    /// Never resumes: the first lost stream cancels the child.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            attempts: 0,
            backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
            deadline: Duration::ZERO,
            healthy_after: Duration::ZERO,
            jitter: false,
        }
    }

    /// Sets how many times in a row the stream is reopened.
    #[must_use]
    pub const fn with_attempts(mut self, attempts: u32) -> Self {
        self.attempts = attempts;
        self
    }

    /// Sets the first wait between attempts and the longest one.
    #[must_use]
    pub const fn with_backoff(mut self, first: Duration, longest: Duration) -> Self {
        self.backoff = first;
        self.max_backoff = longest;
        self
    }

    /// Sets how long one loss is worked on.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// The wait before attempt `streak + 1`: [`wait_before`](Self::wait_before),
    /// shortened by `spread` when the policy asks for jitter. The shortening
    /// is passed in so that a test can pin it; the delegation passes the
    /// client's own jitter.
    pub(crate) fn attempt_wait(
        &self,
        streak: u32,
        spread: impl FnOnce(Duration) -> Duration,
    ) -> Duration {
        let wait = self.wait_before(streak);
        if self.jitter { spread(wait) } else { wait }
    }

    /// Sets how long a reopened stream must stay open, having delivered an
    /// event, to count as one that worked.
    #[must_use]
    pub const fn with_healthy_after(mut self, healthy_after: Duration) -> Self {
        self.healthy_after = healthy_after;
        self
    }

    /// Sets whether each wait between attempts is shortened by a random
    /// fraction of up to half. On by default; a test that pins the waits
    /// turns it off.
    #[must_use]
    pub const fn with_jitter(mut self, jitter: bool) -> Self {
        self.jitter = jitter;
        self
    }

    /// How long to wait before reopening attempt number `streak + 1`: nothing
    /// before the first, then [`backoff`](Self::backoff), doubling each time,
    /// never more than [`max_backoff`](Self::max_backoff). Pure, so that its
    /// arithmetic is pinned exactly by the tests and not by how long a loop
    /// happened to take.
    pub(crate) fn wait_before(&self, streak: u32) -> Duration {
        let Some(doublings) = streak.checked_sub(1) else {
            return Duration::ZERO;
        };
        // The clamp keeps the shift in range for a long streak; the cap below
        // is what bounds the wait.
        self.backoff
            .saturating_mul(1_u32 << doublings.min(16))
            .min(self.max_backoff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn there_is_no_wait_before_the_first_attempt_and_it_doubles_after() {
        let p = ResumePolicy::default(); // 200 ms, doubling, capped at 5 s
        let waits: Vec<_> = (0..=7).map(|streak| p.wait_before(streak)).collect();
        assert_eq!(
            waits,
            [
                Duration::ZERO,
                ms(200),
                ms(400),
                ms(800),
                ms(1_600),
                ms(3_200),
                ms(5_000), // 6,400 capped
                ms(5_000),
            ]
        );
    }

    #[test]
    fn the_cap_holds_for_any_streak_and_the_shift_cannot_overflow() {
        let p = ResumePolicy::default().with_backoff(ms(7), Duration::from_secs(3_600));
        assert_eq!(p.wait_before(1), ms(7));
        assert_eq!(p.wait_before(2), ms(14));
        assert_eq!(p.wait_before(3), ms(28));
        // Far past the clamp: the cap, not a panic and not a wrapped shift.
        assert_eq!(p.wait_before(17), ms(7 * 65_536));
        assert_eq!(p.wait_before(18), ms(7 * 65_536));
        assert_eq!(p.wait_before(u32::MAX), ms(7 * 65_536));
        let capped = p.with_backoff(ms(300), ms(100));
        assert_eq!(
            capped.wait_before(1),
            ms(100),
            "the cap wins over the first wait"
        );
    }

    fn halved(wait: Duration) -> Duration {
        wait / 2
    }

    #[test]
    fn jitter_shortens_the_wait_only_when_the_policy_asks_for_it() {
        let on = ResumePolicy::default();
        let off = ResumePolicy::default().with_jitter(false);
        assert!(on.jitter && !off.jitter);
        assert_eq!(on.attempt_wait(2, halved), ms(200), "400 ms, halved");
        assert_eq!(off.attempt_wait(2, halved), ms(400), "left as it was");
        assert_eq!(
            on.attempt_wait(0, halved),
            Duration::ZERO,
            "no wait stays none"
        );
        assert!(!ResumePolicy::none().jitter, "none() waits for nothing");
    }

    #[test]
    fn the_defaults_and_setters_of_the_new_fields() {
        assert_eq!(
            ResumePolicy::default().healthy_after,
            Duration::from_secs(5)
        );
        assert_eq!(ResumePolicy::none().healthy_after, Duration::ZERO);
        assert_eq!(
            ResumePolicy::default()
                .with_healthy_after(ms(7))
                .healthy_after,
            ms(7)
        );
    }

    #[test]
    fn a_zero_backoff_never_waits() {
        let p = ResumePolicy::none();
        assert!((0..40).all(|s| p.wait_before(s).is_zero()));
        let p = ResumePolicy::default().with_backoff(Duration::ZERO, Duration::from_secs(5));
        assert!((0..40).all(|s| p.wait_before(s).is_zero()));
    }
}
