//! Pytest adapter for Python projects.
//!
//! Detects `def test_*` functions and `class Test*` classes, runs `pytest`,
//! and parses the standard pytest output format.

use crate::adapters::{FoundTest, TestAdapter, TestCommand};
use crate::results::{TestResult, TestSuite};

/// Adapter for Python projects using pytest.
pub struct PytestAdapter;

impl TestAdapter for PytestAdapter {
    fn detect(&self, filetype: &str) -> bool {
        filetype == "python"
    }

    fn find_tests(&self, content: &str) -> Vec<FoundTest> {
        find_python_tests(content)
    }

    fn build_command(&self, test_name: &str, file: &str) -> TestCommand {
        // pytest selector: file.py::TestClass::test_method or file.py::test_func
        TestCommand {
            program: "pytest".into(),
            args: vec![
                "-v".into(),
                "--tb=short".into(),
                format!("{file}::{test_name}"),
            ],
            cwd: None,
            env: vec![],
        }
    }

    fn build_file_command(&self, file: &str) -> TestCommand {
        TestCommand {
            program: "pytest".into(),
            args: vec!["-v".into(), "--tb=short".into(), file.to_string()],
            cwd: None,
            env: vec![],
        }
    }

    fn build_suite_command(&self) -> TestCommand {
        TestCommand {
            program: "pytest".into(),
            args: vec!["-v".into(), "--tb=short".into()],
            cwd: None,
            env: vec![],
        }
    }

    fn parse_output(&self, output: &str) -> TestSuite {
        parse_pytest_output(output)
    }
}

/// Find test functions and methods in Python source code.
///
/// Recognizes:
/// - `def test_*(...):` — standalone test functions
/// - `class Test*:` — test classes
/// - `def test_*(self, ...):` — test methods inside classes
/// - `async def test_*(...):` — async test functions
fn find_python_tests(content: &str) -> Vec<FoundTest> {
    let mut tests = Vec::new();
    let mut current_class: Option<String> = None;
    let mut class_indent: usize = 0;

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        // Detect test classes.
        if let Some(class_name) = parse_test_class(trimmed) {
            current_class = Some(class_name);
            class_indent = indent;
            continue;
        }

        // Reset class context when indentation returns to class level or less.
        if current_class.is_some() && !trimmed.is_empty() && indent <= class_indent {
            // If this line is at the class indent level and not a class, we've left.
            if !trimmed.starts_with("class ") {
                current_class = None;
            }
        }

        // Detect test functions/methods.
        if let Some(fn_name) = parse_test_function(trimmed) {
            tests.push(FoundTest {
                name: fn_name,
                line: line_idx,
                module: current_class.clone(),
            });
        }
    }

    tests
}

/// Parse a test class declaration like `class TestFoo:` or `class TestFoo(Base):`.
fn parse_test_class(line: &str) -> Option<String> {
    let rest = line.strip_prefix("class ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();

    if name.starts_with("Test") && name.len() > 4 {
        Some(name)
    } else {
        None
    }
}

/// Parse a test function declaration like `def test_foo(...):`
/// or `async def test_foo(...):`.
fn parse_test_function(line: &str) -> Option<String> {
    let rest = if let Some(r) = line.strip_prefix("def ") {
        r
    } else if let Some(r) = line.strip_prefix("async def ") {
        r
    } else {
        return None;
    };

    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();

    if name.starts_with("test_") {
        Some(name)
    } else {
        None
    }
}

/// Parse pytest verbose output into structured results.
///
/// Pytest `-v` format:
/// ```text
/// tests/test_math.py::test_add PASSED
/// tests/test_math.py::TestCalc::test_sub FAILED
/// tests/test_math.py::test_skip SKIPPED
/// ```
fn parse_pytest_output(output: &str) -> TestSuite {
    let mut suite = TestSuite::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Look for lines with PASSED/FAILED/SKIPPED/ERROR at the end.
        if let Some(result) = parse_pytest_result_line(trimmed) {
            suite.push(result);
        }
    }

    // Parse summary line for duration.
    // Format: `===== 3 passed, 1 failed in 1.23s =====`
    for line in output.lines().rev() {
        let trimmed = line.trim();
        if let Some(dur) = parse_pytest_summary_duration(trimmed) {
            suite.duration = Some(dur);
            break;
        }
    }

    suite
}

