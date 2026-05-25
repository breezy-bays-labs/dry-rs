//! Binary smoke tests for the `dry4rs` CLI.
//!
//! Exercises the genuine `main()` entry path so the coverage gate
//! covers `crates/dry4rs/src/main.rs`. Uses `CARGO_BIN_EXE_dry4rs`
//! (cargo's per-binary env var) — cargo-llvm-cov instruments
//! subprocess executions so the lines run here count toward the
//! workspace coverage report.
//!
//! The full pipeline + JSON-shape integration tests live in
//! `crates/dry4rs/tests/cli_pipeline.rs`; this file is the
//! adapter-binary smoke layer only.

use std::process::Command;

#[test]
fn binary_runs_with_help_and_exits_success() {
    // `--help` short-circuits clap with exit 0 and prints the
    // about/long_about block; the binary identifier appears in the
    // help header so we use it as a smoke marker.
    let bin = env!("CARGO_BIN_EXE_dry4rs");
    let output = Command::new(bin)
        .arg("--help")
        .output()
        .expect("dry4rs binary should execute");
    assert!(
        output.status.success(),
        "dry4rs --help exited non-zero: status={:?}, stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("dry4rs"),
        "expected --help to mention dry4rs, got: {stdout:?}"
    );
}

#[test]
fn binary_runs_with_version_flag_and_exits_success() {
    // `--version` exit-code-and-format check. clap emits
    // `dry4rs <version>` on stdout.
    let bin = env!("CARGO_BIN_EXE_dry4rs");
    let output = Command::new(bin)
        .arg("--version")
        .output()
        .expect("dry4rs binary should execute");
    assert!(
        output.status.success(),
        "dry4rs --version exited non-zero: status={:?}, stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("dry4rs "), "got: {stdout:?}");
}

#[test]
fn binary_runs_check_subcommand_on_empty_dir_and_exits_success() {
    // `check` mode is exit-code-only — no human-readable stdout. On
    // an empty source tree no matches surface, so `result.passed ==
    // true` and the exit code is 0. This exercises the full pipeline
    // from clap parse → walker → normalize → compare → ExitCode
    // derivation without depending on a specific report format.
    let bin = env!("CARGO_BIN_EXE_dry4rs");
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let output = Command::new(bin)
        .arg("check")
        .arg(tmp.path())
        .output()
        .expect("dry4rs binary should execute");
    assert!(
        output.status.success(),
        "dry4rs check exited non-zero: status={:?}, stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn binary_rejects_invalid_threshold_with_exit_code_two() {
    // clap's `value_parser` rejects out-of-range threshold with
    // `ErrorKind::ValueValidation`, which exits with code 2.
    let bin = env!("CARGO_BIN_EXE_dry4rs");
    let output = Command::new(bin)
        .arg("--threshold")
        .arg("2.0")
        .output()
        .expect("dry4rs binary should execute");
    assert!(
        !output.status.success(),
        "dry4rs --threshold 2.0 should fail: status={:?}",
        output.status
    );
    // Unix returns exit code in low byte of wait status. clap's
    // default argument-error exit code is 2.
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit code 2 (clap arg error), got: {:?}",
        output.status
    );
}
