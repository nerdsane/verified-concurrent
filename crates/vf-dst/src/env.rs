//! DST environment combining clock, RNG, fault injector, and scheduler.
//!
//! The `DstEnv` is the central context for deterministic simulation tests.
//! It provides all the building blocks needed for reproducible testing.

use crate::clock::SimClock;
use crate::fault::{FaultConfig, FaultInjector};
use crate::random::DeterministicRng;
use crate::scheduler::Scheduler;

/// Complete DST environment.
///
/// Combines all DST components with a single seed for full reproducibility.
/// Given the same seed, all behavior is deterministic.
///
/// # Usage
///
/// ```rust
/// use vf_dst::{DstEnv, get_or_generate_seed};
///
/// let seed = get_or_generate_seed();
/// let mut env = DstEnv::new(seed);
///
/// // Access components
/// let now = env.clock().now_ms();
/// let random_val: u64 = env.rng().gen();
/// let should_fail = env.fault().should_fail();
/// ```
pub struct DstEnv {
    seed: u64,
    clock: SimClock,
    rng: DeterministicRng,
    fault: FaultInjector,
    scheduler: Option<Scheduler>,
}

impl DstEnv {
    /// Create a new DST environment with the given seed.
    ///
    /// All components are initialized deterministically from this seed.
    pub fn new(seed: u64) -> Self {
        debug_assert!(seed != 0, "Seed should not be zero");

        let mut master_rng = DeterministicRng::new(seed);

        // Derive seeds for each component
        let rng_seed = master_rng.gen::<u64>();
        let fault_seed = master_rng.gen::<u64>();

        let rng = DeterministicRng::new(rng_seed);
        let fault_rng = DeterministicRng::new(fault_seed);
        let fault = FaultInjector::with_default_config(fault_rng);

        Self {
            seed,
            clock: SimClock::new(),
            rng,
            fault,
            scheduler: None,
        }
    }

    /// Create with custom fault configuration.
    pub fn with_fault_config(seed: u64, fault_config: FaultConfig) -> Self {
        debug_assert!(seed != 0, "Seed should not be zero");

        let mut master_rng = DeterministicRng::new(seed);
        let rng_seed = master_rng.gen::<u64>();
        let fault_seed = master_rng.gen::<u64>();

        let rng = DeterministicRng::new(rng_seed);
        let fault_rng = DeterministicRng::new(fault_seed);
        let fault = FaultInjector::new(fault_rng, fault_config);

        Self {
            seed,
            clock: SimClock::new(),
            rng,
            fault,
            scheduler: None,
        }
    }

    /// Create with scheduler for multi-threaded tests.
    pub fn with_scheduler(seed: u64, threads_count: usize) -> Self {
        debug_assert!(seed != 0, "Seed should not be zero");
        debug_assert!(threads_count > 0, "Must have at least one thread");

        let mut master_rng = DeterministicRng::new(seed);
        let rng_seed = master_rng.gen::<u64>();
        let fault_seed = master_rng.gen::<u64>();
        let sched_seed = master_rng.gen::<u64>();

        let rng = DeterministicRng::new(rng_seed);
        let fault_rng = DeterministicRng::new(fault_seed);
        let fault = FaultInjector::with_default_config(fault_rng);
        let sched_rng = DeterministicRng::new(sched_seed);
        let scheduler = Scheduler::with_defaults(sched_rng, threads_count);

        Self {
            seed,
            clock: SimClock::new(),
            rng,
            fault,
            scheduler: Some(scheduler),
        }
    }

    /// Get the seed used to create this environment.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Access the simulated clock.
    #[must_use]
    pub fn clock(&self) -> &SimClock {
        &self.clock
    }

    /// Access the deterministic RNG.
    pub fn rng(&mut self) -> &mut DeterministicRng {
        &mut self.rng
    }

    /// Access the fault injector.
    pub fn fault(&mut self) -> &mut FaultInjector {
        &mut self.fault
    }

    /// Access the scheduler (if configured).
    pub fn scheduler(&mut self) -> Option<&mut Scheduler> {
        self.scheduler.as_mut()
    }

    /// Fork a new RNG for a sub-component.
    ///
    /// Useful for giving each component its own deterministic RNG.
    pub fn fork_rng(&mut self) -> DeterministicRng {
        self.rng.fork()
    }