/// Parse a single pytest result line.
///
/// Input: `"tests/test_math.py::test_add PASSED"` -> `TestResult::pass("test_add")`
fn parse_pytest_result_line(line: &str) -> Option<TestResult> {
    // Status markers and their corresponding constructors.
    let (name_part, status) = if let Some(name) = line.strip_suffix(" PASSED") {
        (name, "passed")
    } else if let Some(name) = line.strip_suffix(" FAILED") {
        (name, "failed")
    } else if let Some(name) = line.strip_suffix(" SKIPPED") {
        (name, "skipped")
    } else if let Some(name) = line.strip_suffix(" ERROR") {
        (name, "error")
    } else if let Some(name) = line.strip_suffix(" XFAIL") {
        (name, "xfail")
    } else if let Some(name) = line.strip_suffix(" XPASS") {
        (name, "xpass")
    } else {
        return None;
    };

    // Extract just the test name from the full path.
    // `tests/test_math.py::TestCalc::test_add` -> `TestCalc::test_add`
    let test_name = name_part
        .rsplit_once("::")
        .map_or(name_part, |(_, name)| name);

    // Use the full path as the test name for better identification.
    let full_name = if name_part.contains("::") {
        // Extract everything after the file path.
        name_part
            .split_once("::")
            .map_or(name_part.to_string(), |(_, rest)| rest.to_string())
    } else {
        test_name.to_string()
    };

    let mut result = match status {
        "passed" | "xfail" => TestResult::pass(&full_name),
        "failed" | "error" | "xpass" => TestResult::fail(&full_name, None),
        "skipped" => TestResult::skip(&full_name),
        _ => return None,
    };

    // Try to extract the file from the path.
    if let Some((file, _)) = name_part.split_once("::") {
        result = result.with_file(file.trim());
    }

    Some(result)
}

