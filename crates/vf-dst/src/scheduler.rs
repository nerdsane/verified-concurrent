//! Deterministic thread scheduler for DST.
//!
//! Controls thread interleaving in a reproducible way.
//! This is the core mechanism for finding concurrency bugs.

use crate::random::DeterministicRng;

/// Thread scheduling decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleDecision {
    /// Continue executing the current thread
    Continue,
    /// Yield to another thread
    Yield,
    /// Context switch to a specific thread
    SwitchTo(usize),
}

/// Deterministic thread scheduler.
///
/// Makes reproducible scheduling decisions based on a seeded RNG.
/// Given the same seed and same thread states, always produces
/// the same interleaving.
pub struct Scheduler {
    rng: DeterministicRng,
    /// Number of threads being scheduled
    threads_count: usize,
    /// Current thread index
    current_thread: usize,
    /// Probability of yielding on each decision point
    yield_probability: f64,
    /// Schedule decisions made
    decisions_count: u64,
}

/// Maximum threads to schedule.
const THREADS_COUNT_MAX: usize = 64;

/// Maximum decisions before warning.
const DECISIONS_COUNT_WARNING_MAX: u64 = 10_000_000;

impl Scheduler {
    /// Create a new scheduler.
    ///
    /// # Arguments
    /// - `rng`: Deterministic RNG for scheduling decisions
    /// - `threads_count`: Number of threads to schedule
    /// - `yield_probability`: Probability of yielding on each decision
    pub fn new(rng: DeterministicRng, threads_count: usize, yield_probability: f64) -> Self {
        debug_assert!(threads_count > 0, "Must have at least one thread");
        debug_assert!(
            threads_count <= THREADS_COUNT_MAX,
            "Too many threads: {} > {}",
            threads_count,
            THREADS_COUNT_MAX
        );
        debug_assert!(
            (0.0..=1.0).contains(&yield_probability),
            "Yield probability must be in [0.0, 1.0]"
        );

        Self {
            rng,
            threads_count,
            current_thread: 0,
            yield_probability,
            decisions_count: 0,
        }
    }

    /// Create with default yield probability (10%).
    pub fn with_defaults(rng: DeterministicRng, threads_count: usize) -> Self {
        Self::new(rng, threads_count, 0.1)
    }

    /// Get the current thread index.
    #[must_use]
    pub fn current_thread(&self) -> usize {
        self.current_thread
    }

    /// Get the total number of threads.
    #[must_use]
    pub fn threads_count(&self) -> usize {
        self.threads_count
    }

    /// Make a scheduling decision.
    ///
    /// Called at each yield point in the test. Returns what the
    /// current thread should do.
    pub fn decide(&mut self) -> ScheduleDecision {
        self.decisions_count += 1;
        debug_assert!(
            self.decisions_count < DECISIONS_COUNT_WARNING_MAX,
            "Very high number of scheduling decisions - possible infinite loop"
        );

        if self.threads_count == 1 {
            return ScheduleDecision::Continue;
        }

        if self.rng.gen_bool(self.yield_probability) {
            // Pick a different thread
            let other = self.pick_other_thread();
            self.current_thread = other;
            ScheduleDecision::SwitchTo(other)
        } else {
            ScheduleDecision::Continue
        }
    }

    /// Force a context switch to a random thread.
    pub fn force_switch(&mut self) -> usize {
        self.decisions_count += 1;

        if self.threads_count == 1 {
            return 0;
        }

        let other = self.pick_other_thread();
        self.current_thread = other;
        other
    }

    /// Pick a random thread other than the current one.
    fn pick_other_thread(&mut self) -> usize {
        debug_assert!(self.threads_count > 1);

        loop {
            let candidate = self.rng.gen_range(0..self.threads_count);
            if candidate != self.current_thread {
                return candidate;
            }
        }
    }

    /// Set the current thread explicitly.
    ///
    /// Used for deterministic replay.
    pub fn set_current_thread(&mut self, thread: usize) {
        debug_assert!(thread < self.threads_count);
        self.current_thread = thread;
    }

