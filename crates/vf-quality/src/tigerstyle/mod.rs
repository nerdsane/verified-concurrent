//! TigerStyle code quality checker.
//!
//! Implements the TigerStyle philosophy from tigerstyle.dev.

mod naming;
mod safety;

pub use naming::NamingChecker;
pub use safety::SafetyChecker;

/// A code quality violation.
#[derive(Debug, Clone)]
pub struct Violation {
    /// Rule that was violated
    pub rule: &'static str,
    /// Description of the violation
    pub message: String,
    /// Line number (if available)
    pub line: Option<usize>,
    /// Severity level
    pub severity: Severity,
}

/// Violation severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Must fix before passing
    Error,
    /// Should fix but not blocking
    Warning,
    /// Style suggestion
    Info,
}

impl Violation {
    /// Create a new error violation.
    pub fn error(rule: &'static str, message: impl Into<String>) -> Self {
        Self {
            rule,
            message: message.into(),
            line: None,
            severity: Severity::Error,
        }
    }

    /// Create a new warning violation.
    pub fn warning(rule: &'static str, message: impl Into<String>) -> Self {
        Self {
            rule,
            message: message.into(),
            line: None,
            severity: Severity::Warning,
        }
    }

    /// Add line number.
    pub fn at_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    /// Format for display.
    pub fn format(&self) -> String {
        let severity = match self.severity {
            Severity::Error => "ERROR",
            Severity::Warning => "WARN",
            Severity::Info => "INFO",
        };
        let line = self.line.map_or(String::new(), |l| format!(":{}", l));
        format!("[{}]{} {}: {}", severity, line, self.rule, self.message)
    }
}

/// Result of TigerStyle checking.
#[derive(Debug, Clone)]
pub struct TigerStyleResult {
    /// All violations found
    pub violations: Vec<Violation>,
    /// Whether all required rules pass
    pub passes: bool,
}

impl TigerStyleResult {
    /// Create from violations.
    pub fn from_violations(violations: Vec<Violation>) -> Self {
        let passes = !violations.iter().any(|v| v.severity == Severity::Error);
        Self { violations, passes }
    }

    /// Get error count.
    pub fn errors_count(&self) -> usize {
        self.violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .count()
    }

    /// Get warning count.
    pub fn warnings_count(&self) -> usize {
        self.violations
            .iter()
            .filter(|v| v.severity == Severity::Warning)
            .count()
    }

    /// Format as report.
    pub fn format_report(&self) -> String {
        let mut report = String::new();

        report.push_str("TigerStyle Check Results\n");
        report.push_str("========================\n\n");

        for violation in &self.violations {
            report.push_str(&violation.format());
            report.push('\n');
        }

        report.push_str(&format!(
            "\nSummary: {} errors, {} warnings\n",
            self.errors_count(),
            self.warnings_count()
        ));

        if self.passes {
            report.push_str("Result: PASS\n");
        } else {
            report.push_str("Result: FAIL\n");
        }

        report
    }
}

/// Complete TigerStyle checker.
pub struct TigerStyleChecker {
    safety: SafetyChecker,
    naming: NamingChecker,
}

impl TigerStyleChecker {
    /// Create a new checker.
    pub fn new() -> Self {
        Self {
            safety: SafetyChecker::new(),
            naming: NamingChecker::new(),
        }
    }

    /// Check code against TigerStyle rules.
    pub fn check(&self, code: &str) -> TigerStyleResult {
        let mut violations = Vec::new();

        // Safety checks
        violations.extend(self.safety.check_assertions(code));
        violations.extend(self.safety.check_explicit_limits(code));
        violations.extend(self.safety.check_usize_usage(code));

        // Naming checks
        violations.extend(self.naming.check_big_endian_naming(code));
        violations.extend(self.naming.check_snake_case(code));

        TigerStyleResult::from_violations(violations)
    }
}

impl Default for TigerStyleChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_violation_format() {
        let v = Violation::error("ExplicitLimits", "Missing _MAX constant").at_line(42);
        let formatted = v.format();
        assert!(formatted.contains("ERROR"));
        assert!(formatted.contains(":42"));
        assert!(formatted.contains("ExplicitLimits"));
    }

    #[test]
    fn test_result_passes() {
        let result = TigerStyleResult::from_violations(vec![
            Violation::warning("Test", "test warning"),
        ]);
        assert!(result.passes);

        let result = TigerStyleResult::from_violations(vec![
            Violation::error("Test", "test error"),
        ]);
        assert!(!result.passes);
    }
}
