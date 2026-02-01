//! Generic prompt generation from TLA+ specs.
//!
//! Implements the bitter lesson: derive everything from specs, no implementation hints.
//! The generator states WHAT must be satisfied (invariants), not HOW to implement.

use vf_core::TlaSpec;
use vf_evaluators::CascadeResult;
use vf_perf::ProgressGuarantee;

/// Generic prompt builder - derives everything from spec.
pub struct PromptBuilder;

impl PromptBuilder {
    /// Build generation prompt from spec alone.
    ///
    /// No implementation hints. Just invariants, operations, and constraints.
    pub fn build_generation_prompt(spec: &TlaSpec) -> String {
        let invariants = Self::format_invariants_as_constraints(spec);
        let operations = Self::extract_operations(spec);
        let types = Self::extract_types(spec);

        format!(
            r#"Implement a Rust module that satisfies the following TLA+ specification.

## MODULE: {name}

## CORRECTNESS CONSTRAINTS

Your implementation MUST satisfy these invariants (from the TLA+ spec):

{invariants}

## REQUIRED OPERATIONS

Based on the spec, implement these operations:

{operations}

## TYPES

{types}

## TLA+ SPECIFICATION (for reference)

```tla
{spec_content}
```

## RULES

1. Code must be SELF-CONTAINED (only std crate, no external dependencies except crossbeam-epoch if needed for memory safety)
2. Must be thread-safe (Send + Sync)
3. Use u64 for IDs and counts (not usize)
4. Include at least 2 assertions per public function

## FREEDOM

You decide the implementation strategy. The evaluator cascade only cares that invariants hold.
Any correct solution is valid. Among correct solutions, prefer better performing ones.

Return ONLY the Rust code in a ```rust code block. No explanations outside the code."#,
            name = spec.name,
            invariants = invariants,
            operations = operations,
            types = types,
            spec_content = spec.content,
        )
    }

    /// Build fix prompt from cascade failure.
    ///
    /// Diagnostic, not prescriptive. States what failed, not how to fix.
    pub fn build_fix_prompt(
        spec: &TlaSpec,
        previous_code: &str,
        result: &CascadeResult,
    ) -> String {
        let error_info = Self::format_error_diagnostic(result);
        let invariants = Self::format_invariants_as_constraints(spec);

        format!(
            r#"Your implementation of {name} failed verification.

## REQUIRED INVARIANTS

{invariants}

## PREVIOUS CODE

```rust
{previous_code}
```

## VERIFICATION FAILURE

{error_info}

## TASK

Fix the code to satisfy the invariants. The error above tells you what property was violated.
Your fix must preserve all invariants, not just the one that failed.

Return ONLY the fixed Rust code in a ```rust code block."#,
            name = spec.name,
            invariants = invariants,
            previous_code = previous_code,
            error_info = error_info,
        )
    }

