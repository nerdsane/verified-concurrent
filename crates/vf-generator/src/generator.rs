//! Code generator with verification loop.
//!
//! Implements the generate → verify → optimize cycle.
//! Bitter lesson aligned: derive everything from specs, let LLM figure out implementation.
//! Performance is first-class: iterate toward best performing correct solution.

use std::path::Path;
use std::time::{Duration, Instant};

use vf_core::TlaSpec;
use vf_evaluators::{CascadeConfig, CascadeResult, EvaluatorCascade};
use vf_perf::{analyze_progress_guarantee, ProgressGuarantee};

use crate::client::{ClaudeClient, ClientError, Message};
use crate::prompt::{extract_code_block, PromptBuilder};

/// Generator configuration.
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Maximum number of correctness attempts
    pub max_correctness_attempts: u32,
    /// Maximum number of performance improvement attempts (after correctness)
    pub max_perf_attempts: u32,
    /// Minimum acceptable progress guarantee
    pub min_progress_guarantee: ProgressGuarantee,
    /// Target progress guarantee (will try to achieve)
    pub target_progress_guarantee: ProgressGuarantee,
    /// Cascade configuration for verification
    pub cascade_config: CascadeConfig,
    /// Whether to print verbose output
    pub verbose: bool,
    /// Output directory for generated code
    pub output_dir: Option<String>,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            max_correctness_attempts: 5,
            max_perf_attempts: 3,
            min_progress_guarantee: ProgressGuarantee::Blocking,
            target_progress_guarantee: ProgressGuarantee::LockFree,
            cascade_config: CascadeConfig::default(),
            verbose: false,
            output_dir: None,
        }
    }
}

impl GeneratorConfig {
    /// Quick config for fast iteration.
    pub fn quick() -> Self {
        Self {
            max_correctness_attempts: 3,
            max_perf_attempts: 1,
            cascade_config: CascadeConfig::fast(),
            verbose: true,
            ..Default::default()
        }
    }

    /// Thorough config for production.
    pub fn thorough() -> Self {
        Self {
            max_correctness_attempts: 10,
            max_perf_attempts: 5,
            target_progress_guarantee: ProgressGuarantee::WaitFree,
            cascade_config: CascadeConfig::thorough(),
            verbose: true,
            ..Default::default()
        }
    }
}

/// Result from code generation.
#[derive(Debug, Clone)]
pub struct GeneratorResult {
    /// Whether generation succeeded (correct AND meets min performance)
    pub success: bool,
    /// The generated code (if successful)
    pub code: Option<String>,
    /// Correctness attempts made
    pub correctness_attempts: u32,
    /// Performance improvement attempts made
    pub perf_attempts: u32,
    /// Final progress guarantee achieved
    pub progress_guarantee: Option<ProgressGuarantee>,
    /// Total duration
    pub duration: Duration,
    /// Cascade result from final attempt
    pub cascade_result: Option<CascadeResult>,
    /// History of all attempts
    pub attempt_history: Vec<AttemptRecord>,
}

/// Record of a single generation attempt.
#[derive(Debug, Clone)]
pub struct AttemptRecord {
    /// Attempt number (1-indexed)
    pub attempt: u32,
    /// Phase: "correctness" or "performance"
    pub phase: String,
    /// Generated code
    pub code: String,
    /// Cascade result
    pub cascade_result: CascadeResult,
    /// Progress guarantee (if correctness passed)
    pub progress_guarantee: Option<ProgressGuarantee>,
    /// Duration of this attempt
    pub duration: Duration,
}

impl GeneratorResult {
    /// Format as a summary string.
    pub fn format_summary(&self) -> String {
        let status = if self.success { "SUCCESS" } else { "FAILED" };
        let mut summary = format!(
            "[{}] Generation completed in {:.2}s\n",
            status,
            self.duration.as_secs_f64(),
        );

        summary.push_str(&format!(
            "  Correctness attempts: {}\n",
            self.correctness_attempts
        ));
        summary.push_str(&format!(
            "  Performance attempts: {}\n",
            self.perf_attempts
        ));

        if let Some(progress) = self.progress_guarantee {
            summary.push_str(&format!(
                "  Progress guarantee: {:?} ({})\n",
                progress,
                progress.description()
            ));
        }

        if let Some(ref result) = self.cascade_result {
            summary.push_str(&result.format_report());
        }

        if self.success {
            if let Some(ref code) = self.code {
                let lines = code.lines().count();
                summary.push_str(&format!("\nGenerated {} lines of code.\n", lines));
            }
        } else {
            summary.push_str("\nGeneration failed.\n");
            for record in &self.attempt_history {
                if let Some(ref failure) = record.cascade_result.first_failure {
                    summary.push_str(&format!(
                        "  {} #{}: Failed at {} - {}\n",
                        record.phase,
                        record.attempt,
                        failure.evaluator,
                        failure.error.as_deref().unwrap_or("unknown")
                    ));
                }
            }
        }

        summary
    }
}