    /// Get number of decisions made.
    #[must_use]
    pub fn decisions_count(&self) -> u64 {
        self.decisions_count
    }

    /// Add a thread (e.g., when spawning).
    ///
    /// Returns the new thread's index.
    pub fn add_thread(&mut self) -> usize {
        debug_assert!(
            self.threads_count < THREADS_COUNT_MAX,
            "Cannot add more threads"
        );
        let idx = self.threads_count;
        self.threads_count += 1;
        idx
    }

    /// Remove a thread (e.g., when joining).
    ///
    /// Adjusts current thread if necessary.
    pub fn remove_thread(&mut self, thread: usize) {
        debug_assert!(thread < self.threads_count);
        debug_assert!(self.threads_count > 1, "Cannot remove last thread");

        self.threads_count -= 1;

        if self.current_thread == thread {
            // Switch to thread 0 or the previous one
            self.current_thread = if thread > 0 { thread - 1 } else { 0 };
        } else if self.current_thread > thread {
            // Adjust index for removed thread
            self.current_thread -= 1;
        }
    }
}

/// A deterministic yield point.
///
/// Call this at points where thread interleaving should be possible.
/// In DST, this makes a scheduling decision. In production, this is a no-op.
#[inline]
pub fn yield_point(scheduler: &mut Scheduler) -> ScheduleDecision {
    scheduler.decide()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_thread() {
        let rng = DeterministicRng::new(12345);
        let mut sched = Scheduler::with_defaults(rng, 1);

        // Single thread always continues
        for _ in 0..100 {
            assert_eq!(sched.decide(), ScheduleDecision::Continue);
        }
    }

    #[test]
    fn test_deterministic_scheduling() {
        let rng1 = DeterministicRng::new(42);
        let rng2 = DeterministicRng::new(42);

        let mut sched1 = Scheduler::new(rng1, 4, 0.5);
        let mut sched2 = Scheduler::new(rng2, 4, 0.5);

        for _ in 0..100 {
            assert_eq!(sched1.decide(), sched2.decide());
        }
    }

    #[test]
    fn test_yield_probability() {
        let rng = DeterministicRng::new(12345);
        let mut sched = Scheduler::new(rng, 4, 0.5);

        let mut yields = 0;
        let trials = 1000;
        for _ in 0..trials {
            if sched.decide() != ScheduleDecision::Continue {
                yields += 1;
            }
        }

        // With 50% yield probability, expect ~500 yields
        let ratio = yields as f64 / trials as f64;
        assert!(
            (0.4..=0.6).contains(&ratio),
            "Expected ~50% yields, got {}%",
            ratio * 100.0
        );
    }

    #[test]
    fn test_force_switch() {
        let rng = DeterministicRng::new(12345);
        let mut sched = Scheduler::with_defaults(rng, 4);

        assert_eq!(sched.current_thread(), 0);

        let new = sched.force_switch();
        assert_ne!(new, 0);
        assert_eq!(sched.current_thread(), new);
    }

    #[test]
    fn test_add_remove_thread() {
        let rng = DeterministicRng::new(12345);
        let mut sched = Scheduler::with_defaults(rng, 2);

        assert_eq!(sched.threads_count(), 2);

        let idx = sched.add_thread();
        assert_eq!(idx, 2);
        assert_eq!(sched.threads_count(), 3);

        sched.remove_thread(1);
        assert_eq!(sched.threads_count(), 2);
    }

    #[test]
    fn test_current_thread_adjustment_on_remove() {
        let rng = DeterministicRng::new(12345);
        let mut sched = Scheduler::with_defaults(rng, 4);

        sched.set_current_thread(3);
        assert_eq!(sched.current_thread(), 3);

        // Remove thread 1 (before current)
        sched.remove_thread(1);
        // Current should adjust from 3 to 2
        assert_eq!(sched.current_thread(), 2);
    }
}
