//! Deterministic random number generation.
//!
//! Uses a seeded PRNG (Xoshiro256**) that produces identical sequences
//! for identical seeds, enabling reproducible test runs.

use rand::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256StarStar;

/// Deterministic random number generator.
///
/// Wraps Xoshiro256** with a seed for reproducibility.
/// Given the same seed, always produces the same sequence.
///
/// # Example
///
/// ```rust
/// use vf_dst::DeterministicRng;
///
/// let mut rng = DeterministicRng::new(12345);
/// let a: u64 = rng.gen();
/// let b: u64 = rng.gen();
///
/// // Same seed produces same sequence
/// let mut rng2 = DeterministicRng::new(12345);
/// assert_eq!(rng2.gen::<u64>(), a);
/// assert_eq!(rng2.gen::<u64>(), b);
/// ```
pub struct DeterministicRng {
    seed: u64,
    rng: Xoshiro256StarStar,
    calls_count: u64,
}

/// Maximum number of RNG calls before warning.
const RNG_CALLS_WARNING_THRESHOLD: u64 = 1_000_000_000;

impl DeterministicRng {
    /// Create a new RNG with the given seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        debug_assert!(seed != 0, "Seed should not be zero for better randomness");

        Self {
            seed,
            rng: Xoshiro256StarStar::seed_from_u64(seed),
            calls_count: 0,
        }
    }

    /// Get the seed used to create this RNG.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Get number of random values generated.
    #[must_use]
    pub fn calls_count(&self) -> u64 {
        self.calls_count
    }

    /// Generate a random value of type T.
    pub fn gen<T>(&mut self) -> T
    where
        rand::distributions::Standard: rand::distributions::Distribution<T>,
    {
        self.calls_count += 1;
        debug_assert!(
            self.calls_count < RNG_CALLS_WARNING_THRESHOLD,
            "Very high number of RNG calls - possible infinite loop"
        );
        self.rng.gen()
    }

    /// Generate a random value in the given range.
    pub fn gen_range<T, R>(&mut self, range: R) -> T
    where
        T: rand::distributions::uniform::SampleUniform,
        R: rand::distributions::uniform::SampleRange<T>,
    {
        self.calls_count += 1;
        debug_assert!(
            self.calls_count < RNG_CALLS_WARNING_THRESHOLD,
            "Very high number of RNG calls - possible infinite loop"
        );
        self.rng.gen_range(range)
    }

    /// Generate a boolean with the given probability of true.
    pub fn gen_bool(&mut self, probability: f64) -> bool {
        debug_assert!(
            (0.0..=1.0).contains(&probability),
            "Probability must be in [0.0, 1.0]"
        );
        self.calls_count += 1;
        self.rng.gen_bool(probability)
    }

    /// Shuffle a slice in place.
    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        use rand::seq::SliceRandom;
        self.calls_count += 1;
        slice.shuffle(&mut self.rng);
    }

    /// Choose a random element from a slice.
    pub fn choose<'a, T>(&mut self, slice: &'a [T]) -> Option<&'a T> {
        use rand::seq::SliceRandom;
        self.calls_count += 1;
        slice.choose(&mut self.rng)
    }

    /// Fork this RNG into a new one with a derived seed.
    ///
    /// Useful for giving each thread/component its own deterministic RNG.
    #[must_use]
    pub fn fork(&mut self) -> Self {
        let new_seed = self.gen::<u64>();
        Self::new(new_seed)
    }

    /// Reset to initial state (same seed).
    pub fn reset(&mut self) {
        self.rng = Xoshiro256StarStar::seed_from_u64(self.seed);
        self.calls_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determinism() {
        let mut rng1 = DeterministicRng::new(42);
        let mut rng2 = DeterministicRng::new(42);

        for _ in 0..100 {
            assert_eq!(rng1.gen::<u64>(), rng2.gen::<u64>());
        }
    }

    #[test]
    fn test_different_seeds() {
        let mut rng1 = DeterministicRng::new(42);
        let mut rng2 = DeterministicRng::new(43);

        // Different seeds should produce different sequences
        let seq1: Vec<u64> = (0..10).map(|_| rng1.gen()).collect();
        let seq2: Vec<u64> = (0..10).map(|_| rng2.gen()).collect();
        assert_ne!(seq1, seq2);
    }

    #[test]
    fn test_gen_range() {
        let mut rng = DeterministicRng::new(12345);

        for _ in 0..100 {
            let val = rng.gen_range(0..10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_gen_bool() {
        let mut rng = DeterministicRng::new(12345);

        // With probability 0, should always be false
        for _ in 0..10 {
            assert!(!rng.gen_bool(0.0));
        }

        // With probability 1, should always be true
        for _ in 0..10 {
            assert!(rng.gen_bool(1.0));
        }
    }

    #[test]
    fn test_shuffle() {
        let mut rng = DeterministicRng::new(12345);
        let mut data = vec![1, 2, 3, 4, 5];
        let original = data.clone();

        rng.shuffle(&mut data);
        // Shuffle should change order (with very high probability)
        // Reset and verify same shuffle happens
        rng.reset();
        let mut data2 = original.clone();
        rng.shuffle(&mut data2);
        assert_eq!(data, data2);
    }

    #[test]
    fn test_fork() {
        let mut rng = DeterministicRng::new(12345);
        let mut forked = rng.fork();

        // Forked RNG has different seed
        assert_ne!(forked.seed(), 12345);

        // But the process is deterministic
        let mut rng2 = DeterministicRng::new(12345);
        let mut forked2 = rng2.fork();
        assert_eq!(forked.seed(), forked2.seed());
    }

    #[test]
    fn test_reset() {
        let mut rng = DeterministicRng::new(12345);
        let first_value: u64 = rng.gen();

        // Generate more values
        for _ in 0..100 {
            let _: u64 = rng.gen();
        }

        // Reset and verify sequence restarts
        rng.reset();
        assert_eq!(rng.gen::<u64>(), first_value);
    }

    #[test]
    fn test_calls_count() {
        let mut rng = DeterministicRng::new(12345);
        assert_eq!(rng.calls_count(), 0);

        let _: u64 = rng.gen();
        assert_eq!(rng.calls_count(), 1);

        let _ = rng.gen_range(0..10);
        assert_eq!(rng.calls_count(), 2);
    }
}