/// Parse duration from pytest summary line.
///
/// Input: `"===== 3 passed in 1.23s ======"` -> `Duration(1.23s)`
fn parse_pytest_summary_duration(line: &str) -> Option<std::time::Duration> {
    // Strip leading/trailing `=` and spaces.
    let stripped = line.trim_matches(|c: char| c == '=' || c.is_whitespace());

    // Find "in X.XXs" pattern.
    let in_idx = stripped.rfind(" in ")?;
    let after_in = &stripped[in_idx + 4..];
    let secs_str = after_in.trim().trim_end_matches('s');
    let secs: f64 = secs_str.parse().ok()?;
    Some(std::time::Duration::from_secs_f64(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- find_python_tests ---

    #[test]
    fn find_simple_test_function() {
        let src = r#"
def test_addition():
    assert 1 + 1 == 2
"#;
        let tests = find_python_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "test_addition");
        assert_eq!(tests[0].line, 1);
        assert!(tests[0].module.is_none());
    }

    #[test]
    fn find_async_test_function() {
        let src = r#"
async def test_async_op():
    result = await some_call()
    assert result
"#;
        let tests = find_python_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "test_async_op");
    }

    #[test]
    fn find_test_in_class() {
        let src = r#"
class TestMath:
    def test_add(self):
        assert 1 + 1 == 2

    def test_sub(self):
        assert 2 - 1 == 1
"#;
        let tests = find_python_tests(src);
        assert_eq!(tests.len(), 2);
        assert_eq!(tests[0].name, "test_add");
        assert_eq!(tests[0].module, Some("TestMath".into()));
        assert_eq!(tests[1].name, "test_sub");
        assert_eq!(tests[1].module, Some("TestMath".into()));
    }

    #[test]
    fn find_multiple_standalone_tests() {
        let src = r#"
def test_a():
    pass

def test_b():
    pass

def test_c():
    pass
"#;
        let tests = find_python_tests(src);
        assert_eq!(tests.len(), 3);
        assert_eq!(tests[0].name, "test_a");
        assert_eq!(tests[1].name, "test_b");
        assert_eq!(tests[2].name, "test_c");
    }

    #[test]
    fn skip_non_test_functions() {
        let src = r#"
def helper():
    return 42

def setup():
    pass

def test_real():
    assert helper() == 42
"#;
        let tests = find_python_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "test_real");
    }

    #[test]
    fn skip_non_test_classes() {
        let src = r#"
class Helper:
    def do_thing(self):
        pass

class TestSuite:
    def test_thing(self):
        pass
"#;
        let tests = find_python_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "test_thing");
        assert_eq!(tests[0].module, Some("TestSuite".into()));
    }

    #[test]
    fn no_tests_in_regular_code() {
        let src = r#"
def main():
    print("hello")

class Config:
    def __init__(self):
        self.value = 42
"#;
        let tests = find_python_tests(src);
        assert!(tests.is_empty());
    }

    #[test]
    fn test_function_with_args() {
        let src = r#"
def test_with_fixture(db, client):
    response = client.get("/")
    assert response.status_code == 200
"#;
        let tests = find_python_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "test_with_fixture");
    }

    #[test]
    fn class_name_must_start_with_test() {
        let src = r#"
class Test:
    def test_x(self):
        pass
"#;
        // "Test" alone is too short (must be > 4 chars, i.e., "Test" + something)
        let tests = find_python_tests(src);
        assert_eq!(tests.len(), 1);
        // The test function is found but no class context since "Test" is rejected.
        assert!(tests[0].module.is_none());
    }

    // --- parse_test_class ---

    #[test]
    fn parse_class_simple() {
        assert_eq!(parse_test_class("class TestFoo:"), Some("TestFoo".into()));
    }

    #[test]
    fn parse_class_with_base() {
        assert_eq!(
            parse_test_class("class TestBar(BaseTest):"),
            Some("TestBar".into())
        );
    }

    #[test]
    fn parse_class_not_test() {
        assert_eq!(parse_test_class("class Helper:"), None);
    }

    #[test]
    fn parse_class_too_short() {
        assert_eq!(parse_test_class("class Test:"), None);
    }

    #[test]
    fn parse_class_not_a_class() {
        assert_eq!(parse_test_class("def foo():"), None);
    }

    // --- parse_test_function ---

    #[test]
    fn parse_fn_test() {
        assert_eq!(
            parse_test_function("def test_foo():"),
            Some("test_foo".into())
        );
    }

    #[test]
    fn parse_fn_async_test() {
        assert_eq!(
            parse_test_function("async def test_bar():"),
            Some("test_bar".into())
        );
    }

    #[test]
    fn parse_fn_non_test() {
        assert_eq!(parse_test_function("def helper():"), None);
    }

    #[test]
    fn parse_fn_with_self() {
        assert_eq!(
            parse_test_function("def test_method(self):"),
            Some("test_method".into())
        );
    }

    #[test]
    fn parse_fn_not_a_function() {
        assert_eq!(parse_test_function("class Foo:"), None);
    }

    // --- parse_pytest_output ---

    #[test]
    fn parse_all_passing() {
        let output = r"
tests/test_math.py::test_add PASSED
tests/test_math.py::test_sub PASSED
========== 2 passed in 0.12s ==========
";
        let suite = parse_pytest_output(output);
        assert_eq!(suite.total(), 2);
        assert_eq!(suite.passed(), 2);
        assert!(suite.all_passed());
    }

    #[test]
    fn parse_mixed_results() {
        let output = r"
tests/test_math.py::test_add PASSED
tests/test_math.py::test_div FAILED
tests/test_math.py::test_skip SKIPPED
========== 1 passed, 1 failed, 1 skipped in 0.50s ==========
";
        let suite = parse_pytest_output(output);
        assert_eq!(suite.total(), 3);
        assert_eq!(suite.passed(), 1);
        assert_eq!(suite.failed(), 1);
        assert_eq!(suite.skipped(), 1);
    }

    #[test]
    fn parse_with_class_tests() {
        let output = r"
tests/test_calc.py::TestCalc::test_add PASSED
tests/test_calc.py::TestCalc::test_sub FAILED
========== 1 passed, 1 failed in 0.30s ==========
";
        let suite = parse_pytest_output(output);
        assert_eq!(suite.total(), 2);
        assert_eq!(suite.results[0].name, "TestCalc::test_add");
        assert_eq!(suite.results[1].name, "TestCalc::test_sub");
    }

    #[test]
    fn parse_xfail_and_xpass() {
        let output = "test_a XFAIL\ntest_b XPASS\n";
        let suite = parse_pytest_output(output);
        assert_eq!(suite.total(), 2);
        assert_eq!(suite.passed(), 1); // xfail = expected
        assert_eq!(suite.failed(), 1); // xpass = unexpected
    }

    #[test]
    fn parse_error_status() {
        let output = "tests/test_broken.py::test_setup ERROR\n";
        let suite = parse_pytest_output(output);
        assert_eq!(suite.total(), 1);
        assert_eq!(suite.failed(), 1);
    }

    #[test]
    fn parse_empty_output() {
        let suite = parse_pytest_output("");
        assert_eq!(suite.total(), 0);
    }

    #[test]
    fn parse_duration_from_summary() {
        let output = "========== 3 passed in 1.23s ==========\n";
        let suite = parse_pytest_output(output);
        assert!(suite.duration.is_some());
        let dur = suite.duration.unwrap();
        assert!((dur.as_secs_f64() - 1.23).abs() < 0.01);
    }

    #[test]
    fn parse_duration_complex_summary() {
        let output = "===== 1 passed, 2 failed, 1 skipped in 5.67s =====\n";
        let dur = parse_pytest_summary_duration(output.trim()).unwrap();
        assert!((dur.as_secs_f64() - 5.67).abs() < 0.01);
    }

    #[test]
    fn parse_file_extraction() {
        let output = "tests/test_math.py::test_add PASSED\n";
        let suite = parse_pytest_output(output);
        assert_eq!(suite.results[0].file.as_deref(), Some("tests/test_math.py"));
    }

    // --- parse_pytest_result_line ---

    #[test]
    fn result_line_passed() {
        let result =
            parse_pytest_result_line("tests/test_foo.py::test_bar PASSED").unwrap();
        assert_eq!(result.name, "test_bar");
        assert_eq!(result.status, crate::results::TestStatus::Pass);
    }

    #[test]
    fn result_line_failed() {
        let result =
            parse_pytest_result_line("tests/test_foo.py::test_baz FAILED").unwrap();
        assert_eq!(result.name, "test_baz");
        assert!(matches!(result.status, crate::results::TestStatus::Fail(_)));
    }

    #[test]
    fn result_line_skipped() {
        let result =
            parse_pytest_result_line("tests/test_foo.py::test_skip SKIPPED").unwrap();
        assert_eq!(result.name, "test_skip");
        assert_eq!(result.status, crate::results::TestStatus::Skip);
    }

    #[test]
    fn result_line_no_match() {
        assert!(parse_pytest_result_line("some random text").is_none());
    }

    // --- detect ---

    #[test]
    fn detects_python_filetype() {
        let adapter = PytestAdapter;
        assert!(adapter.detect("python"));
        assert!(!adapter.detect("rust"));
        assert!(!adapter.detect("javascript"));
    }

    // --- build_command ---

    #[test]
    fn build_single_test() {
        let adapter = PytestAdapter;
        let cmd = adapter.build_command("test_add", "tests/test_math.py");
        assert_eq!(cmd.program, "pytest");
        assert!(cmd.args.contains(&"tests/test_math.py::test_add".to_string()));
    }

    #[test]
    fn build_file() {
        let adapter = PytestAdapter;
        let cmd = adapter.build_file_command("tests/test_math.py");
        assert!(cmd.args.contains(&"tests/test_math.py".to_string()));
    }

    #[test]
    fn build_suite() {
        let adapter = PytestAdapter;
        let cmd = adapter.build_suite_command();
        assert_eq!(cmd.program, "pytest");
        assert!(cmd.args.contains(&"-v".to_string()));
    }
}
