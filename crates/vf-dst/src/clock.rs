//! Deterministic simulated time.
//!
//! Time in DST tests is fully controlled - it only advances when
//! explicitly requested. This allows testing time-dependent behavior
//! deterministically.

use std::sync::atomic::{AtomicU64, Ordering};

/// Simulated clock with nanosecond precision.
///
/// Time only advances when explicitly requested via `advance_ns`.
/// All time queries are deterministic and reproducible.
///
/// # Thread Safety
///
/// The clock uses atomic operations and is safe to share across threads.
/// However, for deterministic testing, only one thread should advance time.
pub struct SimClock {
    /// Current time in nanoseconds since epoch
    now_ns: AtomicU64,
}

/// Bounds for time operations.
const TIME_NS_MAX: u64 = u64::MAX - 1_000_000_000_000; // Leave room for advances

impl SimClock {
    /// Create a new clock starting at time 0.
    #[must_use]
    pub fn new() -> Self {
        Self {
            now_ns: AtomicU64::new(0),
        }
    }

    /// Create a clock starting at a specific time.
    #[must_use]
    pub fn with_start_time_ns(start_ns: u64) -> Self {
        debug_assert!(start_ns <= TIME_NS_MAX, "Start time too large");
        Self {
            now_ns: AtomicU64::new(start_ns),
        }
    }

    /// Get current time in nanoseconds.
    #[must_use]
    pub fn now_ns(&self) -> u64 {
        self.now_ns.load(Ordering::Acquire)
    }

    /// Get current time in microseconds.
    #[must_use]
    pub fn now_us(&self) -> u64 {
        self.now_ns() / 1_000
    }

    /// Get current time in milliseconds.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        self.now_ns() / 1_000_000
    }

    /// Advance time by the given number of nanoseconds.
    ///
    /// # Panics
    ///
    /// Panics if the advance would overflow.
    pub fn advance_ns(&self, delta_ns: u64) {
        debug_assert!(delta_ns > 0, "Delta must be positive");

        let current = self.now_ns.load(Ordering::Acquire);
        debug_assert!(
            current <= TIME_NS_MAX - delta_ns,
            "Time advance would overflow"
        );

        self.now_ns.fetch_add(delta_ns, Ordering::Release);
    }

    /// Advance time by the given number of microseconds.
    pub fn advance_us(&self, delta_us: u64) {
        debug_assert!(delta_us > 0, "Delta must be positive");
        self.advance_ns(delta_us * 1_000);
    }

    /// Advance time by the given number of milliseconds.
    pub fn advance_ms(&self, delta_ms: u64) {
        debug_assert!(delta_ms > 0, "Delta must be positive");
        self.advance_ns(delta_ms * 1_000_000);
    }

    /// Simulate a sleep by advancing time.
    ///
    /// Unlike real sleep, this returns immediately after advancing
    /// the clock. Use this in DST tests instead of actual sleeps.
    pub fn sleep_ns(&self, duration_ns: u64) {
        if duration_ns > 0 {
            self.advance_ns(duration_ns);
        }
    }

    /// Simulate a sleep in milliseconds.
    pub fn sleep_ms(&self, duration_ms: u64) {
        if duration_ms > 0 {
            self.advance_ms(duration_ms);
        }
    }

    /// Reset clock to zero.
    ///
    /// Useful for test isolation.
    pub fn reset(&self) {
        self.now_ns.store(0, Ordering::Release);
    }
}

impl Default for SimClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_starts_at_zero() {
        let clock = SimClock::new();
        assert_eq!(clock.now_ns(), 0);
    }

    #[test]
    fn test_clock_with_start_time() {
        let clock = SimClock::with_start_time_ns(1_000_000_000);
        assert_eq!(clock.now_ns(), 1_000_000_000);
        assert_eq!(clock.now_ms(), 1_000);
    }

    #[test]
    fn test_advance_time() {
        let clock = SimClock::new();

        clock.advance_ns(1_000_000); // 1ms
        assert_eq!(clock.now_ns(), 1_000_000);
        assert_eq!(clock.now_ms(), 1);

        clock.advance_ms(100);
        assert_eq!(clock.now_ms(), 101);
    }

    #[test]
    fn test_sleep() {
        let clock = SimClock::new();

        clock.sleep_ms(50);
        assert_eq!(clock.now_ms(), 50);

        clock.sleep_ns(500_000); // 0.5ms
        assert_eq!(clock.now_us(), 50_500);
    }

    #[test]
    fn test_reset() {
        let clock = SimClock::new();
        clock.advance_ms(100);
        assert_eq!(clock.now_ms(), 100);

        clock.reset();
        assert_eq!(clock.now_ns(), 0);
    }

    #[test]
    fn test_unit_conversions() {
        let clock = SimClock::new();
        clock.advance_ns(1_500_000_000); // 1.5 seconds

        assert_eq!(clock.now_ns(), 1_500_000_000);
        assert_eq!(clock.now_us(), 1_500_000);
        assert_eq!(clock.now_ms(), 1_500);
    }
}
