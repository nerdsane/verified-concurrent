//! Deterministic fault injection.
//!
//! Simulates various failure modes in a reproducible way:
//! - Random failures (with configurable probability)
//! - Delays (network latency, disk I/O)
//! - Crashes (abrupt termination)
//! - Bit flips (memory corruption)

use crate::random::DeterministicRng;

/// Configuration for fault injection.
#[derive(Debug, Clone)]
pub struct FaultConfig {
    /// Probability of a random failure (0.0 to 1.0)
    pub failure_probability: f64,
    /// Probability of injecting a delay
    pub delay_probability: f64,
    /// Maximum delay in nanoseconds
    pub delay_ns_max: u64,
    /// Probability of a crash
    pub crash_probability: f64,
    /// Whether fault injection is enabled
    pub enabled: bool,
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self {
            failure_probability: 0.01,  // 1% chance
            delay_probability: 0.05,    // 5% chance
            delay_ns_max: 10_000_000,   // 10ms max delay
            crash_probability: 0.001,   // 0.1% chance
            enabled: true,
        }
    }
}

impl FaultConfig {
    /// No faults - useful for baseline testing.
    #[must_use]
    pub fn none() -> Self {
        Self {
            failure_probability: 0.0,
            delay_probability: 0.0,
            delay_ns_max: 0,
            crash_probability: 0.0,
            enabled: false,
        }
    }

    /// Aggressive faults for stress testing.
    #[must_use]
    pub fn aggressive() -> Self {
        Self {
            failure_probability: 0.1,   // 10% chance
            delay_probability: 0.2,     // 20% chance
            delay_ns_max: 100_000_000,  // 100ms max delay
            crash_probability: 0.01,    // 1% chance
            enabled: true,
        }
    }

    /// Only delays, no failures or crashes.
    #[must_use]
    pub fn delays_only() -> Self {
        Self {
            failure_probability: 0.0,
            delay_probability: 0.2,
            delay_ns_max: 50_000_000,
            crash_probability: 0.0,
            enabled: true,
        }
    }
}

/// Deterministic fault injector.
///
/// Uses a seeded RNG to inject faults in a reproducible way.
/// The same seed produces the same fault sequence.
pub struct FaultInjector {
    rng: DeterministicRng,
    config: FaultConfig,
    faults_injected_count: u64,
    delays_injected_count: u64,
    crashes_injected_count: u64,
}

/// Maximum number of faults before warning.
const FAULTS_COUNT_WARNING_MAX: u64 = 100_000;

impl FaultInjector {
    /// Create a new fault injector with the given RNG and config.
    pub fn new(rng: DeterministicRng, config: FaultConfig) -> Self {
        debug_assert!(
            config.failure_probability >= 0.0 && config.failure_probability <= 1.0,
            "Failure probability must be in [0.0, 1.0]"
        );
        debug_assert!(
            config.delay_probability >= 0.0 && config.delay_probability <= 1.0,
            "Delay probability must be in [0.0, 1.0]"
        );
        debug_assert!(
            config.crash_probability >= 0.0 && config.crash_probability <= 1.0,
            "Crash probability must be in [0.0, 1.0]"
        );

        Self {
            rng,
            config,
            faults_injected_count: 0,
            delays_injected_count: 0,
            crashes_injected_count: 0,
        }
    }

    /// Create with default config.
    pub fn with_default_config(rng: DeterministicRng) -> Self {
        Self::new(rng, FaultConfig::default())
    }

    /// Check if a failure should occur.
    ///
    /// Returns true with probability `config.failure_probability`.
    pub fn should_fail(&mut self) -> bool {
        if !self.config.enabled {
            return false;
        }

        let result = self.rng.gen_bool(self.config.failure_probability);
        if result {
            self.faults_injected_count += 1;
            debug_assert!(
                self.faults_injected_count < FAULTS_COUNT_WARNING_MAX,
                "Very high number of faults - possible issue with test"
            );
        }
        result
    }

    /// Get delay to inject (if any).
    ///
    /// Returns Some(nanoseconds) with probability `config.delay_probability`.
    pub fn maybe_delay_ns(&mut self) -> Option<u64> {
        if !self.config.enabled || self.config.delay_ns_max == 0 {
            return None;
        }

        if self.rng.gen_bool(self.config.delay_probability) {
            self.delays_injected_count += 1;
            Some(self.rng.gen_range(1..=self.config.delay_ns_max))
        } else {
            None
        }
    }

    /// Check if a crash should occur.
    ///
    /// Returns true with probability `config.crash_probability`.
    /// Unlike `should_fail`, this simulates a complete crash.
    pub fn should_crash(&mut self) -> bool {
        if !self.config.enabled {
            return false;
        }

        let result = self.rng.gen_bool(self.config.crash_probability);
        if result {
            self.crashes_injected_count += 1;
        }
        result
    }

