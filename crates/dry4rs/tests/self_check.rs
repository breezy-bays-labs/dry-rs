//! Self-referential test — dry4rs analyzes its own source as the final
//! v0.1 walking-skeleton gate.
//!
//! Per `ops/decisions/dry-rs/adr-hexagonal-layout.md` ("Self-referential
//! test") and the AGENTS.md `Working rules` list, the analyzer must be
//! validated by running it against its own production code. This file
//! spawns the `dry4rs` binary via the `CARGO_BIN_EXE_dry4rs` entry
//! point (cargo's per-binary env var that cargo-llvm-cov instruments),
//! parses the v0.1 JSON wire envelope, and asserts that a stable subset
//! of `result.summary` matches a committed insta snapshot.
//!
//! ## Why a snapshot rather than a strict allowlist
//!
//! The `[[allowed_match]]` allowlist UX in `.dry-rs-ignore.toml` is a
//! v0.2 deliverable per the roadmap. The wire envelope at v0.1 does NOT
//! expose per-match fingerprints — `Match.forms`/`score`/`tier` plus
//! three reserved score slots — so a fingerprint-keyed filter cannot
//! ride the v0.1 wire. v0.1 instead snapshots a low-resolution summary
//! (`total_forms` + `matches_count` + `by_tier`) so a regression in
//! the comparison engine or normalizer fails CI predictably.
//!
//! **The snapshot is intentionally fragile by design.** Source edits
//! that change form counts WILL fail this test; that's the gate, not a
//! bug. The remediation when a change is intentional is to run
//! `cargo insta review` and accept the new baseline AS PART OF the PR
//! introducing the change — never as a follow-up. The PR that touches
//! production code also moves the baseline; reviewers see both diffs in
//! the same review.
//!
//! ## Why a JSON projection, not the full envelope
//!
//! Full-envelope snapshots are dominated by per-match `forms[].file` /
//! `span.start` / `span.end` data that flutters on every source edit
//! (a one-line insertion shifts every span below it by one). The
//! projection captures only the load-bearing aggregates — counts that
//! reflect the analyzer's *behavior*, not the source layout — so a
//! refactor that preserves duplication structure (moves lines around)
//! does NOT churn the snapshot, while a regression that changes how
//! the comparison engine routes findings does.
//!
//! ## --no-fail
//!
//! Running `dry4rs` against its own crates surfaces real matches today
//! (43 at PR 9 time: 5 auto_refactor + 38 review_first). The binary's
//! default exit code reflects the gate verdict (`result.passed`); we
//! pass `--no-fail` to override the exit code so the test asserts on
//! the JSON shape regardless of whether the in-tree matches pass the
//! threshold.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Resolve the path to `crates/` from the integration-test's
/// `CARGO_MANIFEST_DIR` (which is `<repo>/crates/dry4rs/`).
///
/// Using `CARGO_MANIFEST_DIR` makes the test invariant to the cargo
/// invocation's working directory — `cargo test`, `cargo nextest run
/// -p dry4rs`, and `cargo nextest run --workspace` all set this env
/// var to the per-crate manifest dir before spawning the test binary.
fn workspace_crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("crates dir must resolve")
}

/// Spawn the `dry4rs` binary with the supplied args from the
/// repository root, capture stdout/stderr/status. The current working
/// directory is set to the repo root so emitted `FormRef.file` paths
/// are relative to the workspace, matching what a user sees when they
/// invoke dry4rs locally from the repo root.
fn run_dry4rs(args: &[&str]) -> (std::process::ExitStatus, String, String) {
    let bin = env!("CARGO_BIN_EXE_dry4rs");
    // `CARGO_MANIFEST_DIR` = <repo>/crates/dry4rs; the repo root is two
    // levels up. canonicalize() resolves the `..` segments so the
    // emitted JSON has clean relative paths (`crates/...` rather than
    // `crates/dry4rs/../../crates/...`).
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root must resolve");
    let output = Command::new(bin)
        .current_dir(&repo_root)
        .args(args)
        .output()
        .expect("dry4rs binary should execute");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status, stdout, stderr)
}

