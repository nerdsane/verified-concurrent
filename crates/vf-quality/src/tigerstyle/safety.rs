//! TigerStyle safety rules.
//!
//! Safety rules are REQUIRED - code must pass all of them.

use super::Violation;

/// Safety rule checker.
pub struct SafetyChecker;

impl SafetyChecker {
    /// Create a new checker.
    pub fn new() -> Self {
        Self
    }

    /// Check for 2+ assertions per function.
    ///
    /// Rule: Every non-trivial function should have at least 2 assertions.
    pub fn check_assertions(&self, code: &str) -> Vec<Violation> {
        let mut violations = Vec::new();

        // Parse function definitions and count assertions
        let mut in_function = false;
        let mut function_name = String::new();
        let mut function_start_line = 0;
        let mut assertion_count = 0;
        let mut brace_depth = 0;

        for (line_num, line) in code.lines().enumerate() {
            let trimmed = line.trim();

            // Detect function start
            if (trimmed.starts_with("pub fn ") || trimmed.starts_with("fn "))
                && trimmed.contains('(')
            {
                if in_function && brace_depth == 0 {
                    // End previous function
                    self.check_function_assertions(
                        &function_name,
                        function_start_line,
                        assertion_count,
                        &mut violations,
                    );
                }

                in_function = true;
                function_name = extract_function_name(trimmed);
                function_start_line = line_num + 1;
                assertion_count = 0;
                brace_depth = 0;
            }

            if in_function {
                // Track brace depth
                brace_depth += line.chars().filter(|&c| c == '{').count() as i32;
                brace_depth -= line.chars().filter(|&c| c == '}').count() as i32;

                // Count assertions
                if trimmed.contains("debug_assert!")
                    || trimmed.contains("debug_assert_eq!")
                    || trimmed.contains("debug_assert_ne!")
                    || trimmed.contains("assert!")
                    || trimmed.contains("assert_eq!")
                    || trimmed.contains("assert_ne!")
                {
                    assertion_count += 1;
                }

                // Function ended
                if brace_depth == 0 && line.contains('}') {
                    self.check_function_assertions(
                        &function_name,
                        function_start_line,
                        assertion_count,
                        &mut violations,
                    );
                    in_function = false;
                }
            }
        }

        violations
    }

    fn check_function_assertions(
        &self,
        name: &str,
        line: usize,
        count: usize,
        violations: &mut Vec<Violation>,
    ) {
        // Skip trivial functions (getters, constructors, etc.)
        let trivial_prefixes = ["new", "default", "get_", "is_", "as_", "into_", "from_"];
        let is_trivial = trivial_prefixes.iter().any(|p| name.starts_with(p));

        if !is_trivial && count < 2 {
            violations.push(
                Violation::warning(
                    "Assertions",
                    format!(
                        "Function '{}' has {} assertion(s), recommend 2+",
                        name, count
                    ),
                )
                .at_line(line),
            );
        }
    }

    /// Check for explicit limits with _MAX suffix.
    ///
    /// Rule: Bound all resources with explicit constants.
    pub fn check_explicit_limits(&self, code: &str) -> Vec<Violation> {
        let mut violations = Vec::new();

        // Look for patterns that suggest unbounded resources
        let unbounded_patterns = [
            ("Vec::new()", "Consider using Vec::with_capacity() and a MAX constant"),
            ("VecDeque::new()", "Consider using VecDeque::with_capacity() and a MAX constant"),
            ("HashMap::new()", "Consider using HashMap::with_capacity() and a MAX constant"),
            ("loop {", "Ensure loop has explicit bounds or termination"),
        ];

        for (line_num, line) in code.lines().enumerate() {
            for (pattern, suggestion) in &unbounded_patterns {
                if line.contains(pattern) {
                    violations.push(
                        Violation::warning("ExplicitLimits", *suggestion).at_line(line_num + 1),
                    );
                }
            }
        }

        // Check for MAX constants exist when size/count fields are defined
        let has_size_field = code.contains("size:") || code.contains("count:");
        let has_max_constant = code.contains("_MAX") || code.contains("_max");

        if has_size_field && !has_max_constant {
            violations.push(Violation::warning(
                "ExplicitLimits",
                "Code has size/count fields but no _MAX constants defined",
            ));
        }

        violations
    }

    /// Check for usize usage in data fields.
    ///
    /// Rule: Use u64 for data fields, not usize (platform-dependent).
    pub fn check_usize_usage(&self, code: &str) -> Vec<Violation> {
        let mut violations = Vec::new();

        for (line_num, line) in code.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comments
            if trimmed.starts_with("//") {
                continue;
            }

            // Check struct fields
            if trimmed.contains(": usize") && !trimmed.contains("// allow usize") {
                // Allow usize for indices and lengths that are genuinely platform-specific
                let allowed_names = ["index", "len", "idx", "offset", "capacity"];
                let is_allowed = allowed_names.iter().any(|n| trimmed.contains(n));

                if !is_allowed {
                    violations.push(
                        Violation::warning(
                            "UsizeUsage",
                            "Consider using u64 instead of usize for cross-platform consistency",
                        )
                        .at_line(line_num + 1),
                    );
                }
            }
        }

        violations
    }
}

impl Default for SafetyChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract function name from a function definition line.
fn extract_function_name(line: &str) -> String {
    // "pub fn foo(" or "fn foo("
    let start = if line.contains("pub fn ") {
        line.find("pub fn ").unwrap() + 7
    } else {
        line.find("fn ").unwrap() + 3
    };

    let end = line[start..].find('(').unwrap_or(line.len() - start);
    line[start..start + end].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_function_name() {
        assert_eq!(extract_function_name("pub fn foo("), "foo");
        assert_eq!(extract_function_name("fn bar()"), "bar");
        assert_eq!(extract_function_name("pub fn baz<T>(x: T)"), "baz<T>");
    }

    #[test]
    fn test_check_assertions() {
        let checker = SafetyChecker::new();

        // Function with enough assertions
        let good_code = r#"
fn process(x: u64) -> u64 {
    debug_assert!(x > 0);
    debug_assert!(x < 1000);
    x * 2
}
"#;
        let violations = checker.check_assertions(good_code);
        assert!(violations.is_empty());

        // Function missing assertions
        let bad_code = r#"
fn process(x: u64) -> u64 {
    x * 2
}
"#;
        let violations = checker.check_assertions(bad_code);
        assert!(!violations.is_empty());
    }

    #[test]
    fn test_check_explicit_limits() {
        let checker = SafetyChecker::new();

        let code_without_limits = r#"
struct Foo {
    items: Vec<u64>,
    size: u64,
}

impl Foo {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            size: 0,
        }
    }
}
"#;
        let violations = checker.check_explicit_limits(code_without_limits);
        assert!(!violations.is_empty());
    }

    #[test]
    fn test_check_usize_usage() {
        let checker = SafetyChecker::new();

        let code = r#"
struct Stats {
    total: usize,  // Should be u64
    len: usize,    // OK - genuinely platform-specific
}
"#;
        let violations = checker.check_usize_usage(code);
        // Should warn about 'total' but not 'len'
        assert_eq!(violations.len(), 1);
    }
}
