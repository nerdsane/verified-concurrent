//! TigerStyle naming rules.
//!
//! Naming rules are REQUIRED - code must pass all of them.

use super::Violation;

/// Naming rule checker.
pub struct NamingChecker;

impl NamingChecker {
    /// Create a new checker.
    pub fn new() -> Self {
        Self
    }

    /// Check for big-endian naming (most significant first).
    ///
    /// Rule: Names should read from most significant to least significant.
    /// - GOOD: `segment_size_bytes_max`, `connection_delay_min_ms`
    /// - BAD: `max_segment_size`, `min_connection_delay`
    pub fn check_big_endian_naming(&self, code: &str) -> Vec<Violation> {
        let mut violations = Vec::new();

        // Patterns that indicate little-endian naming
        let bad_prefixes = [
            ("max_", "Use _max suffix instead (e.g., count_max)"),
            ("min_", "Use _min suffix instead (e.g., delay_min)"),
            ("num_", "Use _count suffix instead (e.g., items_count)"),
            ("get_", "Consider removing get_ prefix (e.g., foo() not get_foo())"),
            ("is_empty", "Consider empty() instead of is_empty()"),
        ];

        for (line_num, line) in code.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comments
            if trimmed.starts_with("//") {
                continue;
            }

            for (prefix, suggestion) in &bad_prefixes {
                // Look for function definitions with bad prefixes
                if trimmed.contains(&format!("fn {}", prefix)) {
                    violations.push(
                        Violation::warning("BigEndianNaming", *suggestion).at_line(line_num + 1),
                    );
                }

                // Look for const/let definitions with bad prefixes
                if trimmed.contains(&format!("const {}", prefix.to_uppercase()))
                    || trimmed.contains(&format!("let {}", prefix))
                {
                    violations.push(
                        Violation::warning("BigEndianNaming", *suggestion).at_line(line_num + 1),
                    );
                }
            }
        }

        violations
    }

    /// Check for proper snake_case naming.
    ///
    /// Rule: Use snake_case, don't abbreviate.
    pub fn check_snake_case(&self, code: &str) -> Vec<Violation> {
        let mut violations = Vec::new();

        // Common abbreviations to avoid
        let abbreviations = [
            ("cnt", "count"),
            ("idx", "index"),
            ("ptr", "pointer"),
            ("buf", "buffer"),
            ("len", "length"),
            ("num", "count or number"),
            ("sz", "size"),
            ("val", "value"),
            ("tmp", "temporary or descriptive name"),
            ("ret", "result or descriptive name"),
            ("err", "error"),
            ("msg", "message"),
            ("cfg", "config"),
            ("ctx", "context"),
        ];

        for (line_num, line) in code.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comments
            if trimmed.starts_with("//") {
                continue;
            }

            for (abbr, full) in &abbreviations {
                // Check if the abbreviation is used as a standalone identifier
                // (surrounded by non-alphanumeric characters)
                let patterns = [
                    format!("let {}", abbr),
                    format!("let mut {}", abbr),
                    format!("fn {}", abbr),
                    format!(": {}", abbr),
                    format!("_{}", abbr),
                    format!("{}_", abbr),
                ];

                for pattern in &patterns {
                    // Check pattern exists and isn't part of a longer word (e.g., "context" not "ctx")
                    let longer_word_check = format!("{}}}",  abbr);  // e.g., "cnt}"
                    if trimmed.contains(pattern) && !trimmed.contains(&longer_word_check) {
                        violations.push(
                            Violation::warning(
                                "NoAbbreviations",
                                format!("Consider using '{}' instead of '{}'", full, abbr),
                            )
                            .at_line(line_num + 1),
                        );
                        break;
                    }
                }
            }
        }

        violations
    }

    /// Check for qualifiers at end of name.
    ///
    /// Rule: Append qualifiers like units and bounds to names.
    /// - GOOD: `size_bytes`, `delay_ms`, `timeout_seconds`
    /// - BAD: `byte_size`, `ms_delay`, `seconds_timeout`
    pub fn check_qualifiers(&self, code: &str) -> Vec<Violation> {
        let mut violations = Vec::new();

        // Patterns where qualifiers should be at the end
        let bad_patterns = [
            ("byte_", "_bytes"),
            ("bytes_", "_bytes"),
            ("ms_", "_ms"),
            ("sec_", "_seconds"),
            ("us_", "_us"),
            ("ns_", "_ns"),
        ];

        for (line_num, line) in code.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comments
            if trimmed.starts_with("//") {
                continue;
            }

            for (bad_prefix, good_suffix) in &bad_patterns {
                if trimmed.contains(bad_prefix) {
                    violations.push(
                        Violation::warning(
                            "QualifiersAtEnd",
                            format!("Consider suffix '{}' instead of prefix", good_suffix),
                        )
                        .at_line(line_num + 1),
                    );
                }
            }
        }

        violations
    }
}

impl Default for NamingChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_big_endian_naming() {
        let checker = NamingChecker::new();

        // Bad: prefix style
        let bad_code = r#"
const MAX_SIZE: u64 = 1000;
fn get_value() -> u64 { 0 }
"#;
        let violations = checker.check_big_endian_naming(bad_code);
        assert!(!violations.is_empty());

        // Good: suffix style
        let good_code = r#"
const SIZE_MAX: u64 = 1000;
fn value() -> u64 { 0 }
"#;
        let violations = checker.check_big_endian_naming(good_code);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_check_snake_case() {
        let checker = NamingChecker::new();

        // Bad: abbreviations
        let bad_code = r#"
let cnt = 0;
let buf = Vec::new();
"#;
        let violations = checker.check_snake_case(bad_code);
        assert!(!violations.is_empty());
    }

    #[test]
    fn test_check_qualifiers() {
        let checker = NamingChecker::new();

        // Bad: prefix qualifiers
        let bad_code = r#"
let byte_count = 100;
let ms_delay = 50;
"#;
        let violations = checker.check_qualifiers(bad_code);
        assert!(!violations.is_empty());

        // Good: suffix qualifiers
        let good_code = r#"
let count_bytes = 100;
let delay_ms = 50;
"#;
        let violations = checker.check_qualifiers(good_code);
        assert!(violations.is_empty());
    }
}
