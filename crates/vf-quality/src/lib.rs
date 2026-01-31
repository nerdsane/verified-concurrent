//! # vf-quality
//!
//! Code quality checkers for verified lock-free structures.
//!
//! ## TigerStyle
//!
//! Full implementation of the TigerStyle philosophy from tigerstyle.dev.
//! TigerStyle is not optional - it's a required part of the evaluation.
//!
//! ### Safety Rules (MUST pass)
//! - Defense-in-depth verification
//! - Explicit limits with `_MAX` suffix
//! - Static memory allocation
//! - 2+ assertions per function
//! - Zero dependencies (minimal Cargo.toml)
//! - u64 not usize for data fields
//!
//! ### Performance Rules (SHOULD pass)
//! - Primary Colors framework (network, storage, memory, compute)
//! - Control/Data plane separation
//! - Zero copy operations
//! - Cache-aligned structs
//!
//! ### Naming Rules (MUST pass)
//! - Big-endian naming (most significant first)
//! - Qualifiers at end
//! - Snake_case, no abbreviations

pub mod tigerstyle;

pub use tigerstyle::{TigerStyleChecker, TigerStyleResult, Violation};