    /// Build performance improvement prompt.
    ///
    /// For correct solutions that could be faster.
    pub fn build_perf_improvement_prompt(
        spec: &TlaSpec,
        current_code: &str,
        current_progress: ProgressGuarantee,
        target_progress: ProgressGuarantee,
    ) -> String {
        let invariants = Self::format_invariants_as_constraints(spec);

        format!(
            r#"Your implementation of {name} is CORRECT but can be improved.

## CURRENT PERFORMANCE

Progress guarantee: {current:?} ({current_desc})

## TARGET PERFORMANCE

Progress guarantee: {target:?} ({target_desc})

## CURRENT CODE

```rust
{code}
```

## REQUIRED INVARIANTS (must still hold)

{invariants}

## TASK

Improve the implementation to achieve {target:?} progress guarantee while preserving correctness.
All invariants must still hold.

Return ONLY the improved Rust code in a ```rust code block."#,
            name = spec.name,
            current = current_progress,
            current_desc = current_progress.description(),
            target = target_progress,
            target_desc = target_progress.description(),
            code = current_code,
            invariants = invariants,
        )
    }

    /// Get system prompt - minimal, no implementation hints.
    pub fn system_prompt() -> &'static str {
        r#"You are a Rust systems programmer.

Your task: implement modules that satisfy TLA+ specifications.

The TLA+ spec defines invariants. Your implementation must preserve them.
The evaluator cascade will verify correctness automatically.

Style:
- Use u64 for IDs and counts
- Include assertions (at least 2 per public function)
- Code must be self-contained

Any implementation that satisfies the invariants is correct.
Among correct implementations, prefer better performing ones:
- WaitFree > LockFree > ObstructionFree > Blocking

Return ONLY Rust code in a ```rust code block."#
    }

    /// Format invariants as mathematical constraints.
    fn format_invariants_as_constraints(spec: &TlaSpec) -> String {
        if spec.invariants.is_empty() {
            return "No invariants specified (see TLA+ spec)".to_string();
        }

        spec.invariants
            .iter()
            .map(|inv| {
                let desc = inv.description.as_deref().unwrap_or("");
                let evaluators = if inv.evaluators.is_empty() {
                    String::new()
                } else {
                    format!(" [checked by: {}]", inv.evaluators.join(", "))
                };
                format!("- **{}**: {}{}", inv.name, desc, evaluators)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Extract operations from spec content.
    fn extract_operations(spec: &TlaSpec) -> String {
        let mut operations = Vec::new();
        let content_lower = spec.content.to_lowercase();

        // Look for common operation patterns in TLA+ specs
        if content_lower.contains("push") {
            operations.push("- `push(value)`: Add element to the structure");
        }
        if content_lower.contains("pop") {
            operations.push("- `pop() -> Option<value>`: Remove and return element");
        }
        if content_lower.contains("begin") {
            operations.push("- `begin() -> TxnId`: Start a new transaction");
        }
        if content_lower.contains("read") && content_lower.contains("txn") {
            operations.push("- `read(txn, key) -> Option<Value>`: Read value in transaction");
        }
        if content_lower.contains("write") && content_lower.contains("txn") {
            operations.push("- `write(txn, key, value) -> bool`: Write value in transaction");
        }
        if content_lower.contains("commit") {
            operations.push("- `commit(txn) -> bool`: Commit transaction (false = aborted)");
        }
        if content_lower.contains("abort") {
            operations.push("- `abort(txn)`: Abort transaction");
        }
        if content_lower.contains("enqueue") {
            operations.push("- `enqueue(value)`: Add element to queue");
        }
        if content_lower.contains("dequeue") {
            operations.push("- `dequeue() -> Option<value>`: Remove and return element from queue");
        }

        if operations.is_empty() {
            // Fall back to listing variables as hints
            format!(
                "Operations should manipulate these state variables:\n{}",
                spec.variables
                    .iter()
                    .map(|v| format!("- `{}`", v))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        } else {
            operations.join("\n")
        }
    }

    /// Extract types from spec constants.
    fn extract_types(spec: &TlaSpec) -> String {
        if spec.constants.is_empty() {
            return "Use u64 for all IDs and values.".to_string();
        }

        let types: Vec<String> = spec
            .constants
            .iter()
            .filter_map(|c| {
                let c_lower = c.to_lowercase();
                if c_lower.contains("element") {
                    Some(format!("- `{}`: Use u64 for element values", c))
                } else if c_lower.contains("thread") {
                    Some(format!("- `{}`: Use u64 for thread IDs (handled by Rust)", c))
                } else if c_lower.contains("key") {
                    Some(format!("- `{}`: Use u64 for keys", c))
                } else if c_lower.contains("value") {
                    Some(format!("- `{}`: Use u64 for values", c))
                } else if c_lower.contains("txn") || c_lower.contains("transaction") {
                    Some(format!("- `{}`: Use u64 for transaction IDs", c))
                } else if c == "NULL" {
                    None // Skip NULL constant
                } else {
                    Some(format!("- `{}`", c))
                }
            })
            .collect();

        if types.is_empty() {
            "Use u64 for all IDs and values.".to_string()
        } else {
            types.join("\n")
        }
    }

    /// Format error as diagnostic (what failed, not how to fix).
    fn format_error_diagnostic(result: &CascadeResult) -> String {
        if let Some(ref failure) = result.first_failure {
            let mut info = format!("**Evaluator**: {} (level {})\n", failure.evaluator,
                match failure.evaluator.as_str() {
                    "rustc" => "0 - compilation",
                    "miri" => "1 - undefined behavior",
                    "loom" => "2 - thread interleavings",
                    "DST" => "3 - fault injection",
                    "stateright" => "4 - model checking",
                    "kani" => "5 - bounded proofs",
                    "verus" => "6 - theorem proving",
                    _ => "unknown",
                });

            if let Some(ref error) = failure.error {
                info.push_str(&format!("\n**Error**: {}\n", error));
            }

            if let Some(ref ce) = failure.counterexample {
                info.push_str(&format!("\n**Counterexample**:\n{}\n", ce.render_diagram()));
            }

            // Include relevant output (limited)
            if !failure.output.is_empty() {
                let output_lines: Vec<&str> = failure.output
                    .lines()
                    .filter(|l| {
                        let l = l.to_lowercase();
                        l.contains("error") || l.contains("failed") ||
                        l.contains("violated") || l.contains("panicked")
                    })
                    .take(20)
                    .collect();
                if !output_lines.is_empty() {
                    info.push_str(&format!("\n**Relevant output**:\n```\n{}\n```\n",
                        output_lines.join("\n")));
                }
            }

            info
        } else {
            "Unknown error".to_string()
        }
    }
}

/// Extract code from a markdown code block.
pub fn extract_code_block(response: &str) -> Option<String> {
    // Look for ```rust ... ``` block
    let rust_start = response.find("```rust")?;
    let code_start = rust_start + 7;

    let content_after = &response[code_start..];
    let actual_start = content_after
        .find(|c: char| !c.is_whitespace() || c == '\n')
        .map(|i| code_start + i)
        .unwrap_or(code_start);

    let code_end = response[actual_start..].find("```")?;

    Some(response[actual_start..actual_start + code_end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_code_block() {
        let response = r#"Here's the implementation:

```rust
fn main() {
    println!("Hello");
}
```

This code prints hello."#;

        let code = extract_code_block(response).unwrap();
        assert!(code.contains("fn main()"));
        assert!(code.contains("println!"));
    }

    #[test]
    fn test_generation_prompt_has_no_implementation_hints() {
        let spec_content = r#"
---------------------------- MODULE test_stack ----------------------------
CONSTANTS Elements
VARIABLES head, pushed, popped

NoLostElements ==
    \A e \in pushed: e \in stack \/ e \in popped

=============================================================================
"#;
        let spec = vf_core::TlaSpec::parse(spec_content).unwrap();
        let prompt = PromptBuilder::build_generation_prompt(&spec);

        // Should NOT contain implementation hints
        assert!(!prompt.contains("crossbeam_epoch"));
        assert!(!prompt.contains("Acquire"));
        assert!(!prompt.contains("Release"));
        assert!(!prompt.contains("CAS"));
        assert!(!prompt.contains("compare_exchange"));

        // Should contain constraints from spec
        assert!(prompt.contains("NoLostElements"));
        assert!(prompt.contains("test_stack"));
    }

    #[test]
    fn test_system_prompt_is_minimal() {
        let system = PromptBuilder::system_prompt();

        // Should NOT contain implementation details
        assert!(!system.contains("epoch"));
        assert!(!system.contains("Acquire"));
        assert!(!system.contains("CAS"));

        // Should mention performance ordering
        assert!(system.contains("WaitFree"));
        assert!(system.contains("LockFree"));
    }
}
