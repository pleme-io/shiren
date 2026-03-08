//! Jest/Vitest adapter for JavaScript and TypeScript projects.
//!
//! Detects `test(`, `it(`, and `describe(` blocks, runs tests via
//! `npx jest --json`, and parses the structured JSON output.

use crate::adapters::{FoundTest, TestAdapter, TestCommand};
use crate::results::{TestResult, TestSuite};

/// Adapter for JavaScript/TypeScript projects using Jest or Vitest.
pub struct JestAdapter;

impl TestAdapter for JestAdapter {
    fn detect(&self, filetype: &str) -> bool {
        matches!(
            filetype,
            "javascript"
                | "typescript"
                | "javascriptreact"
                | "typescriptreact"
        )
    }

    fn find_tests(&self, content: &str) -> Vec<FoundTest> {
        find_js_tests(content)
    }

    fn build_command(&self, test_name: &str, file: &str) -> TestCommand {
        TestCommand {
            program: "npx".into(),
            args: vec![
                "jest".into(),
                "--json".into(),
                "--no-coverage".into(),
                "--testPathPattern".into(),
                file.to_string(),
                "--testNamePattern".into(),
                test_name.to_string(),
            ],
            cwd: None,
            env: vec![],
        }
    }

    fn build_file_command(&self, file: &str) -> TestCommand {
        TestCommand {
            program: "npx".into(),
            args: vec![
                "jest".into(),
                "--json".into(),
                "--no-coverage".into(),
                "--testPathPattern".into(),
                file.to_string(),
            ],
            cwd: None,
            env: vec![],
        }
    }

    fn build_suite_command(&self) -> TestCommand {
        TestCommand {
            program: "npx".into(),
            args: vec!["jest".into(), "--json".into(), "--no-coverage".into()],
            cwd: None,
            env: vec![],
        }
    }

    fn parse_output(&self, output: &str) -> TestSuite {
        parse_jest_output(output)
    }
}

/// Find test functions in JavaScript/TypeScript source code.
///
/// Recognizes:
/// - `test("name", ...)`  / `test('name', ...)`
/// - `it("name", ...)`    / `it('name', ...)`
/// - `describe("name", ...)` (as a group marker, not a runnable test)
/// - `test.only(...)`, `it.only(...)`, `test.skip(...)`, `it.skip(...)`
fn find_js_tests(content: &str) -> Vec<FoundTest> {
    let mut tests = Vec::new();
    let mut describe_stack: Vec<(String, i32)> = Vec::new();
    let mut brace_depth: i32 = 0;

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Track describe blocks for nesting context.
        if let Some(name) = extract_call_name(trimmed, "describe") {
            describe_stack.push((name, brace_depth));
        }

        // Count braces on this line to track depth.
        for ch in line.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    // Pop any describe blocks that have closed.
                    while let Some((_, start_depth)) = describe_stack.last() {
                        if brace_depth <= *start_depth {
                            describe_stack.pop();
                        } else {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }

        // Skip lines that were describe declarations.
        if extract_call_name(trimmed, "describe").is_some() {
            continue;
        }

        // Look for test/it calls.
        let test_name = extract_call_name(trimmed, "test")
            .or_else(|| extract_call_name(trimmed, "it"))
            .or_else(|| extract_call_name(trimmed, "test.only"))
            .or_else(|| extract_call_name(trimmed, "it.only"))
            .or_else(|| extract_call_name(trimmed, "test.skip"))
            .or_else(|| extract_call_name(trimmed, "it.skip"));

        if let Some(name) = test_name {
            let module = describe_stack.first().map(|(n, _)| n.clone());
            tests.push(FoundTest {
                name,
                line: line_idx,
                module,
            });
        }
    }

    tests
}

