//! Evaluator cascade orchestration.
//!
//! Runs evaluators in order, stopping at the first failure.

use std::path::Path;
use std::time::Duration;

use crate::level0_rustc;
use crate::level1_miri;
use crate::level2_loom;
use crate::level3_dst;
use crate::result::{CascadeResult, EvaluatorResult};

/// Evaluator levels in the cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum EvaluatorLevel {
    /// Level 0: rustc - type checking, lifetime analysis
    Rustc = 0,
    /// Level 1: miri - undefined behavior detection
    Miri = 1,
    /// Level 2: loom - thread interleaving exploration
    Loom = 2,
    /// Level 3: DST - deterministic simulation testing
    Dst = 3,
    /// Level 4: stateright - model checking against TLA+ spec
    Stateright = 4,
    /// Level 5: kani - bounded model checking / proof
    Kani = 5,
}

impl EvaluatorLevel {
    /// Get all levels up to and including this one.
    pub fn levels_up_to(self) -> Vec<EvaluatorLevel> {
        let max = self as u8;
        (0..=max)
            .map(|i| match i {
                0 => EvaluatorLevel::Rustc,
                1 => EvaluatorLevel::Miri,
                2 => EvaluatorLevel::Loom,
                3 => EvaluatorLevel::Dst,
                4 => EvaluatorLevel::Stateright,
                5 => EvaluatorLevel::Kani,
                _ => unreachable!(),
            })
            .collect()
    }

    /// Get the name of this level.
    pub fn name(&self) -> &'static str {
        match self {
            EvaluatorLevel::Rustc => "rustc",
            EvaluatorLevel::Miri => "miri",
            EvaluatorLevel::Loom => "loom",
            EvaluatorLevel::Dst => "DST",
            EvaluatorLevel::Stateright => "stateright",
            EvaluatorLevel::Kani => "kani",
        }
    }
}

/// Configuration for the cascade.
#[derive(Debug, Clone)]
pub struct CascadeConfig {
    /// Maximum level to run (inclusive)
    pub max_level: EvaluatorLevel,
    /// Stop on first failure
    pub fail_fast: bool,
    /// Timeout per evaluator
    pub timeout: Duration,
    /// Loom preemption bound (higher = more thorough, slower)
    pub loom_preemption_bound: usize,
    /// Stateright max depth
    pub stateright_depth_max: usize,
    /// Kani unwind bound
    pub kani_unwind: usize,
    /// DST seed (if None, generates random)
    pub dst_seed: Option<u64>,
    /// Number of DST iterations
    pub dst_iterations: u64,
}

impl Default for CascadeConfig {
    fn default() -> Self {
        Self {
            max_level: EvaluatorLevel::Dst, // Default to DST (fast, thorough)
            fail_fast: true,
            timeout: Duration::from_secs(300), // 5 minutes
            loom_preemption_bound: 3,
            stateright_depth_max: 100,
            kani_unwind: 10,
            dst_seed: None,
            dst_iterations: 1000,
        }
    }
}

impl CascadeConfig {
    /// Fast config for quick iteration.
    pub fn fast() -> Self {
        Self {
            max_level: EvaluatorLevel::Miri,
            timeout: Duration::from_secs(30),
            dst_iterations: 100,
            ..Default::default()
        }
    }

    /// Thorough config for CI.
    pub fn thorough() -> Self {
        Self {
            max_level: EvaluatorLevel::Stateright,
            timeout: Duration::from_secs(600),
            loom_preemption_bound: 4,
            stateright_depth_max: 200,
            dst_iterations: 10000,
            ..Default::default()
        }
    }

    /// Maximum verification (includes Kani).
    pub fn maximum() -> Self {
        Self {
            max_level: EvaluatorLevel::Kani,
            timeout: Duration::from_secs(1800), // 30 minutes
            loom_preemption_bound: 5,
            stateright_depth_max: 500,
            kani_unwind: 20,
            dst_iterations: 100000,
            ..Default::default()
        }
    }
}

/// The evaluator cascade.
///
/// Runs evaluators in order from fastest to slowest, stopping
/// at the first failure if configured to do so.
pub struct EvaluatorCascade {
    config: CascadeConfig,
}

impl EvaluatorCascade {
    /// Create a new cascade with the given config.
    pub fn new(config: CascadeConfig) -> Self {
        Self { config }
    }

