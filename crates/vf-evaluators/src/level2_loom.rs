//! Level 2: loom evaluator.
//!
//! Runs tests with loom enabled to explore thread interleavings.

use std::path::Path;
use std::time::{Duration, Instant};

use tokio::process::Command;

use crate::result::EvaluatorResult;

/// Run loom tests on a crate.
///
/// Loom tests must be annotated with `#[cfg(loom)]` and use
/// `loom::sync::atomic` instead of `std::sync::atomic`.
pub async fn run(crate_path: &Path, timeout: Duration, preemption_bound: usize) -> EvaluatorResult {
    let start = Instant::now();

    // Run tests with loom feature enabled
    let result = tokio::time::timeout(
        timeout,
        Command::new("cargo")
            .args(["test", "--release"])
            .env("RUSTFLAGS", "--cfg loom")
            .env("LOOM_MAX_PREEMPTIONS", preemption_bound.to_string())
            .current_dir(crate_path)
            .output(),
    )
    .await;

    let duration = start.elapsed();

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}\n{}", stdout, stderr);

            if output.status.success() {
                // Check for loom output indicating it actually ran
                let loom_ran = combined.contains("loom") || combined.contains("thread ");
                if loom_ran {
                    EvaluatorResult::pass_with_output("loom", duration, combined)
                } else {
                    // Loom tests might not exist, that's okay
                    EvaluatorResult::pass_with_output(
                        "loom",
                        duration,
                        "No loom tests found (add #[cfg(loom)] tests)".to_string(),
                    )
                }
            } else {
                let error = extract_loom_error(&stderr, &stdout);
                EvaluatorResult::fail("loom", error, duration, combined)
            }
        }
        Ok(Err(e)) => EvaluatorResult::fail(
            "loom",
            format!("Failed to run loom tests: {}", e),
            duration,
            String::new(),
        ),
        Err(_) => EvaluatorResult::fail(
            "loom",
            format!("Timeout after {:?}", timeout),
            duration,
            String::new(),
        ),
    }
}

/// Extract loom's error message.
fn extract_loom_error(stderr: &str, stdout: &str) -> String {
    // Loom reports panics with thread info
    for line in stderr.lines().chain(stdout.lines()) {
        if line.contains("panicked at") {
            return line.to_string();
        }
        if line.contains("assertion failed") {
            return line.to_string();
        }
        if line.contains("thread '") && line.contains("panicked") {
            return line.to_string();
        }
    }

    // Look for test failure
    for line in stdout.lines() {
        if line.contains("FAILED") {
            return line.to_string();
        }
    }

    "loom test failed".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_loom_error() {
        let stdout = r#"
running 1 test
thread 'test_concurrent_push_pop' panicked at 'assertion failed: pushed.is_subset(&contents)',
    src/treiber_stack.rs:150:9
test test_concurrent_push_pop ... FAILED
"#;
        let error = extract_loom_error("", stdout);
        assert!(error.contains("panicked") || error.contains("assertion failed"));
    }
}
