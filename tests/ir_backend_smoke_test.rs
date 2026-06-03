//! Purpose:
//! Integration smoke tests for the opt-in EIR backend CLI path.
//!
//! Called from:
//! - `cargo test` through Rust's test harness.
//!
//! Key details:
//! - These tests exercise the binary-level `--ir-backend` path instead of only
//!   testing library helpers.

use std::fs;
use std::process::Command;

/// Returns the path to the cargo-built `elephc` binary.
fn elephc_cli_bin() -> String {
    std::env::var("CARGO_BIN_EXE_elephc").unwrap_or_else(|_| {
        let mut path = std::env::current_exe().expect("failed to resolve current test binary");
        path.pop();
        if path.ends_with("deps") {
            path.pop();
        }
        path.join("elephc").to_string_lossy().into_owned()
    })
}

/// Verifies the IR backend compiles, links, and runs a straight-line scalar echo program.
#[test]
fn ir_backend_hello_world() {
    let dir = std::env::temp_dir().join(format!(
        "elephc_ir_backend_hello_{}_{}",
        std::process::id(),
        unique_test_id()
    ));
    fs::create_dir_all(&dir).expect("failed to create IR backend hello directory");
    let php_path = dir.join("main.php");
    fs::write(&php_path, "<?php echo 42;").expect("failed to write IR backend PHP fixture");

    let compile = Command::new(elephc_cli_bin())
        .env("XDG_CACHE_HOME", dir.join("cache-root"))
        .current_dir(&dir)
        .arg("--ir-backend")
        .arg(&php_path)
        .output()
        .expect("failed to run elephc CLI with --ir-backend");
    assert!(
        compile.status.success(),
        "elephc --ir-backend failed: stderr={}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(dir.join("main"))
        .current_dir(&dir)
        .output()
        .expect("failed to run IR backend binary");
    assert!(run.status.success(), "IR backend binary failed");
    assert_eq!(String::from_utf8(run.stdout).unwrap(), "42");

    let _ = fs::remove_dir_all(&dir);
}

/// Returns a coarse unique suffix for temporary test directories.
fn unique_test_id() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos()
}
