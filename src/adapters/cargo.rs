//! Cargo test adapter for Rust projects.
//!
//! Detects `#[test]` and `#[tokio::test]` attributes, runs `cargo test`,
//! and parses the standard libtest output format.

use crate::adapters::{FoundTest, TestAdapter, TestCommand};
use crate::results::{TestResult, TestSuite};

/// Adapter for Rust projects using `cargo test`.
pub struct CargoAdapter;

impl TestAdapter for CargoAdapter {
    fn detect(&self, filetype: &str) -> bool {
        filetype == "rust"
    }

    fn find_tests(&self, content: &str) -> Vec<FoundTest> {
        find_rust_tests(content)
    }

    fn build_command(&self, test_name: &str, _file: &str) -> TestCommand {
        TestCommand {
            program: "cargo".into(),
            args: vec![
                "test".into(),
                "--".into(),
                test_name.to_string(),
                "--exact".into(),
                "--nocapture".into(),
            ],
            cwd: None,
            env: vec![],
        }
    }

    fn build_file_command(&self, file: &str) -> TestCommand {
        // Extract the module path from the file path for filtering.
        // For `src/foo/bar.rs`, cargo test filters by module name.
        let module = file_to_module(file);
        let mut args = vec!["test".to_string()];
        if let Some(module) = module {
            args.push("--".into());
            args.push(module);
            args.push("--nocapture".into());
        }
        TestCommand {
            program: "cargo".into(),
            args,
            cwd: None,
            env: vec![],
        }
    }

    fn build_suite_command(&self) -> TestCommand {
        TestCommand {
            program: "cargo".into(),
            args: vec!["test".into(), "--".into(), "--nocapture".into()],
            cwd: None,
            env: vec![],
        }
    }

    fn parse_output(&self, output: &str) -> TestSuite {
        parse_cargo_output(output)
    }
}

/// Extract a module filter path from a file path.
///
/// Given `src/adapters/cargo.rs`, returns `"adapters::cargo"`.
/// Given `src/lib.rs` or `src/main.rs`, returns `None` (run all).
fn file_to_module(file: &str) -> Option<String> {
    let path = file.replace('\\', "/");

    // Strip leading `src/` if present.
    let stripped = path.strip_prefix("src/").unwrap_or(&path);

    // Remove `.rs` extension.
    let without_ext = stripped.strip_suffix(".rs")?;

    // Skip lib.rs and main.rs — they represent the crate root.
    if without_ext == "lib" || without_ext == "main" {
        return None;
    }

    // Strip trailing `/mod` for `foo/mod.rs` → `foo`.
    let module = without_ext
        .strip_suffix("/mod")
        .unwrap_or(without_ext);

    // Convert path separators to Rust module separators.
    Some(module.replace('/', "::"))
}

/// Find all test functions in Rust source code.
///
/// Recognizes:
/// - `#[test]`
/// - `#[tokio::test]`
/// - `#[rstest]`
/// - `#[cfg(test)]` module boundaries
fn find_rust_tests(content: &str) -> Vec<FoundTest> {
    let mut tests = Vec::new();
    let mut in_test_module = false;
    let mut current_module: Option<String> = None;
    let mut has_test_attr = false;
    let mut brace_depth: i32 = 0;
    let mut module_start_depth: i32 = 0;

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Track brace depth for module scope detection.
        for ch in line.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    if in_test_module && brace_depth < module_start_depth {
                        in_test_module = false;
                        current_module = None;
                    }
                }
                _ => {}
            }
        }

        // Detect `#[cfg(test)]` on a `mod` block.
        if trimmed == "#[cfg(test)]" {
            // Peek: the module declaration will follow shortly.
            // We set a flag and handle it when we see `mod`.
            has_test_attr = true;
            continue;
        }

        // Detect `mod tests {` or similar.
        if has_test_attr || trimmed.starts_with("mod tests") {
            if let Some(mod_name) = parse_mod_declaration(trimmed) {
                in_test_module = true;
                current_module = Some(mod_name);
                module_start_depth = brace_depth;
                has_test_attr = false;
                continue;
            }
        }

        // Detect test attributes.
        if trimmed == "#[test]"
            || trimmed == "#[tokio::test]"
            || trimmed == "#[rstest]"
            || trimmed.starts_with("#[test_case(")
        {
            has_test_attr = true;
            continue;
        }

        // If we had a test attribute, look for `fn` or `async fn`.
        if has_test_attr {
            if let Some(fn_name) = parse_fn_declaration(trimmed) {
                tests.push(FoundTest {
                    name: fn_name,
                    line: line_idx,
                    module: current_module.clone(),
                });
                has_test_attr = false;
                continue;
            }
            // Attribute might span multiple lines or have other attrs between.
            // Reset only if this line is clearly not part of attrs.
            if !trimmed.starts_with('#') && !trimmed.is_empty() {
                has_test_attr = false;
            }
        }
    }

    tests
}

