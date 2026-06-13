//! End-to-end pipeline tests for the `dry4rs` CLI binary.
//!
//! These tests exercise the full `run::<SynNormalizer>()` pipeline
//! through the genuine `main()` entry path so the truthful-gate
//! invariant (`result.*` immune to `--top` / `--only-failing` /
//! `--no-fail`) is validated against the real wire output, not just
//! the in-process `build_view` helper.
//!
//! A fixture corpus with two identical functions is written into a
//! tempdir; the comparison engine reliably surfaces them as an
//! `auto_refactor` match. Tests then assert the wire-envelope shape
//! and exit-code behavior under various flag combinations.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Write the duplication-fixture corpus into `dir` — two `.rs` files
/// containing structurally-identical function bodies. With identical
/// function names + bodies the comparison engine surfaces them as a
/// score-1.0 `auto_refactor` cluster (Pass 1 hash-bucket match). The
/// adapter normalizes function names verbatim at v0.1 (only local
/// variable identifiers collapse to `Var` placeholders), so renaming
/// either function name would push the pair into Pass 2 with a lower
/// Jaccard score — fine for rename-signal tests but not what we want
/// here.
fn write_duplication_fixture(dir: &Path) {
    fs::create_dir_all(dir).expect("mkdir");
    let body = "pub fn greet(name: &str) -> String {\n    format!(\"hello, {name}\")\n}\n";
    fs::write(dir.join("alpha.rs"), body).expect("write alpha.rs");
    fs::write(dir.join("beta.rs"), body).expect("write beta.rs");
}

/// Run `dry4rs <args>` against the tempdir corpus and return
/// (status, stdout, stderr).
fn run_dry4rs(args: &[&str]) -> (std::process::ExitStatus, String, String) {
    let bin = env!("CARGO_BIN_EXE_dry4rs");
    let output = Command::new(bin)
        .args(args)
        .output()
        .expect("dry4rs binary should execute");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status, stdout, stderr)
}

#[test]
fn json_envelope_omits_view_when_no_shaping_flags_set() {
    // Without `--top` or `--only-failing`, the JSON envelope's `view`
    // field must be ABSENT (skip_serializing_if = "Option::is_none"),
    // matching the wire-envelope snapshot lock.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (status, stdout, _) = run_dry4rs(&["report", "--format", "json", &path_arg]);
    // The fixture has a duplicate so the report exits non-zero by
    // default; we don't care about the exit code here, only the wire
    // shape.
    let _ = status;
    let envelope: Value = serde_json::from_str(&stdout).expect("envelope must parse");
    assert!(
        envelope.get("view").is_none(),
        "view must be absent: {stdout}"
    );
}

#[test]
fn json_envelope_populates_view_when_top_flag_set() {
    // With `--top N`, the view projection populates with the top-N
    // matches by descending score. `result.*` stays unfiltered.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_status, stdout, _) = run_dry4rs(&["report", "--format", "json", "--top", "1", &path_arg]);
    let envelope: Value = serde_json::from_str(&stdout).expect("envelope must parse");
    assert!(
        envelope.get("view").is_some(),
        "view must populate: {stdout}"
    );
    let result_matches = envelope["result"]["matches"]
        .as_array()
        .expect("result.matches must be an array");
    let view_matches = envelope["view"]["matches"]
        .as_array()
        .expect("view.matches must be an array");
    assert!(
        view_matches.len() <= result_matches.len(),
        "view.matches must not exceed result.matches; \
         view={}, result={}",
        view_matches.len(),
        result_matches.len()
    );
    assert!(
        view_matches.len() <= 1,
        "--top 1 must truncate to one match, got {}",
        view_matches.len()
    );
}

#[test]
fn json_envelope_view_does_not_mutate_result_matches_under_top() {
    // Truthful-gate invariant: `result.matches.len()` must be the
    // unfiltered count regardless of `--top`. CI parsers reading
    // `result.passed` are immune to view-shaping.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();

    let (_, stdout_no_top, _) = run_dry4rs(&["report", "--format", "json", &path_arg]);
    let no_top: Value = serde_json::from_str(&stdout_no_top).expect("envelope must parse");
    let unfiltered_count = no_top["result"]["matches"]
        .as_array()
        .expect("result.matches array")
        .len();

    let (_, stdout_top, _) = run_dry4rs(&["report", "--format", "json", "--top", "1", &path_arg]);
    let topped: Value = serde_json::from_str(&stdout_top).expect("envelope must parse");
    let result_count_with_top = topped["result"]["matches"]
        .as_array()
        .expect("result.matches array")
        .len();

    assert_eq!(
        unfiltered_count, result_count_with_top,
        "result.matches.len() must stay unfiltered under --top; \
         no-flag={unfiltered_count}, --top 1={result_count_with_top}"
    );
}