/// Extract the string name from a call like `test("name", ...)` or `it('name', ...)`.
///
/// Also handles `test.only("name", ...)` patterns.
fn extract_call_name(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('(')?;
    let rest = rest.trim_start();

    // Extract the quoted string.
    let (quote, rest) = match rest.chars().next()? {
        '\'' => ('\'', &rest[1..]),
        '"' => ('"', &rest[1..]),
        '`' => ('`', &rest[1..]),
        _ => return None,
    };

    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// Parse Jest JSON output into structured results.
///
/// Jest `--json` output contains a top-level object with `testResults`
/// array, each containing `assertionResults`.
fn parse_jest_output(output: &str) -> TestSuite {
    let mut suite = TestSuite::new();

    // Jest may print non-JSON before the actual JSON output.
    // Find the first `{` that starts a JSON object.
    let json_start = output.find('{');
    let Some(json_start) = json_start else {
        // Fall back to line-based parsing if no JSON found.
        return parse_jest_text_output(output);
    };

    let json_str = &output[json_start..];

    let Ok(value) = serde_json::from_str::<serde_json::Value>(json_str) else {
        return parse_jest_text_output(output);
    };

    // Extract overall timing.
    if let Some(start) = value.get("startTime").and_then(|v| v.as_f64()) {
        if let Some(end_time) = value
            .get("testResults")
            .and_then(|r| r.as_array())
            .and_then(|arr| arr.last())
            .and_then(|r| r.get("endTime"))
            .and_then(|v| v.as_f64())
        {
            let ms = end_time - start;
            if ms > 0.0 {
                suite.duration =
                    Some(std::time::Duration::from_millis(ms as u64));
            }
        }
    }

    // Parse test results.
    let Some(test_results) = value
        .get("testResults")
        .and_then(|v| v.as_array())
    else {
        return suite;
    };

    for file_result in test_results {
        let file_name = file_result
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let Some(assertions) = file_result
            .get("assertionResults")
            .and_then(|v| v.as_array())
        else {
            continue;
        };

        for assertion in assertions {
            let title = assertion
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            let status = assertion
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let ancestors: Vec<&str> = assertion
                .get("ancestorTitles")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .collect()
                })
                .unwrap_or_default();

            let full_name = if ancestors.is_empty() {
                title.to_string()
            } else {
                format!("{} > {title}", ancestors.join(" > "))
            };

            let mut result = match status {
                "passed" => TestResult::pass(&full_name),
                "failed" => {
                    let msg = assertion
                        .get("failureMessages")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|v| v.as_str());
                    TestResult::fail(&full_name, msg)
                }
                "pending" | "skipped" | "todo" => TestResult::skip(&full_name),
                _ => continue,
            };

            if !file_name.is_empty() {
                result = result.with_file(file_name);
            }

            suite.push(result);
        }
    }

    suite
}

