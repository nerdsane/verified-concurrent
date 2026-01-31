//! Invariant traits for verified data structures.
//!
//! Each module defines the properties that implementations must satisfy,
//! all traceable to TLA+ specifications.

pub mod stack;

pub use stack::{StackHistory, StackOperation, StackProperties, StackPropertyChecker};