/// LLM-powered code generator with verification.
///
/// Philosophy: Bitter lesson aligned
/// - Derive prompts from specs (no implementation hints)
/// - Let LLM figure out implementation
/// - Use cascade feedback diagnostically
/// - Iterate toward best performing correct solution
pub struct CodeGenerator {
    client: ClaudeClient,
    config: GeneratorConfig,
}

impl CodeGenerator {
    /// Create a new generator with the given client and config.
    pub fn new(client: ClaudeClient, config: GeneratorConfig) -> Self {
        Self { client, config }
    }

    /// Create from environment variables.
    pub fn from_env(config: GeneratorConfig) -> Result<Self, ClientError> {
        let client = ClaudeClient::from_env()?;
        Ok(Self::new(client, config))
    }

    /// Generate implementation from a TLA+ spec file.
    pub async fn generate_from_file(&self, spec_path: &Path) -> Result<GeneratorResult, GeneratorError> {
        let spec = TlaSpec::from_file(spec_path)
            .map_err(|e| GeneratorError::SpecError(e.to_string()))?;

        self.generate(&spec).await
    }

    /// Generate implementation from a TLA+ spec.
    ///
    /// Two-phase approach:
    /// 1. Correctness phase: iterate until cascade passes
    /// 2. Performance phase: iterate toward target progress guarantee
    pub async fn generate(&self, spec: &TlaSpec) -> Result<GeneratorResult, GeneratorError> {
        let start = Instant::now();
        let mut attempt_history = Vec::new();
        let mut correctness_attempts = 0;
        let mut perf_attempts = 0;

        if self.config.verbose {
            println!("=== GENERATION START ===");
            println!("Module: {}", spec.name);
            println!("Invariants: {}", spec.format_invariants());
            println!("Target progress: {:?}", self.config.target_progress_guarantee);
            println!();
        }

        // Phase 1: Correctness
        let mut current_code: Option<String> = None;
        let mut cascade_result: Option<CascadeResult> = None;

        for attempt in 1..=self.config.max_correctness_attempts {
            correctness_attempts = attempt;
            let attempt_start = Instant::now();

            if self.config.verbose {
                println!("=== Correctness Attempt {}/{} ===",
                    attempt, self.config.max_correctness_attempts);
            }

            // Generate or fix code
            let code = if let Some(ref prev_code) = current_code {
                self.fix_code(spec, prev_code, cascade_result.as_ref()).await?
            } else {
                self.generate_initial(spec).await?
            };

            if self.config.verbose {
                println!("Generated {} lines of code", code.lines().count());
            }

            // Verify with cascade
            let result = self.verify_code(&code, spec).await?;
            let passed = result.all_passed;

            let record = AttemptRecord {
                attempt,
                phase: "correctness".to_string(),
                code: code.clone(),
                cascade_result: result.clone(),
                progress_guarantee: None,
                duration: attempt_start.elapsed(),
            };
            attempt_history.push(record);

            if passed {
                if self.config.verbose {
                    println!("✅ Correctness achieved!");
                }
                current_code = Some(code);
                cascade_result = Some(result);
                break;
            }

            // Log failure
            if self.config.verbose {
                if let Some(ref failure) = result.first_failure {
                    println!(
                        "❌ Failed at {}: {}",
                        failure.evaluator,
                        failure.error.as_deref().unwrap_or("unknown")
                    );
                }
            }

            current_code = Some(code);
            cascade_result = Some(result);
        }

        // Check if correctness was achieved
        let code = match current_code {
            Some(c) if cascade_result.as_ref().map(|r| r.all_passed).unwrap_or(false) => c,
            _ => {
                return Ok(GeneratorResult {
                    success: false,
                    code: current_code,
                    correctness_attempts,
                    perf_attempts: 0,
                    progress_guarantee: None,
                    duration: start.elapsed(),
                    cascade_result,
                    attempt_history,
                });
            }
        };

        // Phase 2: Performance optimization
        let mut best_code = code;
        let mut best_progress = analyze_progress_guarantee(&best_code);

        if self.config.verbose {
            println!();
            println!("=== PERFORMANCE PHASE ===");
            println!("Current progress: {:?} ({})", best_progress, best_progress.description());
            println!("Target progress: {:?}", self.config.target_progress_guarantee);
        }

        // Check if we already meet target
        if best_progress >= self.config.target_progress_guarantee {
            if self.config.verbose {
                println!("✅ Already at or above target progress guarantee!");
            }
        } else {
            // Try to improve performance
            for attempt in 1..=self.config.max_perf_attempts {
                perf_attempts = attempt;
                let attempt_start = Instant::now();

                if self.config.verbose {
                    println!();
                    println!("=== Performance Attempt {}/{} ===",
                        attempt, self.config.max_perf_attempts);
                }

                // Ask for performance improvement
                let improved_code = self.improve_performance(
                    spec,
                    &best_code,
                    best_progress,
                    self.config.target_progress_guarantee,
                ).await?;

                // Verify correctness still holds
                let result = self.verify_code(&improved_code, spec).await?;

                if result.all_passed {
                    let new_progress = analyze_progress_guarantee(&improved_code);

                    let record = AttemptRecord {
                        attempt,
                        phase: "performance".to_string(),
                        code: improved_code.clone(),
                        cascade_result: result.clone(),
                        progress_guarantee: Some(new_progress),
                        duration: attempt_start.elapsed(),
                    };
                    attempt_history.push(record);

                    if new_progress > best_progress {
                        if self.config.verbose {
                            println!("✅ Improved: {:?} -> {:?}", best_progress, new_progress);
                        }
                        best_code = improved_code;
                        best_progress = new_progress;
                        cascade_result = Some(result);

                        if best_progress >= self.config.target_progress_guarantee {
                            if self.config.verbose {
                                println!("✅ Target progress guarantee achieved!");
                            }
                            break;
                        }
                    } else {
                        if self.config.verbose {
                            println!("⚠️ No improvement: still {:?}", new_progress);
                        }
                    }
                } else {
                    if self.config.verbose {
                        println!("❌ Performance attempt broke correctness, reverting");
                    }
                    let record = AttemptRecord {
                        attempt,
                        phase: "performance".to_string(),
                        code: improved_code,
                        cascade_result: result,
                        progress_guarantee: None,
                        duration: attempt_start.elapsed(),
                    };
                    attempt_history.push(record);
                }
            }
        }

        // Check if we meet minimum requirement
        let meets_minimum = best_progress >= self.config.min_progress_guarantee;

        if self.config.verbose {
            println!();
            println!("=== GENERATION COMPLETE ===");
            println!("Final progress: {:?}", best_progress);
            println!("Meets minimum ({:?}): {}", self.config.min_progress_guarantee, meets_minimum);
        }

        Ok(GeneratorResult {
            success: meets_minimum,
            code: Some(best_code),
            correctness_attempts,
            perf_attempts,
            progress_guarantee: Some(best_progress),
            duration: start.elapsed(),
            cascade_result,
            attempt_history,
        })
    }

