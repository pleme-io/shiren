//! Test result types and display formatting.
//!
//! Provides [`TestResult`] for individual test outcomes and [`TestSuite`]
//! for collecting results from a full test run.

use std::fmt;
use std::time::Duration;

/// The outcome of a single test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestStatus {
    /// Test passed.
    Pass,
    /// Test failed with an optional message.
    Fail(Option<String>),
    /// Test was skipped or ignored.
    Skip,
    /// Test is currently running.
    Running,
}

impl TestStatus {
    /// Short label for sign/virtual text display.
    #[must_use]
    pub const fn sign(&self) -> &'static str {
        match self {
            Self::Pass => "ok",
            Self::Fail(_) => "FAIL",
            Self::Skip => "skip",
            Self::Running => "...",
        }
    }

    /// Neovim highlight group name for this status.
    #[must_use]
    pub const fn highlight(&self) -> &'static str {
        match self {
            Self::Pass => "ShirenPass",
            Self::Fail(_) => "ShirenFail",
            Self::Skip => "ShirenSkip",
            Self::Running => "ShirenRunning",
        }
    }

    /// Whether this status represents a completed test.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        matches!(self, Self::Pass | Self::Fail(_) | Self::Skip)
    }
}

impl fmt::Display for TestStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass => write!(f, "PASS"),
            Self::Fail(Some(msg)) => write!(f, "FAIL: {msg}"),
            Self::Fail(None) => write!(f, "FAIL"),
            Self::Skip => write!(f, "SKIP"),
            Self::Running => write!(f, "RUNNING"),
        }
    }
}

/// A single test result with metadata.
#[derive(Debug, Clone)]
pub struct TestResult {
    /// Fully-qualified test name (e.g., `tests::it_works`).
    pub name: String,
    /// The file containing this test, if known.
    pub file: Option<String>,
    /// Line number in the source file (0-indexed).
    pub line: Option<usize>,
    /// Test outcome.
    pub status: TestStatus,
    /// How long the test took to run.
    pub duration: Option<Duration>,
    /// Raw output captured from the test.
    pub output: Option<String>,
}

impl TestResult {
    /// Create a new passing test result.
    #[must_use]
    pub fn pass(name: &str) -> Self {
        Self {
            name: name.to_string(),
            file: None,
            line: None,
            status: TestStatus::Pass,
            duration: None,
            output: None,
        }
    }

    /// Create a new failing test result.
    #[must_use]
    pub fn fail(name: &str, message: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            file: None,
            line: None,
            status: TestStatus::Fail(message.map(String::from)),
            duration: None,
            output: None,
        }
    }

    /// Create a new skipped test result.
    #[must_use]
    pub fn skip(name: &str) -> Self {
        Self {
            name: name.to_string(),
            file: None,
            line: None,
            status: TestStatus::Skip,
            duration: None,
            output: None,
        }
    }

    /// Set the file path.
    #[must_use]
    pub fn with_file(mut self, file: &str) -> Self {
        self.file = Some(file.to_string());
        self
    }

    /// Set the line number.
    #[must_use]
    pub fn with_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    /// Set the duration.
    #[must_use]
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Set the raw output.
    #[must_use]
    pub fn with_output(mut self, output: &str) -> Self {
        self.output = Some(output.to_string());
        self
    }
}

impl fmt::Display for TestResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.status.sign(), self.name, self.status)?;
        if let Some(dur) = self.duration {
            write!(f, " ({dur:.2?})")?;
        }
        Ok(())
    }
}

/// A collection of test results from a single run.
#[derive(Debug, Clone, Default)]
pub struct TestSuite {
    /// Individual test results.
    pub results: Vec<TestResult>,
    /// Total wall-clock time for the suite.
    pub duration: Option<Duration>,
}

impl TestSuite {
    /// Create an empty test suite.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a test result.
    pub fn push(&mut self, result: TestResult) {
        self.results.push(result);
    }