/// Project the unfiltered `result.summary` into a stable subset for
/// snapshotting. Pulls `total_forms`, the unfiltered `matches.len()`
/// (the wire shape's truth for "how many findings did we surface"), and
/// `by_tier` — the agentic-routing distribution that downstream
/// consumers (mokumo scorecard) care about. `by_kind` is intentionally
/// omitted: the production/test split is sensitive to where forms
/// happen to land between source and test trees and would churn the
/// snapshot on a routine test refactor without indicating a real
/// comparison-engine regression.
fn project_stable_subset(envelope: &Value) -> Value {
    let result = &envelope["result"];
    let summary = &result["summary"];
    let matches_count = result["matches"]
        .as_array()
        .expect("result.matches must be an array")
        .len();
    serde_json::json!({
        "total_forms": summary["total_forms"],
        "matches_count": matches_count,
        "by_tier": summary["by_tier"],
    })
}

#[test]
fn dry4rs_self_analysis_summary_matches_snapshot() {
    // Run dry4rs against the workspace's own crates/ tree. `--no-fail`
    // suppresses the non-zero exit so the test can run regardless of
    // whether in-tree matches pass the default threshold.
    let crates_arg = workspace_crates_dir();
    let crates_arg_str = crates_arg.to_string_lossy().into_owned();
    let (status, stdout, stderr) = run_dry4rs(&[
        "report",
        "--format",
        "json",
        "--no-fail",
        crates_arg_str.as_str(),
    ]);
    assert!(
        status.success(),
        "dry4rs --no-fail must exit 0 regardless of findings: \
         status={status:?}, stderr={stderr}"
    );
    let envelope: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("dry4rs JSON envelope must parse: {err}, stdout: {stdout}"));

    // Stable-subset snapshot. See module-level docs for the rationale
    // on which fields are snapshotted and which are intentionally
    // omitted.
    let projection = project_stable_subset(&envelope);
    insta::assert_json_snapshot!("dry4rs_self_analysis_stable_summary", projection);
}

#[test]
fn dry4rs_self_analysis_envelope_carries_locked_top_level_shape() {
    // Belt-and-suspenders structural check on the v0.1 wire envelope
    // from a REAL run. The wire-envelope-snapshot test in dry-core
    // proves the SHAPE is locked from a synthesized fixture; this test
    // proves a real, end-to-end production invocation surfaces the same
    // top-level keys. If either side drifts (e.g. someone bumps
    // schema_version without an ADR), this fails alongside the
    // dry-core snapshot.
    let crates_arg = workspace_crates_dir();
    let crates_arg_str = crates_arg.to_string_lossy().into_owned();
    let (status, stdout, _stderr) = run_dry4rs(&[
        "report",
        "--format",
        "json",
        "--no-fail",
        crates_arg_str.as_str(),
    ]);
    assert!(status.success(), "self-analysis must exit 0 with --no-fail");
    let envelope: Value = serde_json::from_str(&stdout).expect("envelope must parse");

    // Top-level locked keys per `adr-nested-json-envelope.md`.
    assert_eq!(envelope["schema_version"], 1, "schema_version must be 1");
    assert_eq!(envelope["tool"], "dry4rs", "tool identity must be dry4rs");
    assert_eq!(envelope["language"], "rust", "language must be rust");
    assert!(
        envelope["tool_version"].is_string(),
        "tool_version must be a string"
    );
    assert!(
        envelope["timestamp"].is_string(),
        "timestamp must be a string"
    );
    assert!(
        envelope["threshold_mode"].is_string(),
        "threshold_mode must be a string"
    );
    assert!(
        envelope["result"].is_object(),
        "result block must be present"
    );
    // `view` / `delta` / `diagnostics` are reserved at v0.1 and must
    // be ABSENT (not null) per the skip_serializing_if invariant.
    assert!(
        envelope.get("view").is_none(),
        "view must be absent without --top / --only-failing"
    );
    assert!(envelope.get("delta").is_none(), "delta must be absent");
    assert!(
        envelope.get("diagnostics").is_none(),
        "diagnostics must be absent"
    );
}