#[test]
fn json_envelope_view_passed_mirrors_result_passed() {
    // Per the wire-envelope ADR: the view's `passed` never overrides
    // the gate verdict; it carries the same value for symmetry.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_, stdout, _) = run_dry4rs(&["report", "--format", "json", "--top", "1", &path_arg]);
    let envelope: Value = serde_json::from_str(&stdout).expect("envelope must parse");
    let result_passed = envelope["result"]["passed"]
        .as_bool()
        .expect("result.passed bool");
    let view_passed = envelope["view"]["passed"]
        .as_bool()
        .expect("view.passed bool");
    assert_eq!(
        result_passed, view_passed,
        "view.passed must mirror result.passed; got result={result_passed}, view={view_passed}"
    );
}

#[test]
fn match_form_ref_carries_real_source_path_not_synthesized_stub() {
    // Regression test for the run-loop path-wiring fix. The comparison
    // engine's library-facing `compare()` synthesizes `FormRef.file`
    // from `qualified_name` (a placeholder); the run loop calls
    // `compare_with_paths` so each emitted FormRef carries the real
    // source path. Verifying via the binary so regressions surface
    // even if internal helpers refactor.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_, stdout, _) = run_dry4rs(&["report", "--format", "json", &path_arg]);
    let envelope: Value = serde_json::from_str(&stdout).expect("envelope must parse");
    let forms = envelope["result"]["matches"][0]["forms"]
        .as_array()
        .expect("result.matches[0].forms array");
    assert_eq!(forms.len(), 2, "fixture emits two duplicate forms");
    for form in forms {
        let file = form["file"]
            .as_str()
            .expect("form.file must be a string")
            .to_string();
        assert!(
            file.contains(".rs"),
            "form.file must be a real source path (containing .rs), got: {file:?}"
        );
        // The synthesized fallback would produce a path like "greet"
        // (no extension); the real path contains the file basename.
        assert!(
            file.ends_with("alpha.rs") || file.ends_with("beta.rs"),
            "form.file must be one of the fixture file paths, got: {file:?}"
        );
    }
}

#[test]
fn view_summary_total_forms_mirrors_result_summary_total_forms() {
    // Per the wire-envelope ADR: `total_forms` is a per-run survey
    // total, NOT a per-match aggregate. View shaping happens AFTER the
    // survey, so `view.summary.total_forms` must mirror
    // `result.summary.total_forms` — it counts the pre-filter survey,
    // not the post-filter match list.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_, stdout, _) = run_dry4rs(&["report", "--format", "json", "--top", "1", &path_arg]);
    let envelope: Value = serde_json::from_str(&stdout).expect("envelope must parse");
    let result_total = envelope["result"]["summary"]["total_forms"]
        .as_u64()
        .expect("result.summary.total_forms");
    let view_total = envelope["view"]["summary"]["total_forms"]
        .as_u64()
        .expect("view.summary.total_forms");
    assert_eq!(
        result_total, view_total,
        "view.summary.total_forms must mirror result.summary.total_forms; \
         result={result_total}, view={view_total}"
    );
    assert!(
        result_total > 0,
        "fixture should contribute at least one form to the survey"
    );
}

#[test]
fn json_envelope_has_locked_top_level_fields() {
    // Spot-check the v0.1 wire envelope shape over a real run. The
    // mechanical lock lives in `wire_envelope_snapshot.rs`; this test
    // verifies the binary actually emits that shape through the full
    // pipeline.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_, stdout, _) = run_dry4rs(&["report", "--format", "json", &path_arg]);
    let envelope: Value = serde_json::from_str(&stdout).expect("envelope must parse");

    assert_eq!(
        envelope["schema_version"].as_u64(),
        Some(1),
        "schema_version locked at 1"
    );
    assert_eq!(envelope["tool"].as_str(), Some("dry4rs"));
    assert_eq!(envelope["language"].as_str(), Some("rust"));
    assert_eq!(envelope["threshold_mode"].as_str(), Some("default"));
    assert!(envelope["timestamp"].as_str().is_some());
    assert!(envelope["tool_version"].as_str().is_some());
    assert!(envelope["result"].is_object(), "result must be present");
}

#[test]
fn exit_code_is_failure_when_findings_exceed_threshold() {
    // The fixture produces a score-1.0 match which fails the default
    // 0.85 threshold; without `--no-fail`, exit code is non-zero.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (status, _stdout, _) = run_dry4rs(&["check", &path_arg]);
    assert!(
        !status.success(),
        "expected non-zero exit on duplicate fixture; got status: {status:?}"
    );
    // ExitCode::FAILURE = 1 on Unix.
    assert_eq!(
        status.code(),
        Some(1),
        "expected exit code 1 (FAILURE), got: {status:?}"
    );
}

