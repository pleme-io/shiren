//! Shiren (試練) — test runner framework for Neovim with language-specific adapters.
//!
//! Part of the blnvim-ng distribution — a Rust-native Neovim plugin suite.
//! Built with [`nvim-oxi`](https://github.com/noib3/nvim-oxi) for zero-cost
//! Neovim API bindings.
//!
//! ## Commands
//!
//! - `:ShirenRun` — run the nearest test to the cursor
//! - `:ShirenFile` — run all tests in the current file
//! - `:ShirenSuite` — run the full test suite
//!
//! ## Supported Frameworks
//!
//! - **Rust** — `cargo test` (detects `#[test]`, `#[tokio::test]`)
//! - **JavaScript/TypeScript** — `npx jest` (detects `test(`, `it(`, `describe(`)
//! - **Python** — `pytest` (detects `def test_*`, `class Test*`)

pub mod adapters;
pub mod detect;
pub mod results;
pub mod runner;

use adapters::cargo::CargoAdapter;
use adapters::jest::JestAdapter;
use adapters::pytest::PytestAdapter;
use adapters::TestAdapter;
use nvim_oxi::api;
use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::api::types::LogLevel;
use nvim_oxi::Dictionary;
use tane::prelude::*;

/// All registered adapters, checked in order.
fn all_adapters() -> Vec<Box<dyn TestAdapter>> {
    vec![
        Box::new(CargoAdapter),
        Box::new(JestAdapter),
        Box::new(PytestAdapter),
    ]
}

/// Find the first adapter that handles the given filetype.
fn adapter_for_filetype(filetype: &str) -> Option<Box<dyn TestAdapter>> {
    all_adapters().into_iter().find(|a| a.detect(filetype))
}

/// Register all Shiren highlights.
fn setup_highlights() -> oxi::Result<()> {
    Highlight::new("ShirenPass")
        .fg("#a6e3a1")
        .bold()
        .apply()
        .map_err(tane_to_oxi)?;
    Highlight::new("ShirenFail")
        .fg("#f38ba8")
        .bold()
        .apply()
        .map_err(tane_to_oxi)?;
    Highlight::new("ShirenSkip")
        .fg("#a6adc8")
        .italic()
        .apply()
        .map_err(tane_to_oxi)?;
    Highlight::new("ShirenRunning")
        .fg("#f9e2af")
        .apply()
        .map_err(tane_to_oxi)?;
    Ok(())
}

/// Convert a tane error to an nvim-oxi error.
fn tane_to_oxi(e: tane::Error) -> oxi::Error {
    match e {
        tane::Error::Oxi(oxi_err) => oxi_err,
        other => oxi::Error::Api(nvim_oxi::api::Error::Other(other.to_string())),
    }
}

#[oxi::plugin]
fn shiren() -> oxi::Result<()> {
    setup_highlights()?;

    // :ShirenRun -- run nearest test
    UserCommand::new("ShirenRun")
        .desc("Run the nearest test to the cursor")
        .bar()
        .register(|_args| {
            run_nearest().map_err(|e| tane::Error::Custom(e.to_string()))?;
            Ok(())
        })
        .map_err(tane_to_oxi)?;

    // :ShirenFile -- run all tests in file
    UserCommand::new("ShirenFile")
        .desc("Run all tests in the current file")
        .bar()
        .register(|_args| {
            run_file().map_err(|e| tane::Error::Custom(e.to_string()))?;
            Ok(())
        })
        .map_err(tane_to_oxi)?;

    // :ShirenSuite -- run full suite
    UserCommand::new("ShirenSuite")
        .desc("Run the full test suite")
        .bar()
        .register(|_args| {
            run_suite().map_err(|e| tane::Error::Custom(e.to_string()))?;
            Ok(())
        })
        .map_err(tane_to_oxi)?;

    Ok(())
}

