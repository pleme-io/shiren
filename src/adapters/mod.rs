//! Test framework adapters.
//!
//! Each adapter knows how to:
//! - Detect whether it applies to a given file type
//! - Find test functions in source code
//! - Build a shell command to run tests
//! - Parse the output into [`TestResult`](crate::results::TestResult)s

pub mod cargo;
pub mod jest;
pub mod pytest;

use crate::results::TestSuite;

/// Describes a test found in source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundTest {
    /// The test function/method name.
    pub name: String,
    /// Line number in the source file (0-indexed).
    pub line: usize,
    /// The module path prefix, if any (e.g., `tests` for `mod tests { ... }`).
    pub module: Option<String>,
}

impl FoundTest {
    /// Fully-qualified name including module prefix.
    #[must_use]
    pub fn full_name(&self) -> String {
        match &self.module {
            Some(module) => format!("{module}::{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// A command to execute for running tests.
#[derive(Debug, Clone)]
pub struct TestCommand {
    /// The program to run (e.g., `"cargo"`, `"npx"`, `"pytest"`).
    pub program: String,
    /// Arguments to pass.
    pub args: Vec<String>,
    /// Working directory (project root).
    pub cwd: Option<String>,
    /// Environment variables to set.
    pub env: Vec<(String, String)>,
}

/// Trait that every test framework adapter must implement.
pub trait TestAdapter {
    /// Check whether this adapter handles the given file type.
    ///
    /// The `filetype` string matches Neovim's `&filetype` (e.g., `"rust"`,
    /// `"python"`, `"javascript"`, `"typescript"`).
    fn detect(&self, filetype: &str) -> bool;

    /// Scan source code and return all test functions/methods found.
    fn find_tests(&self, content: &str) -> Vec<FoundTest>;

    /// Build a command to run a specific test by name in a file.
    fn build_command(&self, test_name: &str, file: &str) -> TestCommand;

    /// Build a command to run all tests in a file.
    fn build_file_command(&self, file: &str) -> TestCommand;

    /// Build a command to run the entire test suite.
    fn build_suite_command(&self) -> TestCommand;

    /// Parse raw test output into structured results.
    fn parse_output(&self, output: &str) -> TestSuite;
}

/// Find the nearest test to a given line number.
///
/// Returns the test whose line number is closest to `cursor_line` without
/// going past it (prefers the test definition above the cursor).
#[must_use]
pub fn nearest_test(tests: &[FoundTest], cursor_line: usize) -> Option<&FoundTest> {
    tests
        .iter()
        .filter(|t| t.line <= cursor_line)
        .max_by_key(|t| t.line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_test_full_name_no_module() {
        let t = FoundTest {
            name: "it_works".into(),
            line: 10,
            module: None,
        };
        assert_eq!(t.full_name(), "it_works");
    }

    #[test]
    fn found_test_full_name_with_module() {
        let t = FoundTest {
            name: "it_works".into(),
            line: 10,
            module: Some("tests".into()),
        };
        assert_eq!(t.full_name(), "tests::it_works");
    }

    #[test]
    fn nearest_test_exact_line() {
        let tests = vec![
            FoundTest {
                name: "a".into(),
                line: 5,
                module: None,
            },
            FoundTest {
                name: "b".into(),
                line: 15,
                module: None,
            },
        ];
        let found = nearest_test(&tests, 15).unwrap();
        assert_eq!(found.name, "b");
    }

    #[test]
    fn nearest_test_between_tests() {
        let tests = vec![
            FoundTest {
                name: "a".into(),
                line: 5,
                module: None,
            },
            FoundTest {
                name: "b".into(),
                line: 15,
                module: None,
            },
        ];
        let found = nearest_test(&tests, 10).unwrap();
        assert_eq!(found.name, "a");
    }

    #[test]
    fn nearest_test_before_all() {
        let tests = vec![FoundTest {
            name: "a".into(),
            line: 10,
            module: None,
        }];
        assert!(nearest_test(&tests, 5).is_none());
    }

    #[test]
    fn nearest_test_empty() {
        let tests: Vec<FoundTest> = vec![];
        assert!(nearest_test(&tests, 10).is_none());
    }
}
