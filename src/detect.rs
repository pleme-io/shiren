//! Auto-detect the test framework from project files.
//!
//! Examines the current working directory for marker files
//! (`Cargo.toml`, `pyproject.toml`, `package.json`, `go.mod`) and
//! returns the appropriate [`Framework`].

use std::path::Path;

/// Supported test frameworks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framework {
    /// Rust — `cargo test`
    Cargo,
    /// Python — `pytest`
    Pytest,
    /// JavaScript/TypeScript — `jest` or `vitest`
    Jest,
    /// Go — `go test`
    GoTest,
}

impl Framework {
    /// Human-readable name for display in the UI.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Cargo => "cargo test",
            Self::Pytest => "pytest",
            Self::Jest => "jest/vitest",
            Self::GoTest => "go test",
        }
    }
}

/// Detect the test framework by walking up from `start` looking for
/// marker files. Returns `None` if no known framework is detected.
#[must_use]
pub fn detect_framework(start: &Path) -> Option<Framework> {
    let dir = if start.is_file() {
        start.parent()?
    } else {
        start
    };

    detect_in_ancestors(dir)
}

/// Walk up the directory tree looking for framework markers.
fn detect_in_ancestors(mut dir: &Path) -> Option<Framework> {
    loop {
        if let Some(fw) = detect_in_dir(dir) {
            return Some(fw);
        }
        dir = dir.parent()?;
    }
}

/// Check a single directory for framework marker files.
///
/// Priority order: Cargo.toml > go.mod > pyproject.toml/setup.py > package.json
fn detect_in_dir(dir: &Path) -> Option<Framework> {
    if dir.join("Cargo.toml").is_file() {
        return Some(Framework::Cargo);
    }
    if dir.join("go.mod").is_file() {
        return Some(Framework::GoTest);
    }
    if dir.join("pyproject.toml").is_file()
        || dir.join("setup.py").is_file()
        || dir.join("setup.cfg").is_file()
    {
        return Some(Framework::Pytest);
    }
    if dir.join("package.json").is_file() {
        return Some(Framework::Jest);
    }
    None
}

/// Detect framework from a file extension alone (fallback when no
/// project marker is found).
#[must_use]
pub fn detect_from_extension(path: &Path) -> Option<Framework> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some(Framework::Cargo),
        "py" => Some(Framework::Pytest),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Some(Framework::Jest),
        "go" => Some(Framework::GoTest),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_project(marker: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(marker), "").unwrap();
        dir
    }

    #[test]
    fn detect_cargo() {
        let dir = temp_project("Cargo.toml");
        assert_eq!(detect_framework(dir.path()), Some(Framework::Cargo));
    }

    #[test]
    fn detect_go() {
        let dir = temp_project("go.mod");
        assert_eq!(detect_framework(dir.path()), Some(Framework::GoTest));
    }

    #[test]
    fn detect_python_pyproject() {
        let dir = temp_project("pyproject.toml");
        assert_eq!(detect_framework(dir.path()), Some(Framework::Pytest));
    }

    #[test]
    fn detect_python_setup_py() {
        let dir = temp_project("setup.py");
        assert_eq!(detect_framework(dir.path()), Some(Framework::Pytest));
    }

    #[test]
    fn detect_python_setup_cfg() {
        let dir = temp_project("setup.cfg");
        assert_eq!(detect_framework(dir.path()), Some(Framework::Pytest));
    }

    #[test]
    fn detect_jest() {
        let dir = temp_project("package.json");
        assert_eq!(detect_framework(dir.path()), Some(Framework::Jest));
    }

    #[test]
    fn detect_none_in_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        // Walk will eventually hit `/` which has no markers either,
        // so this should return None.
        assert_eq!(detect_in_dir(dir.path()), None);
    }

    #[test]
    fn detect_from_nested_file() {
        let dir = temp_project("Cargo.toml");
        let sub = dir.path().join("src");
        fs::create_dir_all(&sub).unwrap();
        let file = sub.join("main.rs");
        fs::write(&file, "").unwrap();
        assert_eq!(detect_framework(&file), Some(Framework::Cargo));
    }

    #[test]
    fn cargo_takes_priority_over_package_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        fs::write(dir.path().join("package.json"), "").unwrap();
        assert_eq!(detect_framework(dir.path()), Some(Framework::Cargo));
    }

    #[test]
    fn go_takes_priority_over_package_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "").unwrap();
        fs::write(dir.path().join("package.json"), "").unwrap();
        assert_eq!(detect_framework(dir.path()), Some(Framework::GoTest));
    }

    // Extension-based detection tests.

    #[test]
    fn ext_rust() {
        assert_eq!(
            detect_from_extension(Path::new("src/main.rs")),
            Some(Framework::Cargo)
        );
    }

    #[test]
    fn ext_python() {
        assert_eq!(
            detect_from_extension(Path::new("tests/test_foo.py")),
            Some(Framework::Pytest)
        );
    }

    #[test]
    fn ext_typescript() {
        assert_eq!(
            detect_from_extension(Path::new("src/App.tsx")),
            Some(Framework::Jest)
        );
    }

    #[test]
    fn ext_go() {
        assert_eq!(
            detect_from_extension(Path::new("main_test.go")),
            Some(Framework::GoTest)
        );
    }

    #[test]
    fn ext_unknown() {
        assert_eq!(detect_from_extension(Path::new("README.md")), None);
    }

    #[test]
    fn ext_no_extension() {
        assert_eq!(detect_from_extension(Path::new("Makefile")), None);
    }

    #[test]
    fn ext_js_variants() {
        for ext in &["js", "jsx", "mjs", "cjs"] {
            let path_str = format!("test.{ext}");
            assert_eq!(
                detect_from_extension(Path::new(&path_str)),
                Some(Framework::Jest),
                "failed for .{ext}"
            );
        }
    }

    #[test]
    fn display_names() {
        assert_eq!(Framework::Cargo.display_name(), "cargo test");
        assert_eq!(Framework::Pytest.display_name(), "pytest");
        assert_eq!(Framework::Jest.display_name(), "jest/vitest");
        assert_eq!(Framework::GoTest.display_name(), "go test");
    }
}
