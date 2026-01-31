//! # vf-dst
//!
//! Deterministic Simulation Testing framework for verified lock-free structures.
//!
//! Inspired by FoundationDB and TigerBeetle, this crate provides deterministic
//! simulation of time, randomness, and faults. All behavior is reproducible
//! via a seed.
//!
//! ## Usage
//!
//! ```rust
//! use vf_dst::DstEnv;
//!
//! let seed = 12345;
//! let mut env = DstEnv::new(seed);
//!
//! // Deterministic time
//! let now = env.clock().now_ns();
//! env.clock().advance_ns(1_000_000); // 1ms
//!
//! // Deterministic randomness
//! let value: u64 = env.rng().gen();
//! let choice = env.rng().gen_range(0..10);
//!
//! // Deterministic fault injection
//! if env.fault().should_fail() {
//!     // Simulate failure
//! }
//! ```
//!
//! ## Reproducibility
//!
//! To reproduce a failing test:
//! ```bash
//! DST_SEED=12345 cargo test
//! ```

pub mod clock;
pub mod env;
pub mod fault;
pub mod random;
pub mod scheduler;

pub use clock::SimClock;
pub use env::DstEnv;
pub use fault::{FaultConfig, FaultInjector};
pub use random::DeterministicRng;
pub use scheduler::{ScheduleDecision, Scheduler};

/// Get DST seed from environment or generate random one.
///
/// Prints the seed for reproduction. Use `DST_SEED=<seed>` to reproduce.
#[must_use]
pub fn get_or_generate_seed() -> u64 {
    match std::env::var("DST_SEED") {
        Ok(s) => {
            let seed: u64 = s.parse().expect("DST_SEED must be a valid u64");
            println!("DST_SEED={} (from environment)", seed);
            seed
        }
        Err(_) => {
            let seed = rand::random::<u64>();
            println!("DST_SEED={} (randomly generated)", seed);
            seed
        }
    }
}
