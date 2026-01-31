//! Level 0: rustc evaluator.
//!
//! Runs `cargo check` to verify type correctness and lifetimes.

use std::path::Path;
use std::time::{Duration, Instant};

use tokio::process::Command;

use crate::result::EvaluatorResult;

/// Run rustc type checking on a crate.
pub async fn run(crate_path: &Path, timeout: Duration) -> EvaluatorResult {
    let start = Instant::now();

    let result = tokio::time::timeout(
        timeout,
        Command::new("cargo")
            .args(["check", "--all-targets"])
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
                EvaluatorResult::pass_with_output("rustc", duration, combined)
            } else {
                let error = extract_rustc_error(&stderr);
                EvaluatorResult::fail("rustc", error, duration, combined)
            }
        }
        Ok(Err(e)) => EvaluatorResult::fail(
            "rustc",
            format!("Failed to run cargo check: {}", e),
            duration,
            String::new(),
        ),
        Err(_) => EvaluatorResult::fail(
            "rustc",
            format!("Timeout after {:?}", timeout),
            duration,
            String::new(),
        ),
    }
}

/// Extract the first error message from rustc output.
fn extract_rustc_error(stderr: &str) -> String {
    // Look for "error[E...]:" pattern
    for line in stderr.lines() {
        if line.starts_with("error[E") || line.starts_with("error:") {
            return line.to_string();
        }
    }

    // Fallback to first non-empty line
    stderr
        .lines()
        .find(|l| !l.is_empty())
        .unwrap_or("unknown error")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_error() {
        let stderr = r#"
   Compiling foo v0.1.0
error[E0382]: borrow of moved value: `x`
  --> src/lib.rs:10:5
   |
10 |     println!("{}", x);
   |                    ^ value borrowed here after move
"#;
        let error = extract_rustc_error(stderr);
        assert!(error.contains("E0382"));
    }
}