    /// Generate initial implementation.
    async fn generate_initial(&self, spec: &TlaSpec) -> Result<String, GeneratorError> {
        let prompt = PromptBuilder::build_generation_prompt(spec);
        let system = PromptBuilder::system_prompt().to_string();

        let messages = vec![Message::user(prompt)];

        let response = self
            .client
            .complete_with_system(messages, Some(system))
            .await
            .map_err(GeneratorError::ClientError)?;

        extract_code_block(&response)
            .ok_or_else(|| GeneratorError::NoCodeInResponse(response))
    }

    /// Fix code based on verification failure.
    async fn fix_code(
        &self,
        spec: &TlaSpec,
        previous_code: &str,
        previous_result: Option<&CascadeResult>,
    ) -> Result<String, GeneratorError> {
        let prompt = if let Some(result) = previous_result {
            PromptBuilder::build_fix_prompt(spec, previous_code, result)
        } else {
            format!(
                "The following code has bugs. Fix it:\n\n```rust\n{}\n```",
                previous_code
            )
        };

        let system = PromptBuilder::system_prompt().to_string();
        let messages = vec![Message::user(prompt)];

        let response = self
            .client
            .complete_with_system(messages, Some(system))
            .await
            .map_err(GeneratorError::ClientError)?;

        extract_code_block(&response)
            .ok_or_else(|| GeneratorError::NoCodeInResponse(response))
    }

