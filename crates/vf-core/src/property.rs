//! Property verification types with TLA+ traceability.
//!
//! Every property checked by the evaluator cascade maps back to
//! a specific invariant in the TLA+ specification.

use crate::counterexample::Counterexample;

/// Result of checking a single property, with TLA+ traceability.
///
/// # TLA+ Mapping
///
/// Each property corresponds to a specific invariant in the TLA+ spec:
/// - `tla_spec`: The spec file (e.g., "treiber_stack.tla")
/// - `tla_line`: The line number where the invariant is defined
///
/// This traceability ensures the Rust verification matches the formal model.
#[derive(Debug, Clone)]
pub struct PropertyResult {
    /// Human-readable property name (e.g., "NoLostElements")
    pub name: &'static str,

    /// Whether the property holds
    pub holds: bool,

    /// Description of violation if property doesn't hold
    pub violation: Option<String>,

    /// TLA+ spec file this property maps to
    pub tla_spec: &'static str,

    /// Line number in TLA+ spec (for traceability)
    pub tla_line: u32,

    /// Counterexample showing how to reproduce the violation
    pub counterexample: Option<Counterexample>,
}

impl PropertyResult {
    /// Create a passing property result.
    #[must_use]
    pub fn pass(name: &'static str, tla_spec: &'static str, tla_line: u32) -> Self {
        debug_assert!(!name.is_empty(), "Property name must not be empty");
        debug_assert!(!tla_spec.is_empty(), "TLA+ spec must not be empty");
        debug_assert!(tla_line > 0, "TLA+ line must be positive");

        Self {
            name,
            holds: true,
            violation: None,
            tla_spec,
            tla_line,
            counterexample: None,
        }
    }

    /// Create a failing property result.
    #[must_use]
    pub fn fail(
        name: &'static str,
        tla_spec: &'static str,
        tla_line: u32,
        violation: String,
        counterexample: Option<Counterexample>,
    ) -> Self {
        debug_assert!(!name.is_empty(), "Property name must not be empty");
        debug_assert!(!tla_spec.is_empty(), "TLA+ spec must not be empty");
        debug_assert!(tla_line > 0, "TLA+ line must be positive");
        debug_assert!(!violation.is_empty(), "Violation description must not be empty");

        Self {
            name,
            holds: false,
            violation: Some(violation),
            tla_spec,
            tla_line,
            counterexample,
        }
    }

    /// Format as a single-line status for logging.
    #[must_use]
    pub fn format_status(&self) -> String {
        debug_assert!(!self.name.is_empty());

        if self.holds {
            format!("[PASS] {} ({}:{})", self.name, self.tla_spec, self.tla_line)
        } else {
            format!(
                "[FAIL] {} ({}:{}): {}",
                self.name,
                self.tla_spec,
                self.tla_line,
                self.violation.as_deref().unwrap_or("unknown")
            )
        }
    }
}

/// Trait for verifying properties against a state.
///
/// Implementations provide the set of invariants that must hold
/// for a given data structure, all traceable to TLA+ specs.
pub trait PropertyChecker {
    /// Check all properties and return results.
    ///
    /// Returns a vector of `PropertyResult`, one for each invariant.
    /// Even passing properties are included for completeness.
    fn check_all(&self) -> Vec<PropertyResult>;

    /// Verify all properties, returning the first failure.
    ///
    /// This is useful for fail-fast testing where you want to
    /// stop on the first violation.
    fn verify_all(&self) -> Result<(), PropertyResult> {
        for result in self.check_all() {
            if !result.holds {
                return Err(result);
            }
        }
        Ok(())
    }

    /// Check if all properties hold.
    ///
    /// Convenience method for assertions.
    fn all_hold(&self) -> bool {
        self.check_all().iter().all(|r| r.holds)
    }

    /// Get a summary of all property check results.
    fn summary(&self) -> PropertySummary {
        let results = self.check_all();
        let passed = results.iter().filter(|r| r.holds).count() as u64;
        let failed = results.iter().filter(|r| !r.holds).count() as u64;
        let total = results.len() as u64;

        debug_assert!(passed + failed == total);

        PropertySummary {
            passed,
            failed,
            total,
            results,
        }
    }
}

/// Summary of property check results.
#[derive(Debug, Clone)]
pub struct PropertySummary {
    /// Number of properties that passed
    pub passed: u64,
    /// Number of properties that failed
    pub failed: u64,
    /// Total number of properties checked
    pub total: u64,
    /// Individual results
    pub results: Vec<PropertyResult>,
}

impl PropertySummary {
    /// Format as a report string.
    #[must_use]
    pub fn format_report(&self) -> String {
        let mut report = format!(
            "Property Check Summary: {}/{} passed\n",
            self.passed, self.total
        );

        for result in &self.results {
            report.push_str(&result.format_status());
            report.push('\n');
        }

        if let Some(failure) = self.results.iter().find(|r| !r.holds) {
            if let Some(ref ce) = failure.counterexample {
                report.push_str("\nCounterexample:\n");
                report.push_str(&ce.render_diagram());
            }
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_property_result_pass() {
        let result = PropertyResult::pass("NoLostElements", "treiber_stack.tla", 45);
        assert!(result.holds);
        assert!(result.violation.is_none());
        assert!(result.counterexample.is_none());
    }

    #[test]
    fn test_property_result_fail() {
        let result = PropertyResult::fail(
            "NoLostElements",
            "treiber_stack.tla",
            45,
            "Element 42 was lost".to_string(),
            None,
        );
        assert!(!result.holds);
        assert!(result.violation.is_some());
    }

    #[test]
    fn test_format_status() {
        let pass = PropertyResult::pass("Test", "test.tla", 10);
        assert!(pass.format_status().contains("[PASS]"));

        let fail = PropertyResult::fail("Test", "test.tla", 10, "error".to_string(), None);
        assert!(fail.format_status().contains("[FAIL]"));
    }
}
