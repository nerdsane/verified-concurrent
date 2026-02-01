//! # vf-generator
//!
//! LLM-powered code generation with spec-guided verification.
//!
//! ## Philosophy: Bitter Lesson Aligned
//!
//! - **Derive prompts from specs** (no implementation hints)
//! - **Let LLM figure out implementation** strategy
//! - **Use cascade feedback diagnostically** (what failed, not how to fix)
//! - **Performance is first-class** - iterate toward best performing correct solution
//!
//! ## Two-Phase Generation
//!
//! 1. **Correctness Phase**: Iterate until cascade passes
//! 2. **Performance Phase**: Iterate toward target progress guarantee
//!
//! # Usage
//!
//! ```bash
//! # Generate implementation from TLA+ spec
//! cargo run -p vf-generator -- --spec specs/lockfree/treiber_stack.tla
//!
//! # With custom API key
//! ANTHROPIC_API_KEY=sk-... cargo run -p vf-generator -- --spec specs/lockfree/treiber_stack.tla
//! ```
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │  TLA+ Spec  │ ──> │   Generic   │ ──> │   Claude    │
//! │             │     │   Prompt    │     │     API     │
//! └─────────────┘     └─────────────┘     └──────┬──────┘
//!                                                │
//!                     ┌──────────────────────────┘
//!                     ▼
//!              ┌─────────────┐
//!              │  Generated  │
//!              │    Code     │
//!              └──────┬──────┘
//!                     │
//!     ┌───────────────┴───────────────┐
//!     ▼                               ▼
//! ┌─────────────┐               ┌─────────────┐
//! │  Cascade    │  (if passes)  │  vf-perf    │
//! │  Verifier   │ ────────────> │  Analyzer   │
//! └──────┬──────┘               └──────┬──────┘
//!        │                             │
//! (if fails)                    (if below target)
//!        │                             │
//!        ▼                             ▼
//! ┌─────────────┐               ┌─────────────┐
//! │    Fix      │               │   Perf      │
//! │   Prompt    │               │  Improve    │
//! └─────────────┘               └─────────────┘
//! ```

pub mod client;
pub mod generator;
pub mod prompt;

pub use client::{ClaudeClient, ClaudeConfig, Message, Role};
pub use generator::{AttemptRecord, CodeGenerator, GeneratorConfig, GeneratorResult};
pub use prompt::{extract_code_block, PromptBuilder};
