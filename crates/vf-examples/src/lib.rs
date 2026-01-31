//! # vf-examples
//!
//! Reference implementations of verified lock-free structures.
//!
//! Each implementation:
//! - Maps to a TLA+ spec in `specs/lockfree/`
//! - Implements the corresponding Properties trait from `vf-core`
//! - Has DST tests that verify invariants
//! - Has loom tests for thread interleavings (under `#[cfg(loom)]`)

pub mod treiber_stack;

pub use treiber_stack::TreiberStack;
