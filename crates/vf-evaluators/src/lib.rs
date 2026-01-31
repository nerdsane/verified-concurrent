//! # vf-evaluators
//!
//! Evaluator cascade for verified lock-free structures.
//!
//! The cascade runs evaluators in order of speed (fastest first):
//!
//! | Level | Tool | Time | Catches |
//! |-------|------|------|---------|
//! | 0 | rustc | instant | Type errors, lifetime issues |
//! | 1 | miri | seconds | Undefined behavior, aliasing |
//! | 2 | loom | seconds | Race conditions, memory ordering |
//! | 3 | DST | seconds | Faults, crashes, delays |
//! | 4 | stateright | seconds | Invariant violations |
//! | 5 | kani | minutes | Bounded proofs |
//!
//! The cascade stops at the first failure, providing a counterexample.

pub mod cascade;
pub mod level0_rustc;
pub mod level1_miri;
pub mod level2_loom;
pub mod level3_dst;
pub mod result;

pub use cascade::{CascadeConfig, EvaluatorCascade, EvaluatorLevel};
pub use result::{CascadeResult, EvaluatorResult};