    /// Inject a bit flip at a random position in the slice.
    ///
    /// Used to simulate memory corruption. Only flips one bit.
    pub fn maybe_corrupt(&mut self, data: &mut [u8]) -> bool {
        if !self.config.enabled || data.is_empty() {
            return false;
        }

        // Low probability of corruption
        if self.rng.gen_bool(0.0001) {
            let byte_idx = self.rng.gen_range(0..data.len());
            let bit_idx = self.rng.gen_range(0..8);
            data[byte_idx] ^= 1 << bit_idx;
            true
        } else {
            false
        }
    }

    /// Get statistics about injected faults.
    #[must_use]
    pub fn stats(&self) -> FaultStats {
        FaultStats {
            faults_count: self.faults_injected_count,
            delays_count: self.delays_injected_count,
            crashes_count: self.crashes_injected_count,
        }
    }

    /// Get current config.
    #[must_use]
    pub fn config(&self) -> &FaultConfig {
        &self.config
    }

    /// Update config.
    pub fn set_config(&mut self, config: FaultConfig) {
        self.config = config;
    }

    /// Enable or disable fault injection.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.enabled = enabled;
    }
}

/// Statistics about injected faults.
#[derive(Debug, Clone, Copy)]
pub struct FaultStats {
    /// Number of failures injected
    pub faults_count: u64,
    /// Number of delays injected
    pub delays_count: u64,
    /// Number of crashes injected
    pub crashes_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_faults_when_disabled() {
        let rng = DeterministicRng::new(12345);
        let mut injector = FaultInjector::new(rng, FaultConfig::none());

        for _ in 0..1000 {
            assert!(!injector.should_fail());
            assert!(injector.maybe_delay_ns().is_none());
            assert!(!injector.should_crash());
        }
    }

    #[test]
    fn test_deterministic_faults() {
        let rng1 = DeterministicRng::new(42);
        let rng2 = DeterministicRng::new(42);

        let mut inj1 = FaultInjector::new(rng1, FaultConfig::default());
        let mut inj2 = FaultInjector::new(rng2, FaultConfig::default());

        for _ in 0..100 {
            assert_eq!(inj1.should_fail(), inj2.should_fail());
        }
    }

    #[test]
    fn test_fault_probability() {
        let rng = DeterministicRng::new(12345);
        let config = FaultConfig {
            failure_probability: 0.5,
            ..FaultConfig::default()
        };
        let mut injector = FaultInjector::new(rng, config);

        let mut failures = 0;
        let trials = 10000;
        for _ in 0..trials {
            if injector.should_fail() {
                failures += 1;
            }
        }

        // With 50% probability, expect ~5000 failures
        // Allow for statistical variation
        let ratio = failures as f64 / trials as f64;
        assert!(
            (0.45..=0.55).contains(&ratio),
            "Expected ~50% failures, got {}%",
            ratio * 100.0
        );
    }

    #[test]
    fn test_delay_injection() {
        let rng = DeterministicRng::new(12345);
        let config = FaultConfig {
            delay_probability: 1.0, // Always delay
            delay_ns_max: 1_000_000,
            ..FaultConfig::default()
        };
        let mut injector = FaultInjector::new(rng, config);

        for _ in 0..100 {
            let delay = injector.maybe_delay_ns();
            assert!(delay.is_some());
            let d = delay.unwrap();
            assert!(d >= 1 && d <= 1_000_000);
        }
    }

    #[test]
    fn test_stats() {
        let rng = DeterministicRng::new(12345);
        let config = FaultConfig {
            failure_probability: 1.0, // Always fail
            ..FaultConfig::default()
        };
        let mut injector = FaultInjector::new(rng, config);

        for _ in 0..10 {
            injector.should_fail();
        }

        let stats = injector.stats();
        assert_eq!(stats.faults_count, 10);
    }

    #[test]
    fn test_corruption() {
        let rng = DeterministicRng::new(12345);
        let mut injector = FaultInjector::with_default_config(rng);

        // Run many iterations - corruption is very rare
        let mut corrupted_count = 0;
        for _ in 0..100_000 {
            let mut data = [0xAA; 8];
            if injector.maybe_corrupt(&mut data) {
                corrupted_count += 1;
                // Verify exactly one bit flipped
                let diff: u32 = data.iter().map(|&b| (b ^ 0xAA).count_ones()).sum();
                assert_eq!(diff, 1, "Expected exactly one bit flip");
            }
        }
        // With 0.01% probability, expect ~10 corruptions in 100k trials
        assert!(
            corrupted_count > 0,
            "Expected some corruptions in 100k trials"
        );
    }
}