    /// Run an operation with simulated delay.
    ///
    /// If the fault injector decides to inject a delay, advances the clock.
    pub fn maybe_delay(&mut self) {
        if let Some(delay_ns) = self.fault.maybe_delay_ns() {
            self.clock.advance_ns(delay_ns);
        }
    }

    /// Check for failure and advance time.
    ///
    /// Convenience method that combines fault check with clock advance.
    pub fn step(&mut self, duration_ns: u64) -> bool {
        self.clock.advance_ns(duration_ns);
        self.fault.should_fail()
    }

    /// Format seed for error messages.
    ///
    /// Use this in test failures so the seed can be easily copied.
    #[must_use]
    pub fn format_seed(&self) -> String {
        format!("DST_SEED={}", self.seed)
    }

    /// Get summary statistics.
    #[must_use]
    pub fn stats(&self) -> DstStats {
        let fault_stats = self.fault.stats();
        DstStats {
            seed: self.seed,
            elapsed_ns: self.clock.now_ns(),
            rng_calls: self.rng.calls_count(),
            faults_injected: fault_stats.faults_count,
            delays_injected: fault_stats.delays_count,
            scheduler_decisions: self.scheduler.as_ref().map_or(0, |s| s.decisions_count()),
        }
    }
}

/// Statistics about DST execution.
#[derive(Debug, Clone, Copy)]
pub struct DstStats {
    /// Seed used for reproducibility
    pub seed: u64,
    /// Simulated time elapsed (nanoseconds)
    pub elapsed_ns: u64,
    /// Number of random values generated
    pub rng_calls: u64,
    /// Number of faults injected
    pub faults_injected: u64,
    /// Number of delays injected
    pub delays_injected: u64,
    /// Number of scheduler decisions (if scheduler configured)
    pub scheduler_decisions: u64,
}

impl std::fmt::Display for DstStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DST_SEED={} elapsed={}ms rng_calls={} faults={} delays={} sched_decisions={}",
            self.seed,
            self.elapsed_ns / 1_000_000,
            self.rng_calls,
            self.faults_injected,
            self.delays_injected,
            self.scheduler_decisions
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determinism() {
        let mut env1 = DstEnv::new(42);
        let mut env2 = DstEnv::new(42);

        // Same seed should produce same random values
        for _ in 0..100 {
            assert_eq!(env1.rng().gen::<u64>(), env2.rng().gen::<u64>());
        }
    }

    #[test]
    fn test_different_seeds() {
        let mut env1 = DstEnv::new(42);
        let mut env2 = DstEnv::new(43);

        let seq1: Vec<u64> = (0..10).map(|_| env1.rng().gen()).collect();
        let seq2: Vec<u64> = (0..10).map(|_| env2.rng().gen()).collect();
        assert_ne!(seq1, seq2);
    }

    #[test]
    fn test_with_scheduler() {
        let mut env = DstEnv::with_scheduler(12345, 4);

        assert!(env.scheduler().is_some());
        let sched = env.scheduler().unwrap();
        assert_eq!(sched.threads_count(), 4);
    }

    #[test]
    fn test_fault_config() {
        let mut env = DstEnv::with_fault_config(12345, FaultConfig::none());

        // No faults should occur
        for _ in 0..100 {
            assert!(!env.fault().should_fail());
        }
    }

    #[test]
    fn test_fork_rng() {
        let mut env1 = DstEnv::new(42);
        let mut env2 = DstEnv::new(42);

        let mut forked1 = env1.fork_rng();
        let mut forked2 = env2.fork_rng();

        // Forked RNGs should be identical
        for _ in 0..10 {
            assert_eq!(forked1.gen::<u64>(), forked2.gen::<u64>());
        }
    }

    #[test]
    fn test_step() {
        let mut env = DstEnv::new(12345);

        env.step(1_000_000); // 1ms
        assert_eq!(env.clock().now_ms(), 1);

        env.step(9_000_000); // 9ms more
        assert_eq!(env.clock().now_ms(), 10);
    }

    #[test]
    fn test_stats() {
        let mut env = DstEnv::new(12345);

        let _: u64 = env.rng().gen();
        let _: u64 = env.rng().gen();
        env.clock().advance_ms(100);

        let stats = env.stats();
        assert_eq!(stats.seed, 12345);
        assert_eq!(stats.elapsed_ns, 100_000_000);
        assert_eq!(stats.rng_calls, 2);
    }

    #[test]
    fn test_format_seed() {
        let env = DstEnv::new(12345);
        assert_eq!(env.format_seed(), "DST_SEED=12345");
    }
}