#[test]
fn exit_code_is_success_when_no_fail_flag_is_set() {
    // `--no-fail` suppresses the non-zero exit even when findings
    // exceed the threshold. `result.passed` stays false in the JSON
    // output regardless — verifying this requires checking both the
    // exit code AND the wire shape.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();

    let (status, stdout, _) = run_dry4rs(&["report", "--format", "json", "--no-fail", &path_arg]);
    assert!(
        status.success(),
        "--no-fail must suppress non-zero exit; got status: {status:?}"
    );
    let envelope: Value = serde_json::from_str(&stdout).expect("envelope must parse");
    assert_eq!(
        envelope["result"]["passed"].as_bool(),
        Some(false),
        "result.passed must stay false under --no-fail (truthful gate): {stdout}"
    );
}

#[test]
fn completions_flag_emits_a_bash_completion_script() {
    // `--completions bash` short-circuits the analyzer pipeline,
    // emitting the completion script to stdout and exiting 0.
    let bin = env!("CARGO_BIN_EXE_dry4rs");
    let output = Command::new(bin)
        .args(["--completions", "bash"])
        .output()
        .expect("dry4rs binary should execute");
    assert!(
        output.status.success(),
        "--completions must exit 0; got: {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // bash completion scripts include a `complete -F` line at the end.
    assert!(
        stdout.contains("complete") && stdout.contains("dry4rs"),
        "expected bash completion script, got: {stdout:?}"
    );
}

#[test]
fn check_subcommand_exit_code_only_emits_no_stdout() {
    // `check` mode is exit-code-only — no human-readable stdout
    // regardless of `--format`. Verifies the dispatch arm.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_, stdout, _) = run_dry4rs(&["check", &path_arg]);
    assert!(
        stdout.trim().is_empty(),
        "check mode must emit no stdout, got: {stdout:?}"
    );
}

#[test]
fn stats_subcommand_emits_summary_only_in_text_mode() {
    // `stats --format text` emits the summary counters; no per-match
    // detail. Verifies the dispatch arm.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_, stdout, _) = run_dry4rs(&["stats", &path_arg]);
    assert!(
        stdout.contains("total_forms:"),
        "stats text output should include total_forms: {stdout:?}"
    );
    assert!(
        stdout.contains("passed:"),
        "stats text output should include passed: {stdout:?}"
    );
}

#[test]
fn stats_subcommand_text_emits_by_tier_and_by_kind_labels() {
    // The text dispatch path renders the `by_tier` and `by_kind`
    // summary maps with stable string labels. Anchoring at least one
    // representative key covers the inner `match *tier` and
    // `match *kind` arms.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_, stdout, _) = run_dry4rs(&["stats", &path_arg]);
    // The duplication fixture surfaces an auto_refactor cluster, so
    // `by_tier.auto_refactor:` must appear in text output.
    assert!(
        stdout.contains("by_tier.auto_refactor:"),
        "stats text output should include by_tier.auto_refactor label: {stdout:?}"
    );
    // Both fixture files are production (no `#[test]` / cfg-test
    // qualification), so `by_kind.production:` must surface.
    assert!(
        stdout.contains("by_kind.production:"),
        "stats text output should include by_kind.production label: {stdout:?}"
    );
}

#[test]
fn stats_subcommand_emits_full_wire_envelope_in_json_mode() {
    // `stats --format json` reuses the full envelope path — consumers
    // parsing `result.summary` get the same shape they'd get from
    // `report --format json`. This covers the JSON arm of the
    // `emit_stats` dispatch (previously uncovered branch).
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_, stdout, _) = run_dry4rs(&["stats", "--format", "json", &path_arg]);
    let envelope: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stats JSON must parse: {err}, stdout: {stdout}"));
    // Top-level locked keys (mirrors the wire-envelope snapshot lock).
    assert_eq!(envelope["schema_version"], 1, "schema_version must be 1");
    assert_eq!(envelope["tool"], "dry4rs", "tool identity must be dry4rs");
    assert!(
        envelope["result"]["summary"]["total_forms"].is_number(),
        "result.summary.total_forms must be present: {stdout}"
    );
}

#[test]
fn stats_subcommand_json_carries_summary_by_tier() {
    // The duplication fixture surfaces an `auto_refactor` cluster, so
    // the JSON envelope's `result.summary.by_tier` must include an
    // `auto_refactor` entry with a positive count.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_, stdout, _) = run_dry4rs(&["stats", "--format", "json", &path_arg]);
    let envelope: Value = serde_json::from_str(&stdout).expect("stats JSON must parse");
    let by_tier = &envelope["result"]["summary"]["by_tier"];
    assert!(
        by_tier["auto_refactor"].as_u64().unwrap_or(0) >= 1,
        "by_tier.auto_refactor must reflect the fixture cluster: {stdout}"
    );
}