/// Fallback parser for non-JSON Jest output (e.g., when --json is not used).
fn parse_jest_text_output(output: &str) -> TestSuite {
    let mut suite = TestSuite::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Jest text output format:
        // ✓ test name (Xms)
        // ✕ test name
        // ○ skipped test name
        if let Some(rest) = trimmed
            .strip_prefix("\u{2713} ")
            .or_else(|| trimmed.strip_prefix("PASS "))
        {
            let name = rest
                .rsplit_once(" (")
                .map_or(rest, |(name, _)| name);
            suite.push(TestResult::pass(name.trim()));
        } else if let Some(rest) = trimmed
            .strip_prefix("\u{2715} ")
            .or_else(|| trimmed.strip_prefix("FAIL "))
        {
            suite.push(TestResult::fail(rest.trim(), None));
        } else if let Some(rest) = trimmed
            .strip_prefix("\u{25CB} ")
            .or_else(|| trimmed.strip_prefix("skipped "))
        {
            suite.push(TestResult::skip(rest.trim()));
        }
    }

    suite
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- find_js_tests ---

    #[test]
    fn find_test_calls() {
        let src = r#"
test("adds numbers", () => {
  expect(1 + 1).toBe(2);
});
"#;
        let tests = find_js_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "adds numbers");
        assert_eq!(tests[0].line, 1);
        assert!(tests[0].module.is_none());
    }

    #[test]
    fn find_it_calls() {
        let src = r#"
it("should work", () => {
  expect(true).toBe(true);
});
"#;
        let tests = find_js_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "should work");
    }

    #[test]
    fn find_test_with_single_quotes() {
        let src = "test('single quoted', () => {});\n";
        let tests = find_js_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "single quoted");
    }

    #[test]
    fn find_test_with_backticks() {
        let src = "test(`template literal`, () => {});\n";
        let tests = find_js_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "template literal");
    }

    #[test]
    fn find_tests_in_describe() {
        let src = r#"
describe("Math", () => {
  test("addition", () => {
    expect(1 + 1).toBe(2);
  });

  test("subtraction", () => {
    expect(2 - 1).toBe(1);
  });
});
"#;
        let tests = find_js_tests(src);
        assert_eq!(tests.len(), 2);
        assert_eq!(tests[0].name, "addition");
        assert_eq!(tests[0].module, Some("Math".into()));
        assert_eq!(tests[1].name, "subtraction");
        assert_eq!(tests[1].module, Some("Math".into()));
    }

    #[test]
    fn find_test_only() {
        let src = r#"
test.only("focused test", () => {});
"#;
        let tests = find_js_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "focused test");
    }

    #[test]
    fn find_test_skip() {
        let src = r#"
test.skip("skipped test", () => {});
"#;
        let tests = find_js_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "skipped test");
    }

    #[test]
    fn find_it_only() {
        let src = r#"
it.only("focused it", () => {});
"#;
        let tests = find_js_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "focused it");
    }

    #[test]
    fn find_multiple_mixed() {
        let src = r#"
test("a", () => {});
it("b", () => {});
test.only("c", () => {});
it.skip("d", () => {});
"#;
        let tests = find_js_tests(src);
        assert_eq!(tests.len(), 4);
        assert_eq!(tests[0].name, "a");
        assert_eq!(tests[1].name, "b");
        assert_eq!(tests[2].name, "c");
        assert_eq!(tests[3].name, "d");
    }

    #[test]
    fn no_tests_in_regular_code() {
        let src = r#"
const x = 42;
function helper() { return x; }
console.log(helper());
"#;
        let tests = find_js_tests(src);
        assert!(tests.is_empty());
    }

    #[test]
    fn describe_without_tests() {
        let src = r#"
describe("empty suite", () => {
});
"#;
        let tests = find_js_tests(src);
        assert!(tests.is_empty());
    }

    // --- extract_call_name ---

    #[test]
    fn extract_test_double_quotes() {
        assert_eq!(
            extract_call_name(r#"test("hello", () => {})"#, "test"),
            Some("hello".into())
        );
    }

    #[test]
    fn extract_test_single_quotes() {
        assert_eq!(
            extract_call_name("test('hello', () => {})", "test"),
            Some("hello".into())
        );
    }

    #[test]
    fn extract_test_backticks() {
        assert_eq!(
            extract_call_name("test(`hello`, () => {})", "test"),
            Some("hello".into())
        );
    }

    #[test]
    fn extract_with_spaces() {
        assert_eq!(
            extract_call_name(r#"test( "spaced" , () => {})"#, "test"),
            Some("spaced".into())
        );
    }

    #[test]
    fn extract_not_matching() {
        assert_eq!(extract_call_name("const x = 5;", "test"), None);
    }

    #[test]
    fn extract_describe() {
        assert_eq!(
            extract_call_name(r#"describe("Suite", () => {"#, "describe"),
            Some("Suite".into())
        );
    }

    #[test]
    fn extract_test_only() {
        assert_eq!(
            extract_call_name(r#"test.only("focused", () => {})"#, "test.only"),
            Some("focused".into())
        );
    }

    // --- parse_jest_output (JSON) ---

    #[test]
    fn parse_jest_json_passing() {
        let json = r#"{
  "numTotalTests": 2,
  "numPassedTests": 2,
  "startTime": 1000,
  "testResults": [{
    "name": "/path/to/test.js",
    "endTime": 1500,
    "assertionResults": [
      {"title": "adds numbers", "status": "passed", "ancestorTitles": ["Math"]},
      {"title": "subtracts", "status": "passed", "ancestorTitles": ["Math"]}
    ]
  }]
}"#;
        let suite = parse_jest_output(json);
        assert_eq!(suite.total(), 2);
        assert_eq!(suite.passed(), 2);
        assert!(suite.all_passed());
        assert_eq!(suite.results[0].name, "Math > adds numbers");
        assert_eq!(suite.results[1].name, "Math > subtracts");
    }

    #[test]
    fn parse_jest_json_mixed() {
        let json = r#"{
  "testResults": [{
    "name": "test.js",
    "assertionResults": [
      {"title": "passes", "status": "passed", "ancestorTitles": []},
      {"title": "fails", "status": "failed", "ancestorTitles": [], "failureMessages": ["Expected true to be false"]},
      {"title": "pending", "status": "pending", "ancestorTitles": []}
    ]
  }]
}"#;
        let suite = parse_jest_output(json);
        assert_eq!(suite.total(), 3);
        assert_eq!(suite.passed(), 1);
        assert_eq!(suite.failed(), 1);
        assert_eq!(suite.skipped(), 1);
    }

    #[test]
    fn parse_jest_json_with_failure_message() {
        let json = r#"{
  "testResults": [{
    "name": "test.js",
    "assertionResults": [
      {"title": "broken", "status": "failed", "ancestorTitles": [], "failureMessages": ["assert failed"]}
    ]
  }]
}"#;
        let suite = parse_jest_output(json);
        assert_eq!(suite.failed(), 1);
        let fail = &suite.results[0];
        assert!(matches!(&fail.status, crate::results::TestStatus::Fail(Some(msg)) if msg == "assert failed"));
    }

    #[test]
    fn parse_jest_json_no_ancestors() {
        let json = r#"{
  "testResults": [{
    "name": "test.js",
    "assertionResults": [
      {"title": "standalone", "status": "passed", "ancestorTitles": []}
    ]
  }]
}"#;
        let suite = parse_jest_output(json);
        assert_eq!(suite.results[0].name, "standalone");
    }

    #[test]
    fn parse_jest_json_nested_ancestors() {
        let json = r#"{
  "testResults": [{
    "name": "test.js",
    "assertionResults": [
      {"title": "deep", "status": "passed", "ancestorTitles": ["A", "B", "C"]}
    ]
  }]
}"#;
        let suite = parse_jest_output(json);
        assert_eq!(suite.results[0].name, "A > B > C > deep");
    }

    #[test]
    fn parse_jest_json_with_prefix_noise() {
        let output = "Some warning text\n{\"testResults\": [{\"name\": \"t.js\", \"assertionResults\": [{\"title\": \"ok\", \"status\": \"passed\", \"ancestorTitles\": []}]}]}";
        let suite = parse_jest_output(output);
        assert_eq!(suite.total(), 1);
        assert_eq!(suite.passed(), 1);
    }

    #[test]
    fn parse_jest_non_json_fallback() {
        let output = "not json at all\nno braces here";
        let suite = parse_jest_output(output);
        assert_eq!(suite.total(), 0);
    }

    #[test]
    fn parse_jest_todo_status() {
        let json = r#"{
  "testResults": [{
    "name": "test.js",
    "assertionResults": [
      {"title": "todo item", "status": "todo", "ancestorTitles": []}
    ]
  }]
}"#;
        let suite = parse_jest_output(json);
        assert_eq!(suite.skipped(), 1);
    }

    #[test]
    fn parse_jest_timing() {
        let json = r#"{
  "startTime": 1000,
  "testResults": [{
    "name": "test.js",
    "endTime": 2500,
    "assertionResults": [
      {"title": "timed", "status": "passed", "ancestorTitles": []}
    ]
  }]
}"#;
        let suite = parse_jest_output(json);
        assert!(suite.duration.is_some());
        let dur = suite.duration.unwrap();
        assert_eq!(dur.as_millis(), 1500);
    }

    // --- detect ---

    #[test]
    fn detects_js_filetypes() {
        let adapter = JestAdapter;
        assert!(adapter.detect("javascript"));
        assert!(adapter.detect("typescript"));
        assert!(adapter.detect("javascriptreact"));
        assert!(adapter.detect("typescriptreact"));
        assert!(!adapter.detect("rust"));
        assert!(!adapter.detect("python"));
    }

    // --- build_command ---

    #[test]
    fn build_single_test() {
        let adapter = JestAdapter;
        let cmd = adapter.build_command("adds numbers", "src/math.test.ts");
        assert_eq!(cmd.program, "npx");
        assert!(cmd.args.contains(&"jest".to_string()));
        assert!(cmd.args.contains(&"--testNamePattern".to_string()));
        assert!(cmd.args.contains(&"adds numbers".to_string()));
        assert!(cmd.args.contains(&"src/math.test.ts".to_string()));
    }

    #[test]
    fn build_file() {
        let adapter = JestAdapter;
        let cmd = adapter.build_file_command("src/math.test.ts");
        assert!(cmd.args.contains(&"--testPathPattern".to_string()));
        assert!(cmd.args.contains(&"src/math.test.ts".to_string()));
    }

    #[test]
    fn build_suite() {
        let adapter = JestAdapter;
        let cmd = adapter.build_suite_command();
        assert_eq!(cmd.program, "npx");
        assert!(cmd.args.contains(&"jest".to_string()));
        assert!(cmd.args.contains(&"--json".to_string()));
    }
}