/// Parse a module declaration like `mod tests {` and return the module name.
fn parse_mod_declaration(line: &str) -> Option<String> {
    let rest = line.strip_prefix("mod ")?;
    // Find the module name (ends at `{`, `;`, or whitespace).
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// Parse a function declaration and extract the name.
///
/// Handles both `fn foo()` and `async fn foo()`.
fn parse_fn_declaration(line: &str) -> Option<String> {
    let rest = if let Some(after_async) = line.strip_prefix("async fn ") {
        after_async
    } else if let Some(after_pub_async) = line.strip_prefix("pub async fn ") {
        after_pub_async
    } else if let Some(after_pub) = line.strip_prefix("pub fn ") {
        after_pub
    } else if let Some(after_fn) = line.strip_prefix("fn ") {
        after_fn
    } else {
        return None;
    };

    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();

    if name.is_empty() { None } else { Some(name) }
}

/// Parse `cargo test` output into structured results.
///
/// Recognizes the standard libtest format:
/// ```text
/// test result::tests::it_works ... ok
/// test result::tests::it_fails ... FAILED
/// test result::tests::ignored ... ignored
/// ```
fn parse_cargo_output(output: &str) -> TestSuite {
    let mut suite = TestSuite::new();

    for line in output.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("test ") {
            if let Some((name, status)) = parse_test_line(rest) {
                suite.push(match status {
                    "ok" => TestResult::pass(&name),
                    "FAILED" => TestResult::fail(&name, None),
                    "ignored" => TestResult::skip(&name),
                    _ => continue,
                });
            }
        }
    }

    // Try to extract overall duration from summary line.
    // Format: `test result: ok. 3 passed; 0 failed; 1 ignored; ...`
    // or timing: `finished in 1.23s`
    for line in output.lines().rev() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("finished in ") {
            if let Some(dur) = parse_duration_str(rest) {
                suite.duration = Some(dur);
                break;
            }
        }
    }

    suite
}

/// Parse a single test result line after the `test ` prefix.
///
/// Input: `"foo::bar::baz ... ok"` -> `Some(("foo::bar::baz", "ok"))`
fn parse_test_line(line: &str) -> Option<(String, &str)> {
    let parts: Vec<&str> = line.rsplitn(2, " ... ").collect();
    if parts.len() == 2 {
        let status = parts[0];
        let name = parts[1].to_string();
        Some((name, status))
    } else {
        None
    }
}