/// Extract the `#dry-data` island text (base64) from a rendered HTML page.
/// Panics if the island is missing or malformed — the structural gate.
fn extract_data_island(html: &str) -> &str {
    let open = "type=\"application/json\">";
    let start = html.find(open).expect("dry-data island open tag") + open.len();
    let end = start
        + html[start..]
            .find("</script>")
            .expect("dry-data island close");
    &html[start..end]
}

/// Decode the base64 `#dry-data` island back into the parsed JSON envelope.
fn decode_data_island(html: &str) -> Value {
    use base64::Engine as _;
    let b64 = extract_data_island(html).trim();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("dry-data island must be valid base64");
    let json = String::from_utf8(bytes).expect("decoded island must be UTF-8");
    serde_json::from_str(&json).expect("decoded island must be valid JSON")
}

#[test]
fn report_html_emits_single_file_with_valid_data_island() {
    // End-to-end render-smoke through the real binary: `report --format
    // html` must emit one self-contained page whose `#dry-data` island
    // parses as the wire envelope (mode=report) and carries the fixture's
    // cluster. The structural HTML/JSON assertion is the CI gate (no
    // browser needed).
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (_status, stdout, _) = run_dry4rs(&["report", "--format", "html", "--no-fail", &path_arg]);

    // Single-file shell hooks.
    assert!(
        stdout.starts_with("<!doctype html>") || stdout.starts_with("<!DOCTYPE html>"),
        "must be a single HTML file: {stdout}"
    );
    assert!(stdout.contains("<style>"), "inline CSS expected");
    assert!(
        stdout.contains("type=\"module\""),
        "inline ES-module expected"
    );
    assert!(
        stdout.contains("id=\"overview\""),
        "overview container expected"
    );
    assert!(
        stdout.contains("id=\"clusters\""),
        "clusters container expected"
    );
    assert!(stdout.contains("id=\"dry-data\""), "data island expected");
    // The island is base64 — its alphabet has no `<`, so no `</script>`
    // break-out is structurally possible (the security property).
    assert!(
        !extract_data_island(&stdout).contains('<'),
        "base64 island must contain no `<`: {stdout}"
    );

    // The island decodes to the wire envelope, tagged REPORT mode with the
    // SHOWCASE capabilities (dry-rs#149) — every view the frontend now
    // renders is advertised true (template skeleton + substitution grid +
    // d-slider + scope banner), not just the PR13 overview/clusters pair.
    let envelope = decode_data_island(&stdout);
    assert_eq!(envelope["schema_version"], 1);
    assert_eq!(envelope["mode"], "report");
    assert_eq!(envelope["capabilities"]["overview"], true);
    assert_eq!(envelope["capabilities"]["clusters"], true);
    assert_eq!(envelope["capabilities"]["substitution_grid"], true);
    assert_eq!(envelope["capabilities"]["d_slider"], true);
    assert_eq!(envelope["capabilities"]["scope_banner"], true);
    // The fixture surfaces a cluster; the truthful gate carries it.
    assert!(
        envelope["result"]["matches"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "result.matches must carry the fixture cluster: {stdout}"
    );
    // Scope echo is populated by the run loop (Track B), proving the HTML
    // reporter rides the same run-loop envelope as JSON.
    assert!(
        envelope["scope"].is_object(),
        "run-loop envelope must echo scope: {stdout}"
    );
}

#[test]
fn report_html_no_fail_exits_zero() {
    // `--no-fail` suppresses the gate's non-zero exit even when the HTML
    // reporter surfaces findings — same contract as the other reporters.
    let tmp = tempfile::TempDir::new().unwrap();
    write_duplication_fixture(tmp.path());
    let path_arg = tmp.path().to_string_lossy().into_owned();
    let (status, _, _) = run_dry4rs(&["report", "--format", "html", "--no-fail", &path_arg]);
    assert!(status.success(), "html --no-fail must exit 0");
}

#[test]
fn ignore_subcommand_is_skeletal_at_v0_1() {
    // `ignore <fingerprint>` is a v0.1 stub — emits a stderr note
    // explaining the deferral and exits 0.
    let (status, _, stderr) = run_dry4rs(&["ignore", "deadbeef"]);
    assert!(status.success(), "ignore stub must exit 0");
    assert!(
        stderr.contains("v0.1 stub"),
        "ignore stub must surface deferral note: {stderr}"
    );
    assert!(
        stderr.contains("deadbeef"),
        "ignore stub must echo the fingerprint: {stderr}"
    );
}
