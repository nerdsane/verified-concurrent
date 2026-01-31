//! # vf-stateright
//!
//! Stateright models that mirror TLA+ specifications.
//!
//! Each model implements the same invariants as the corresponding TLA+ spec,
//! allowing exhaustive model checking in Rust.

pub mod treiber_stack;

pub use treiber_stack::{StackAction, StackModel, StackState};