/// Parse a duration string like `"1.23s"` into a `Duration`.
fn parse_duration_str(s: &str) -> Option<std::time::Duration> {
    let s = s.trim().trim_end_matches('s');
    let secs: f64 = s.parse().ok()?;
    Some(std::time::Duration::from_secs_f64(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- find_rust_tests ---

    #[test]
    fn find_simple_test() {
        let src = r#"
#[test]
fn it_works() {
    assert!(true);
}
"#;
        let tests = find_rust_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "it_works");
        assert_eq!(tests[0].line, 2);
        assert!(tests[0].module.is_none());
    }

    #[test]
    fn find_async_test() {
        let src = r#"
#[tokio::test]
async fn async_works() {
    let _ = 42;
}
"#;
        let tests = find_rust_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "async_works");
    }

    #[test]
    fn find_test_in_module() {
        let src = r#"
fn not_a_test() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first() {}

    #[test]
    fn second() {}
}
"#;
        let tests = find_rust_tests(src);
        assert_eq!(tests.len(), 2);
        assert_eq!(tests[0].name, "first");
        assert_eq!(tests[0].module, Some("tests".into()));
        assert_eq!(tests[1].name, "second");
        assert_eq!(tests[1].module, Some("tests".into()));
    }

    #[test]
    fn find_multiple_tests() {
        let src = r#"
#[test]
fn a() {}

#[test]
fn b() {}

#[test]
fn c() {}
"#;
        let tests = find_rust_tests(src);
        assert_eq!(tests.len(), 3);
        assert_eq!(tests[0].name, "a");
        assert_eq!(tests[1].name, "b");
        assert_eq!(tests[2].name, "c");
    }

    #[test]
    fn no_tests_in_regular_code() {
        let src = r#"
fn main() {
    println!("hello");
}

pub fn helper() -> i32 {
    42
}
"#;
        let tests = find_rust_tests(src);
        assert!(tests.is_empty());
    }

    #[test]
    fn rstest_attribute() {
        let src = r#"
#[rstest]
fn parameterized(#[values(1, 2, 3)] x: i32) {
    assert!(x > 0);
}
"#;
        let tests = find_rust_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "parameterized");
    }

    #[test]
    fn pub_fn_with_test_attr() {
        let src = r#"
#[test]
pub fn public_test() {}
"#;
        let tests = find_rust_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "public_test");
    }

    #[test]
    fn pub_async_fn_with_test_attr() {
        let src = r#"
#[tokio::test]
pub async fn public_async_test() {}
"#;
        let tests = find_rust_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "public_async_test");
    }

    #[test]
    fn test_attr_with_extra_attrs_between() {
        let src = r#"
#[test]
#[should_panic]
fn panics() {
    panic!();
}
"#;
        let tests = find_rust_tests(src);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "panics");
    }

    #[test]
    fn full_name_no_module() {
        let t = FoundTest {
            name: "works".into(),
            line: 0,
            module: None,
        };
        assert_eq!(t.full_name(), "works");
    }

    #[test]
    fn full_name_with_module() {
        let t = FoundTest {
            name: "works".into(),
            line: 0,
            module: Some("tests".into()),
        };
        assert_eq!(t.full_name(), "tests::works");
    }

    // --- file_to_module ---

    #[test]
    fn module_from_nested_file() {
        assert_eq!(
            file_to_module("src/adapters/cargo.rs"),
            Some("adapters::cargo".into())
        );
    }

    #[test]
    fn module_from_lib_rs() {
        assert_eq!(file_to_module("src/lib.rs"), None);
    }

    #[test]
    fn module_from_main_rs() {
        assert_eq!(file_to_module("src/main.rs"), None);
    }

    #[test]
    fn module_from_mod_rs() {
        assert_eq!(
            file_to_module("src/adapters/mod.rs"),
            Some("adapters".into())
        );
    }

    #[test]
    fn module_from_top_level_module() {
        assert_eq!(
            file_to_module("src/results.rs"),
            Some("results".into())
        );
    }

    #[test]
    fn module_from_windows_path() {
        assert_eq!(
            file_to_module("src\\adapters\\cargo.rs"),
            Some("adapters::cargo".into())
        );
    }

    // --- parse_cargo_output ---

    #[test]
    fn parse_passing_tests() {
        let output = r"
running 2 tests
test results::tests::status_signs ... ok
test results::tests::suite_empty ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
";
        let suite = parse_cargo_output(output);
        assert_eq!(suite.total(), 2);
        assert_eq!(suite.passed(), 2);
        assert_eq!(suite.failed(), 0);
        assert!(suite.all_passed());
    }

    #[test]
    fn parse_mixed_results() {
        let output = r"
running 3 tests
test a ... ok
test b ... FAILED
test c ... ignored

test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.50s
";
        let suite = parse_cargo_output(output);
        assert_eq!(suite.total(), 3);
        assert_eq!(suite.passed(), 1);
        assert_eq!(suite.failed(), 1);
        assert_eq!(suite.skipped(), 1);
        assert!(!suite.all_passed());
    }

    #[test]
    fn parse_duration_from_output() {
        let output = "test a ... ok\nfinished in 1.23s\n";
        let suite = parse_cargo_output(output);
        assert!(suite.duration.is_some());
        let dur = suite.duration.unwrap();
        assert!((dur.as_secs_f64() - 1.23).abs() < 0.01);
    }

    #[test]
    fn parse_empty_output() {
        let suite = parse_cargo_output("");
        assert_eq!(suite.total(), 0);
        assert!(suite.all_passed());
    }

    #[test]
    fn parse_test_with_module_path() {
        let output = "test adapters::cargo::tests::parse_passing_tests ... ok\n";
        let suite = parse_cargo_output(output);
        assert_eq!(suite.total(), 1);
        assert_eq!(suite.results[0].name, "adapters::cargo::tests::parse_passing_tests");
    }

    // --- parse_fn_declaration ---

    #[test]
    fn parse_fn_simple() {
        assert_eq!(parse_fn_declaration("fn foo() {"), Some("foo".into()));
    }

    #[test]
    fn parse_fn_async() {
        assert_eq!(
            parse_fn_declaration("async fn bar() {"),
            Some("bar".into())
        );
    }

    #[test]
    fn parse_fn_pub() {
        assert_eq!(parse_fn_declaration("pub fn baz() {"), Some("baz".into()));
    }

    #[test]
    fn parse_fn_pub_async() {
        assert_eq!(
            parse_fn_declaration("pub async fn qux() {"),
            Some("qux".into())
        );
    }

    #[test]
    fn parse_fn_not_a_fn() {
        assert_eq!(parse_fn_declaration("let x = 42;"), None);
    }

    #[test]
    fn parse_fn_with_args() {
        assert_eq!(
            parse_fn_declaration("fn with_args(x: i32, y: &str) {"),
            Some("with_args".into())
        );
    }

    // --- parse_mod_declaration ---

    #[test]
    fn parse_mod_simple() {
        assert_eq!(parse_mod_declaration("mod tests {"), Some("tests".into()));
    }

    #[test]
    fn parse_mod_with_semicolon() {
        assert_eq!(parse_mod_declaration("mod foo;"), Some("foo".into()));
    }

    #[test]
    fn parse_mod_not_a_mod() {
        assert_eq!(parse_mod_declaration("fn foo() {}"), None);
    }

    // --- detect ---

    #[test]
    fn detects_rust_filetype() {
        let adapter = CargoAdapter;
        assert!(adapter.detect("rust"));
        assert!(!adapter.detect("python"));
        assert!(!adapter.detect("javascript"));
    }

    // --- build_command ---

    #[test]
    fn build_single_test_command() {
        let adapter = CargoAdapter;
        let cmd = adapter.build_command("tests::it_works", "src/lib.rs");
        assert_eq!(cmd.program, "cargo");
        assert!(cmd.args.contains(&"test".to_string()));
        assert!(cmd.args.contains(&"tests::it_works".to_string()));
        assert!(cmd.args.contains(&"--exact".to_string()));
    }

    #[test]
    fn build_file_command_for_module() {
        let adapter = CargoAdapter;
        let cmd = adapter.build_file_command("src/adapters/cargo.rs");
        assert_eq!(cmd.program, "cargo");
        assert!(cmd.args.contains(&"adapters::cargo".to_string()));
    }

    #[test]
    fn build_file_command_for_lib() {
        let adapter = CargoAdapter;
        let cmd = adapter.build_file_command("src/lib.rs");
        assert_eq!(cmd.program, "cargo");
        // Should not have a module filter for lib.rs.
        assert!(!cmd.args.iter().any(|a| a.contains("::")));
    }

    #[test]
    fn build_suite_command() {
        let adapter = CargoAdapter;
        let cmd = adapter.build_suite_command();
        assert_eq!(cmd.program, "cargo");
        assert!(cmd.args.contains(&"test".to_string()));
    }

    // --- parse_duration_str ---

    #[test]
    fn parse_duration_seconds() {
        let dur = parse_duration_str("1.23s").unwrap();
        assert!((dur.as_secs_f64() - 1.23).abs() < 0.001);
    }

    #[test]
    fn parse_duration_sub_second() {
        let dur = parse_duration_str("0.05s").unwrap();
        assert!((dur.as_secs_f64() - 0.05).abs() < 0.001);
    }

    #[test]
    fn parse_duration_invalid() {
        assert!(parse_duration_str("abc").is_none());
    }

    // --- parse_test_line ---

    #[test]
    fn parse_test_line_ok() {
        let (name, status) = parse_test_line("foo::bar ... ok").unwrap();
        assert_eq!(name, "foo::bar");
        assert_eq!(status, "ok");
    }

    #[test]
    fn parse_test_line_failed() {
        let (name, status) = parse_test_line("my_test ... FAILED").unwrap();
        assert_eq!(name, "my_test");
        assert_eq!(status, "FAILED");
    }

    #[test]
    fn parse_test_line_ignored() {
        let (name, status) = parse_test_line("skipped ... ignored").unwrap();
        assert_eq!(name, "skipped");
        assert_eq!(status, "ignored");
    }

    #[test]
    fn parse_test_line_no_separator() {
        assert!(parse_test_line("no separator here").is_none());
    }
}
