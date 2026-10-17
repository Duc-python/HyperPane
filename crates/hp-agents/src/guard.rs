//! Limits on agent runs.
//!
//! A terminal that can open panes for agents can also open fifty of them by
//! accident, or five in a loop that nobody notices until the quota is gone.
//! These are the limits a person would set if they thought about it, so the app
//! thinks about it for them, and refuses rather than warns.
//!
//! Every limit is enforced where it cannot be skipped: the request goes through
//! the guard before a pane exists, not after.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// What the app allows.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Agent panes that may be alive at once.
    pub max_live_panes: usize,
    /// Runs that may be started inside one window.
    pub max_spawns_per_window: usize,
    pub window: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_live_panes: 4,
            max_spawns_per_window: 5,
            window: Duration::from_secs(10 * 60),
        }
    }
}

/// Why a request was refused. Each carries the number the message needs, so the
/// UI never has to guess at or invent one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Too many agent panes are already open.
    TooManyPanes { live: usize, limit: usize },
    /// Too many runs started recently.
    TooFast {
        limit: usize,
        window_seconds: u64,
        retry_in_seconds: u64,
    },
}

impl Refusal {
    /// A sentence to show a person, with the number that made it true.
    pub fn message(&self) -> String {
        match self {
            Refusal::TooManyPanes { live, limit } => format!(
                "{live} agent panes are already open and the limit is {limit}. \
                 Close one and try again."
            ),
            Refusal::TooFast {
                limit,
                window_seconds,
                retry_in_seconds,
            } => format!(
                "{limit} runs have started in the last {} minutes; the next \
                 one can start in {retry_in_seconds}s.",
                window_seconds / 60
            ),
        }
    }
}

/// Counts recent runs and answers whether another may start.
///
/// The clock is passed in rather than read, so the window can be tested in
/// milliseconds instead of waiting ten minutes for a test to pass.
#[derive(Debug)]
pub struct SpawnGuard {
    limits: Limits,
    recent: VecDeque<Instant>,
}

impl SpawnGuard {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            recent: VecDeque::new(),
        }
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Ask to start a run, given how many agent panes are alive.
    ///
    /// A refusal changes nothing. An approval is recorded, so the rate limit
    /// counts attempts that were allowed and not ones that were rejected.
    pub fn request(&mut self, live_panes: usize, now: Instant) -> Result<(), Refusal> {
        self.forget_old(now);

        if live_panes >= self.limits.max_live_panes {
            return Err(Refusal::TooManyPanes {
                live: live_panes,
                limit: self.limits.max_live_panes,
            });
        }

        if self.recent.len() >= self.limits.max_spawns_per_window {
            let oldest = self.recent.front().copied().unwrap_or(now);
            let frees_at = oldest + self.limits.window;
            let retry_in = frees_at.saturating_duration_since(now);
            return Err(Refusal::TooFast {
                limit: self.limits.max_spawns_per_window,
                window_seconds: self.limits.window.as_secs(),
                retry_in_seconds: retry_in.as_secs().max(1),
            });
        }

        self.recent.push_back(now);
        Ok(())
    }

    /// Drop runs that have fallen out of the window.
    fn forget_old(&mut self, now: Instant) {
        while let Some(front) = self.recent.front() {
            if now.saturating_duration_since(*front) >= self.limits.window {
                self.recent.pop_front();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> SpawnGuard {
        SpawnGuard::new(Limits {
            max_live_panes: 3,
            max_spawns_per_window: 2,
            window: Duration::from_secs(60),
        })
    }

    #[test]
    fn a_first_run_is_allowed() {
        let mut guard = guard();
        assert_eq!(guard.request(0, Instant::now()), Ok(()));
    }

    #[test]
    fn the_live_pane_limit_is_refused_with_its_numbers() {
        let mut guard = guard();
        let refusal = guard.request(3, Instant::now()).expect_err("refused");
        assert_eq!(refusal, Refusal::TooManyPanes { live: 3, limit: 3 });
        assert!(
            refusal.message().contains("limit is 3"),
            "{}",
            refusal.message()
        );
    }

    #[test]
    fn a_refusal_does_not_count_as_a_run() {
        let mut guard = guard();
        let now = Instant::now();

        // Two refusals, then two approvals: the refusals must not have used up
        // the window.
        assert!(guard.request(9, now).is_err());
        assert!(guard.request(9, now).is_err());
        assert!(guard.request(0, now).is_ok());
        assert!(guard.request(0, now).is_ok());
        assert!(guard.request(0, now).is_err(), "the window is full");
    }

    #[test]
    fn the_rate_limit_refuses_with_a_retry_time() {
        let mut guard = guard();
        let start = Instant::now();
        assert!(guard.request(0, start).is_ok());
        assert!(guard.request(0, start).is_ok());

        let refusal = guard
            .request(0, start + Duration::from_secs(20))
            .expect_err("refused");
        match refusal {
            Refusal::TooFast {
                limit,
                window_seconds,
                retry_in_seconds,
            } => {
                assert_eq!(limit, 2);
                assert_eq!(window_seconds, 60);
                // The window started at 0, so it frees at 60; we asked at 20.
                assert_eq!(retry_in_seconds, 40);
            }
            other => panic!("expected TooFast, got {other:?}"),
        }
    }

    #[test]
    fn a_slot_frees_once_the_window_has_passed() {
        let mut guard = guard();
        let start = Instant::now();
        assert!(guard.request(0, start).is_ok());
        assert!(guard.request(0, start).is_ok());
        assert!(guard.request(0, start).is_err());

        // Exactly one window later, the oldest run has aged out.
        assert_eq!(guard.request(0, start + Duration::from_secs(60)), Ok(()));
    }

    #[test]
    fn old_runs_are_forgotten_rather_than_kept_forever() {
        let mut guard = guard();
        let start = Instant::now();
        for step in 0..50 {
            let now = start + Duration::from_secs(step * 120);
            assert!(guard.request(0, now).is_ok(), "step {step}");
        }
        assert!(guard.recent.len() <= guard.limits().max_spawns_per_window);
    }

    #[test]
    fn the_default_limits_are_the_ones_the_message_quotes() {
        let limits = Limits::default();
        let mut guard = SpawnGuard::new(limits);
        let now = Instant::now();
        for _ in 0..limits.max_spawns_per_window {
            assert!(guard.request(0, now).is_ok());
        }
        let refusal = guard.request(0, now).expect_err("refused");
        assert!(
            refusal.message().contains("last 10 minutes"),
            "{}",
            refusal.message()
        );
    }
}
