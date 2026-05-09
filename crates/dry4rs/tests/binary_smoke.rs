//! Bootstrap smoke test for the `dry4rs` binary.
//!
//! Exercises the genuine `main()` entry path so the coverage gate
//! covers `crates/dry4rs/src/main.rs`. Uses `CARGO_BIN_EXE_dry4rs`
//! (cargo's per-binary env var) — cargo-llvm-cov instruments
//! subprocess executions so the lines run here count toward the
//! workspace coverage report.
//!
//! The real CLI integration tests land with PR 8.

use std::process::Command;

#[test]
fn binary_runs_and_exits_success() {
    let bin = env!("CARGO_BIN_EXE_dry4rs");
    let output = Command::new(bin)
        .output()
        .expect("dry4rs binary should execute");
    assert!(
        output.status.success(),
        "dry4rs exited non-zero: status={:?}, stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("dry4rs"),
        "expected stdout to mention dry4rs, got: {stdout:?}"
    );
}