    /// Request performance improvement while preserving correctness.
    async fn improve_performance(
        &self,
        spec: &TlaSpec,
        current_code: &str,
        current_progress: ProgressGuarantee,
        target_progress: ProgressGuarantee,
    ) -> Result<String, GeneratorError> {
        let prompt = PromptBuilder::build_perf_improvement_prompt(
            spec,
            current_code,
            current_progress,
            target_progress,
        );

        let system = PromptBuilder::system_prompt().to_string();
        let messages = vec![Message::user(prompt)];

        let response = self
            .client
            .complete_with_system(messages, Some(system))
            .await
            .map_err(GeneratorError::ClientError)?;

        extract_code_block(&response)
            .ok_or_else(|| GeneratorError::NoCodeInResponse(response))
    }

    /// Verify code using the evaluator cascade.
    async fn verify_code(
        &self,
        code: &str,
        spec: &TlaSpec,
    ) -> Result<CascadeResult, GeneratorError> {
        let cascade = EvaluatorCascade::new(self.config.cascade_config.clone());

        // Generate test code based on spec content
        let test_code = derive_test_code(spec);

        let result = cascade.run_on_code(code, &test_code).await;
        Ok(result)
    }
}

/// Derive test code from spec content.
///
/// Examines the spec to determine what operations exist and generates appropriate tests.
fn derive_test_code(spec: &TlaSpec) -> String {
    let content_lower = spec.content.to_lowercase();

    // Detect spec type from content
    let is_stack = content_lower.contains("push") && content_lower.contains("pop");
    let is_queue = content_lower.contains("enqueue") && content_lower.contains("dequeue");
    let is_ssi = content_lower.contains("commit") && content_lower.contains("in_conflict");

    if is_ssi {
        generate_ssi_tests()
    } else if is_stack {
        generate_stack_tests()
    } else if is_queue {
        generate_queue_tests()
    } else {
        // Generic tests based on variables
        generate_generic_tests(spec)
    }
}