    /// Number of tests that passed.
    #[must_use]
    pub fn passed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == TestStatus::Pass)
            .count()
    }

    /// Number of tests that failed.
    #[must_use]
    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.status, TestStatus::Fail(_)))
            .count()
    }

    /// Number of tests that were skipped.
    #[must_use]
    pub fn skipped(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == TestStatus::Skip)
            .count()
    }

    /// Total number of tests.
    #[must_use]
    pub fn total(&self) -> usize {
        self.results.len()
    }

    /// Whether all tests passed (no failures).
    #[must_use]
    pub fn all_passed(&self) -> bool {
        self.failed() == 0
    }

    /// Get only the failed results.
    #[must_use]
    pub fn failures(&self) -> Vec<&TestResult> {
        self.results
            .iter()
            .filter(|r| matches!(r.status, TestStatus::Fail(_)))
            .collect()
    }

    /// Summary line suitable for display in a status bar or notification.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        let passed = self.passed();
        let failed = self.failed();
        let skipped = self.skipped();

        if passed > 0 {
            parts.push(format!("{passed} passed"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        if skipped > 0 {
            parts.push(format!("{skipped} skipped"));
        }

        let summary = parts.join(", ");
        if let Some(dur) = self.duration {
            format!("{summary} ({dur:.2?})")
        } else {
            summary
        }
    }
}

impl fmt::Display for TestSuite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for result in &self.results {
            writeln!(f, "{result}")?;
        }
        writeln!(f, "---")?;
        write!(f, "{}", self.summary())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_signs() {
        assert_eq!(TestStatus::Pass.sign(), "ok");
        assert_eq!(TestStatus::Fail(None).sign(), "FAIL");
        assert_eq!(TestStatus::Skip.sign(), "skip");
        assert_eq!(TestStatus::Running.sign(), "...");
    }

    #[test]
    fn status_highlights() {
        assert_eq!(TestStatus::Pass.highlight(), "ShirenPass");
        assert_eq!(TestStatus::Fail(None).highlight(), "ShirenFail");
        assert_eq!(TestStatus::Skip.highlight(), "ShirenSkip");
        assert_eq!(TestStatus::Running.highlight(), "ShirenRunning");
    }

    #[test]
    fn status_is_finished() {
        assert!(TestStatus::Pass.is_finished());
        assert!(TestStatus::Fail(None).is_finished());
        assert!(TestStatus::Skip.is_finished());
        assert!(!TestStatus::Running.is_finished());
    }

    #[test]
    fn status_display() {
        assert_eq!(TestStatus::Pass.to_string(), "PASS");
        assert_eq!(TestStatus::Fail(None).to_string(), "FAIL");
        assert_eq!(
            TestStatus::Fail(Some("assertion failed".into())).to_string(),
            "FAIL: assertion failed"
        );
        assert_eq!(TestStatus::Skip.to_string(), "SKIP");
        assert_eq!(TestStatus::Running.to_string(), "RUNNING");
    }

    #[test]
    fn test_result_constructors() {
        let pass = TestResult::pass("it_works");
        assert_eq!(pass.name, "it_works");
        assert_eq!(pass.status, TestStatus::Pass);

        let fail = TestResult::fail("it_breaks", Some("oops"));
        assert_eq!(fail.name, "it_breaks");
        assert_eq!(fail.status, TestStatus::Fail(Some("oops".into())));

        let skip = TestResult::skip("ignored");
        assert_eq!(skip.name, "ignored");
        assert_eq!(skip.status, TestStatus::Skip);
    }

    #[test]
    fn test_result_builder_methods() {
        let result = TestResult::pass("my_test")
            .with_file("src/lib.rs")
            .with_line(42)
            .with_duration(Duration::from_millis(150))
            .with_output("all good");

        assert_eq!(result.file.as_deref(), Some("src/lib.rs"));
        assert_eq!(result.line, Some(42));
        assert_eq!(result.duration, Some(Duration::from_millis(150)));
        assert_eq!(result.output.as_deref(), Some("all good"));
    }

    #[test]
    fn test_result_display() {
        let result = TestResult::pass("basic").with_duration(Duration::from_millis(10));
        let s = result.to_string();
        assert!(s.contains("ok"));
        assert!(s.contains("basic"));
        assert!(s.contains("PASS"));
    }

    #[test]
    fn suite_empty() {
        let suite = TestSuite::new();
        assert_eq!(suite.total(), 0);
        assert_eq!(suite.passed(), 0);
        assert_eq!(suite.failed(), 0);
        assert_eq!(suite.skipped(), 0);
        assert!(suite.all_passed());
    }

    #[test]
    fn suite_counts() {
        let mut suite = TestSuite::new();
        suite.push(TestResult::pass("a"));
        suite.push(TestResult::pass("b"));
        suite.push(TestResult::fail("c", Some("boom")));
        suite.push(TestResult::skip("d"));

        assert_eq!(suite.total(), 4);
        assert_eq!(suite.passed(), 2);
        assert_eq!(suite.failed(), 1);
        assert_eq!(suite.skipped(), 1);
        assert!(!suite.all_passed());
    }

    #[test]
    fn suite_failures() {
        let mut suite = TestSuite::new();
        suite.push(TestResult::pass("a"));
        suite.push(TestResult::fail("b", None));
        suite.push(TestResult::fail("c", Some("msg")));

        let failures = suite.failures();
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].name, "b");
        assert_eq!(failures[1].name, "c");
    }

    #[test]
    fn suite_summary() {
        let mut suite = TestSuite::new();
        suite.push(TestResult::pass("a"));
        suite.push(TestResult::fail("b", None));
        suite.push(TestResult::skip("c"));

        let summary = suite.summary();
        assert!(summary.contains("1 passed"));
        assert!(summary.contains("1 failed"));
        assert!(summary.contains("1 skipped"));
    }

    #[test]
    fn suite_summary_with_duration() {
        let mut suite = TestSuite::new();
        suite.push(TestResult::pass("a"));
        suite.duration = Some(Duration::from_secs(2));

        let summary = suite.summary();
        assert!(summary.contains("1 passed"));
        assert!(summary.contains('s'));
    }

    #[test]
    fn suite_all_passed_when_only_passes_and_skips() {
        let mut suite = TestSuite::new();
        suite.push(TestResult::pass("a"));
        suite.push(TestResult::skip("b"));
        assert!(suite.all_passed());
    }

    #[test]
    fn suite_display() {
        let mut suite = TestSuite::new();
        suite.push(TestResult::pass("a"));
        suite.push(TestResult::fail("b", None));

        let display = suite.to_string();
        assert!(display.contains("ok"));
        assert!(display.contains("FAIL"));
        assert!(display.contains("---"));
    }
}