/// Get the filetype of the current buffer.
fn current_filetype() -> Result<String, String> {
    let buf = api::get_current_buf();
    let opts = OptionOpts::builder().buffer(buf).build();
    api::get_option_value::<String>("filetype", &opts).map_err(|e| e.to_string())
}

/// Get the file path of the current buffer.
fn current_file() -> Result<String, String> {
    let buf = api::get_current_buf();
    buf.get_name()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

/// Get the full content of the current buffer.
fn current_content() -> Result<String, String> {
    let buf = api::get_current_buf();
    let line_count = buf.line_count().map_err(|e| e.to_string())?;
    let lines = buf
        .get_lines(0..line_count, false)
        .map_err(|e| e.to_string())?;
    Ok(lines
        .into_iter()
        .map(|l| l.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Get the 0-indexed cursor line.
fn cursor_line() -> Result<usize, String> {
    let win = api::get_current_win();
    let cursor = win.get_cursor().map_err(|e| e.to_string())?;
    Ok(cursor.0 as usize - 1)
}

/// Run the nearest test to the cursor position.
fn run_nearest() -> Result<(), String> {
    let filetype = current_filetype()?;
    let adapter = adapter_for_filetype(&filetype)
        .ok_or_else(|| format!("shiren: no adapter for filetype '{filetype}'"))?;

    let content = current_content()?;
    let tests = adapter.find_tests(&content);
    let line = cursor_line()?;

    let nearest = adapters::nearest_test(&tests, line)
        .ok_or_else(|| "shiren: no test found near cursor".to_string())?;

    let file = current_file()?;
    let cmd = adapter.build_command(&nearest.full_name(), &file);

    let suite = runner::run_and_parse(adapter.as_ref(), &cmd)
        .map_err(|e| e.to_string())?;

    notify_results(&suite);
    Ok(())
}

/// Run all tests in the current file.
fn run_file() -> Result<(), String> {
    let filetype = current_filetype()?;
    let adapter = adapter_for_filetype(&filetype)
        .ok_or_else(|| format!("shiren: no adapter for filetype '{filetype}'"))?;

    let file = current_file()?;
    let cmd = adapter.build_file_command(&file);

    let suite = runner::run_and_parse(adapter.as_ref(), &cmd)
        .map_err(|e| e.to_string())?;

    notify_results(&suite);
    Ok(())
}

/// Run the full test suite.
fn run_suite() -> Result<(), String> {
    let filetype = current_filetype()?;
    let adapter = adapter_for_filetype(&filetype)
        .ok_or_else(|| format!("shiren: no adapter for filetype '{filetype}'"))?;

    let cmd = adapter.build_suite_command();

    let suite = runner::run_and_parse(adapter.as_ref(), &cmd)
        .map_err(|e| e.to_string())?;

    notify_results(&suite);
    Ok(())
}

/// Display test results as a Neovim notification.
fn notify_results(suite: &results::TestSuite) {
    let msg = suite.summary();
    let level = if suite.all_passed() {
        LogLevel::Info
    } else {
        LogLevel::Error
    };
    let _ = api::notify(&msg, level, &Dictionary::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_adapters_returns_three() {
        assert_eq!(all_adapters().len(), 3);
    }

    #[test]
    fn adapter_for_rust() {
        let adapter = adapter_for_filetype("rust");
        assert!(adapter.is_some());
        assert!(adapter.unwrap().detect("rust"));
    }

    #[test]
    fn adapter_for_python() {
        let adapter = adapter_for_filetype("python");
        assert!(adapter.is_some());
        assert!(adapter.unwrap().detect("python"));
    }

    #[test]
    fn adapter_for_javascript() {
        let adapter = adapter_for_filetype("javascript");
        assert!(adapter.is_some());
        assert!(adapter.unwrap().detect("javascript"));
    }

    #[test]
    fn adapter_for_typescript() {
        let adapter = adapter_for_filetype("typescript");
        assert!(adapter.is_some());
    }

    #[test]
    fn adapter_for_unknown() {
        assert!(adapter_for_filetype("haskell").is_none());
    }
}
