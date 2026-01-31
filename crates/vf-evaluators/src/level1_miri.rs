//! Level 1: miri evaluator.
//!
//! Runs `cargo +nightly miri test` to detect undefined behavior.

use std::path::Path;
use std::time::{Duration, Instant};

use tokio::process::Command;

use crate::result::EvaluatorResult;

/// Run miri on a crate's tests.
pub async fn run(crate_path: &Path, timeout: Duration) -> EvaluatorResult {
    let start = Instant::now();

    // First check if miri is available
    let miri_check = Command::new("cargo")
        .args(["+nightly", "miri", "--version"])
        .output()
        .await;

    if miri_check.is_err() || !miri_check.unwrap().status.success() {
        return EvaluatorResult::fail(
            "miri",
            "miri not installed. Run: rustup +nightly component add miri",
            start.elapsed(),
            String::new(),
        );
    }

    // Run miri on tests
    let result = tokio::time::timeout(
        timeout,
        Command::new("cargo")
            .args(["+nightly", "miri", "test"])
            .env("MIRIFLAGS", "-Zmiri-disable-isolation")
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
                EvaluatorResult::pass_with_output("miri", duration, combined)
            } else {
                let error = extract_miri_error(&stderr);
                EvaluatorResult::fail("miri", error, duration, combined)
            }
        }
        Ok(Err(e)) => EvaluatorResult::fail(
            "miri",
            format!("Failed to run miri: {}", e),
            duration,
            String::new(),
        ),
        Err(_) => EvaluatorResult::fail(
            "miri",
            format!("Timeout after {:?}", timeout),
            duration,
            String::new(),
        ),
    }
}

/// Extract miri's undefined behavior error.
fn extract_miri_error(stderr: &str) -> String {
    // Miri outputs "Undefined Behavior:" followed by the issue
    for line in stderr.lines() {
        if line.contains("Undefined Behavior:") {
            return line.to_string();
        }
        if line.contains("error: Undefined Behavior") {
            return line.to_string();
        }
    }

    // Look for general error
    for line in stderr.lines() {
        if line.starts_with("error:") {
            return line.to_string();
        }
    }

    stderr
        .lines()
        .find(|l| !l.is_empty())
        .unwrap_or("undefined behavior detected")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_miri_error() {
        let stderr = r#"
error: Undefined Behavior: trying to retag from <1234> for Unique permission at alloc1234[0x0],
       but that tag does not exist in the borrow stack for this location
"#;
        let error = extract_miri_error(stderr);
        assert!(error.contains("Undefined Behavior"));
    }
}