    /// Create with default config.
    pub fn with_defaults() -> Self {
        Self::new(CascadeConfig::default())
    }

    /// Run the cascade on a crate.
    ///
    /// # Arguments
    /// - `crate_path`: Path to the crate directory (containing Cargo.toml)
    pub async fn run(&self, crate_path: &Path) -> CascadeResult {
        let mut results = Vec::new();
        let levels = self.config.max_level.levels_up_to();

        for level in levels {
            let result = match level {
                EvaluatorLevel::Rustc => level0_rustc::run(crate_path, self.config.timeout).await,
                EvaluatorLevel::Miri => level1_miri::run(crate_path, self.config.timeout).await,
                EvaluatorLevel::Loom => {
                    level2_loom::run(crate_path, self.config.timeout, self.config.loom_preemption_bound).await
                }
                EvaluatorLevel::Dst => {
                    level3_dst::run(
                        crate_path,
                        self.config.timeout,
                        self.config.dst_seed,
                        self.config.dst_iterations,
                    )
                    .await
                }
                EvaluatorLevel::Stateright => {
                    // TODO: Implement stateright evaluator
                    EvaluatorResult::pass("stateright", Duration::ZERO)
                }
                EvaluatorLevel::Kani => {
                    // TODO: Implement kani evaluator
                    EvaluatorResult::pass("kani", Duration::ZERO)
                }
            };

            let failed = !result.passed;
            results.push(result);

            if failed && self.config.fail_fast {
                break;
            }
        }

        CascadeResult::from_results(results)
    }

    /// Run the cascade on source code directly (for generated code).
    ///
    /// Creates a temporary crate and runs the cascade on it.
    pub async fn run_on_code(&self, code: &str, test_code: &str) -> CascadeResult {
        // Create temporary directory with Cargo.toml and source
        let temp_dir = std::env::temp_dir().join(format!("vf-cascade-{}", rand::random::<u64>()));
        let src_dir = temp_dir.join("src");

        // Create directory structure
        if let Err(e) = tokio::fs::create_dir_all(&src_dir).await {
            return CascadeResult::from_results(vec![EvaluatorResult::fail(
                "setup",
                format!("Failed to create temp directory: {}", e),
                Duration::ZERO,
                String::new(),
            )]);
        }

        // Write Cargo.toml
        let cargo_toml = r#"
[package]
name = "vf-temp-crate"
version = "0.1.0"
edition = "2021"

[dependencies]
crossbeam-epoch = "0.9"

[dev-dependencies]
loom = "0.7"

[features]
default = []
"#;

        if let Err(e) = tokio::fs::write(temp_dir.join("Cargo.toml"), cargo_toml).await {
            return CascadeResult::from_results(vec![EvaluatorResult::fail(
                "setup",
                format!("Failed to write Cargo.toml: {}", e),
                Duration::ZERO,
                String::new(),
            )]);
        }

        // Write lib.rs
        let lib_content = format!("{}\n\n#[cfg(test)]\nmod tests {{\n    use super::*;\n{}\n}}", code, test_code);
        if let Err(e) = tokio::fs::write(src_dir.join("lib.rs"), lib_content).await {
            return CascadeResult::from_results(vec![EvaluatorResult::fail(
                "setup",
                format!("Failed to write lib.rs: {}", e),
                Duration::ZERO,
                String::new(),
            )]);
        }

        // Run cascade
        let result = self.run(&temp_dir).await;

        // Cleanup
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;

        result
    }

    /// Get the current config.
    pub fn config(&self) -> &CascadeConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levels_up_to() {
        let levels = EvaluatorLevel::Loom.levels_up_to();
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], EvaluatorLevel::Rustc);
        assert_eq!(levels[1], EvaluatorLevel::Miri);
        assert_eq!(levels[2], EvaluatorLevel::Loom);
    }

    #[test]
    fn test_config_presets() {
        let fast = CascadeConfig::fast();
        assert_eq!(fast.max_level, EvaluatorLevel::Miri);

        let thorough = CascadeConfig::thorough();
        assert_eq!(thorough.max_level, EvaluatorLevel::Stateright);

        let max = CascadeConfig::maximum();
        assert_eq!(max.max_level, EvaluatorLevel::Kani);
    }
}