fn generate_stack_tests() -> String {
    r#"
    #[test]
    fn test_basic_operations() {
        let stack = TreiberStack::new();
        stack.push(1);
        stack.push(2);
        stack.push(3);
        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn test_lifo_order() {
        let stack = TreiberStack::new();
        for i in 1..=5 {
            stack.push(i);
        }
        for i in (1..=5).rev() {
            assert_eq!(stack.pop(), Some(i));
        }
    }

    #[test]
    fn test_is_empty() {
        let stack = TreiberStack::new();
        assert!(stack.is_empty());
        stack.push(42);
        assert!(!stack.is_empty());
        stack.pop();
        assert!(stack.is_empty());
    }

    #[test]
    fn test_no_lost_elements() {
        let stack = TreiberStack::new();
        stack.push(10);
        stack.push(20);
        stack.push(30);

        let pushed = stack.pushed_elements();
        let contents = stack.get_contents();

        for &val in &pushed {
            assert!(contents.contains(&val), "Lost element: {}", val);
        }
    }

    #[test]
    fn test_invariants() {
        let stack = TreiberStack::new();
        stack.push(100);
        stack.push(200);
        stack.push(300);

        let popped_val = stack.pop();
        assert_eq!(popped_val, Some(300));

        let pushed = stack.pushed_elements();
        let popped = stack.popped_elements();
        let contents = stack.get_contents();

        for &val in &pushed {
            let in_stack = contents.contains(&val);
            let was_popped = popped.contains(&val);
            assert!(in_stack || was_popped,
                "NoLostElements violated: {} neither in stack nor popped", val);
        }
    }
"#.to_string()
}

fn generate_ssi_tests() -> String {
    r#"
    #[test]
    fn test_simple_transaction() {
        let store = SsiStore::new();
        let t1 = store.begin();
        assert!(store.write(t1, 1, 100));
        assert!(store.commit(t1));

        let t2 = store.begin();
        assert_eq!(store.read(t2, 1), Some(100));
        assert!(store.commit(t2));
    }

    #[test]
    fn test_snapshot_isolation() {
        let store = SsiStore::new();

        let t1 = store.begin();
        assert!(store.write(t1, 1, 100));
        assert!(store.commit(t1));

        let t2 = store.begin();
        assert_eq!(store.read(t2, 1), Some(100));

        let t3 = store.begin();
        assert!(store.write(t3, 1, 200));
        assert!(store.commit(t3));

        // T2 should still see old value
        assert_eq!(store.read(t2, 1), Some(100));
        assert!(store.commit(t2));
    }

    #[test]
    fn test_dangerous_structure_abort() {
        let store = SsiStore::new();

        let setup = store.begin();
        store.write(setup, 1, 10);
        store.write(setup, 2, 20);
        store.commit(setup);

        let t1 = store.begin();
        let t2 = store.begin();

        store.read(t1, 1);
        store.read(t2, 2);
        store.write(t2, 1, 11);
        store.commit(t2);
        store.write(t1, 2, 21);

        let committed = store.commit(t1);
        assert!(!committed, "T1 should abort due to dangerous structure");
    }

    #[test]
    fn test_disjoint_keys_commit() {
        let store = SsiStore::new();

        let t1 = store.begin();
        let t2 = store.begin();

        assert!(store.write(t1, 1, 100));
        assert!(store.write(t2, 2, 200));

        assert!(store.commit(t1));
        assert!(store.commit(t2));

        let t3 = store.begin();
        assert_eq!(store.read(t3, 1), Some(100));
        assert_eq!(store.read(t3, 2), Some(200));
    }

    #[test]
    fn test_committed_txns_tracking() {
        let store = SsiStore::new();

        let t1 = store.begin();
        store.write(t1, 1, 100);
        assert!(store.commit(t1));

        let committed = store.committed_txns();
        assert!(committed.contains(&t1));

        let t2 = store.begin();
        store.write(t2, 2, 200);
        store.abort(t2);

        let committed = store.committed_txns();
        assert!(!committed.contains(&t2));
    }
"#.to_string()
}

fn generate_queue_tests() -> String {
    r#"
    #[test]
    fn test_basic_operations() {
        let queue = MsQueue::new();
        queue.enqueue(1);
        queue.enqueue(2);
        queue.enqueue(3);
        assert_eq!(queue.dequeue(), Some(1));
        assert_eq!(queue.dequeue(), Some(2));
        assert_eq!(queue.dequeue(), Some(3));
        assert_eq!(queue.dequeue(), None);
    }

    #[test]
    fn test_fifo_order() {
        let queue = MsQueue::new();
        for i in 1..=5 {
            queue.enqueue(i);
        }
        for i in 1..=5 {
            assert_eq!(queue.dequeue(), Some(i));
        }
    }
"#.to_string()
}

fn generate_generic_tests(spec: &TlaSpec) -> String {
    // Generate basic tests based on module name
    format!(r#"
    #[test]
    fn test_basic() {{
        // Generic test for {}
        // TODO: Derive specific tests from spec
        assert!(true);
    }}
"#, spec.name)
}

/// Generator errors.
#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    #[error("Spec error: {0}")]
    SpecError(String),

    #[error("Client error: {0}")]
    ClientError(#[from] ClientError),

    #[error("No code block found in response: {0}")]
    NoCodeInResponse(String),

    #[error("Verification error: {0}")]
    VerificationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generator_config_presets() {
        let quick = GeneratorConfig::quick();
        assert_eq!(quick.max_correctness_attempts, 3);

        let thorough = GeneratorConfig::thorough();
        assert_eq!(thorough.max_correctness_attempts, 10);
    }

    #[test]
    fn test_derive_test_code_stack() {
        let spec_content = r#"
---------------------------- MODULE stack ----------------------------
VARIABLES head, pushed, popped

Push(val) == ...
Pop == ...
=============================================================================
"#;
        let spec = vf_core::TlaSpec::parse(spec_content).unwrap();
        let tests = derive_test_code(&spec);
        assert!(tests.contains("test_basic_operations"));
        assert!(tests.contains("test_lifo_order"));
    }

    #[test]
    fn test_derive_test_code_ssi() {
        let spec_content = r#"
---------------------------- MODULE ssi ----------------------------
VARIABLES txns, in_conflict, out_conflict

Commit(txn) == ...
=============================================================================
"#;
        let spec = vf_core::TlaSpec::parse(spec_content).unwrap();
        let tests = derive_test_code(&spec);
        assert!(tests.contains("test_simple_transaction"));
        assert!(tests.contains("test_dangerous_structure_abort"));
    }
}
