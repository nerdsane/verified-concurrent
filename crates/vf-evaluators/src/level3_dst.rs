//! Level 3: DST evaluator.
//!
//! Runs Deterministic Simulation Testing with fault injection.

use std::path::Path;
use std::time::{Duration, Instant};

use tokio::process::Command;
use vf_core::Counterexample;

use crate::result::EvaluatorResult;

/// Run DST tests on a crate.
///
/// DST tests use the `vf-dst` framework for deterministic simulation.
pub async fn run(
    crate_path: &Path,
    timeout: Duration,
    seed: Option<u64>,
    iterations: u64,
) -> EvaluatorResult {
    let start = Instant::now();

    // Build the test command
    let mut cmd = Command::new("cargo");
    cmd.args(["test", "--release", "--", "--test-threads=1"]);
    cmd.current_dir(crate_path);

    // Set DST seed if provided
    if let Some(s) = seed {
        cmd.env("DST_SEED", s.to_string());
    }

    // Set iterations
    cmd.env("DST_ITERATIONS", iterations.to_string());

    let result = tokio::time::timeout(timeout, cmd.output()).await;

    let duration = start.elapsed();

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}\n{}", stdout, stderr);

            if output.status.success() {
                EvaluatorResult::pass_with_output("DST", duration, combined)
            } else {
                let (error, counterexample) = extract_dst_error(&stderr, &stdout);
                if let Some(ce) = counterexample {
                    EvaluatorResult::fail_with_counterexample("DST", error, ce, duration, combined)
                } else {
                    EvaluatorResult::fail("DST", error, duration, combined)
                }
            }
        }
        Ok(Err(e)) => EvaluatorResult::fail(
            "DST",
            format!("Failed to run DST tests: {}", e),
            duration,
            String::new(),
        ),
        Err(_) => EvaluatorResult::fail(
            "DST",
            format!("Timeout after {:?}", timeout),
            duration,
            String::new(),
        ),
    }
}

/// Extract DST error and seed for reproduction.
fn extract_dst_error(stderr: &str, stdout: &str) -> (String, Option<Counterexample>) {
    let mut seed: Option<u64> = None;
    let mut error = String::new();

    for line in stderr.lines().chain(stdout.lines()) {
        // Look for DST_SEED in output
        if line.contains("DST_SEED=") {
            if let Some(seed_str) = line.split("DST_SEED=").nth(1) {
                if let Some(num_str) = seed_str.split_whitespace().next() {
                    if let Ok(s) = num_str.parse::<u64>() {
                        seed = Some(s);
                    }
                }
            }
        }

        // Look for panic message
        if line.contains("panicked at") || line.contains("assertion failed") {
            error = line.to_string();
        }
    }

    if error.is_empty() {
        error = "DST test failed".to_string();
    }

    let counterexample = seed.map(Counterexample::with_seed);

    (error, counterexample)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_dst_error() {
        let stdout = r#"
running 1 test
DST_SEED=12345 (randomly generated)
thread 'test_stack_under_faults' panicked at 'assertion failed: checker.all_hold()',
    src/treiber_stack.rs:200:9
test test_stack_under_faults ... FAILED
"#;
        let (error, ce) = extract_dst_error("", stdout);
        assert!(error.contains("panicked") || error.contains("assertion failed"));
        assert!(ce.is_some());
        assert_eq!(ce.unwrap().dst_seed, Some(12345));
    }
}
