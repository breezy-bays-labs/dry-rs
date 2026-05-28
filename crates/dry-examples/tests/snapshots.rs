//! Golden snapshot harness for the dry-examples corpus.
//!
//! Walks `examples/<tier>/<fixture>/` for subdirectories containing at
//! least one `.rs` file. For each fixture:
//!
//! - Invokes the dry4rs binary via `cargo run -p dry4rs --` with
//!   `report --format json --no-fail --include-ignored <fixture_dir>/`.
//!   The fixture directory is passed (not a single file) so multi-file
//!   fixtures like `edge_cases/cross_file_duplicate/` exercise the
//!   walker's recursive `.rs` discovery. `cargo run` is used rather
//!   than `CARGO_BIN_EXE_dry4rs` because the latter is package-local
//!   by design (cargo PR #8038) — it is only set when the integration
//!   test and the bin live in the same package. The corpus is in a
//!   sibling crate, so we use `env!("CARGO")` to find cargo and let
//!   it locate + build dry4rs.
//! - Normalizes the resulting JSON envelope per ADR-5: timestamp +
//!   tool_version sentinels at the top level, path-separator
//!   normalization on every `forms[].file` at any depth.
//! - Diffs the normalized envelope against the fixture's
//!   `expected.json` via `serde_json::Value` equality.
//!
//! ## `BLESS=1`
//!
//! Setting `BLESS=1` switches the harness to regen mode: instead of
//! diffing, each fixture's normalized envelope is WRITTEN to
//! `expected.json` (with a trailing `\n` so editor auto-insert-final-
//! newline doesn't churn the file on next save). Use this to (a)
//! generate the initial baselines on a fresh corpus add and (b) accept
//! an intentional detection-behavior change as part of a PR that
//! moves the detector.
//!
//! ## Normalization (per ADR-5)
//!
//! Symmetric normalization runs on BOTH actual and expected before
//! diff, so a checked-in `expected.json` carrying sentinels round-
//! trips cleanly.
//!
//! Three substitutions:
//!
//! 1. **Top-level `timestamp`** → `"TIMESTAMP_REDACTED"`. The
//!    envelope's `timestamp` flutters per run.
//! 2. **Top-level `tool_version`** → `"0.1.0"`. Hard-coded so version
//!    bumps don't invalidate every golden file.
//! 3. **Recursive `file` field path-separator normalization**: every
//!    object with a sibling `span` key has its `file` value rewritten
//!    `\` → `/`. Rust emits backslash separators on Windows runners
//!    (`std::path::Path` produces `examples\tier_1_exact\...`);
//!    rewriting at the boundary lets the same golden file work on
//!    POSIX and Windows. The substitution is a no-op on POSIX.
//!
//! Path emission to a workspace-relative form is achieved by setting
//! `current_dir = <crate manifest dir>` when spawning dry4rs and
//! passing relative paths. Empirically verified: dry4rs emits
//! `forms[].file = "examples/tier_1_exact/..."` rather than absolute
//! paths under this invocation pattern.
//!
//! ## EXPECTED.md sort lint
//!
//! A separate test (`expected_md_table_is_sorted`) parses the catalogue
//! table at `crates/dry-examples/EXPECTED.md` and asserts rows are
//! sorted by `(tier_number, fixture_path_lex)` with `edge_cases` last.
//! Per ADR-4, this prevents parallel-branch merge conflicts on the
//! catalogue.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ────────────────────────────────────────────────────────────────────
// Determinism guards (per ADR-5)
// ────────────────────────────────────────────────────────────────────

/// Sentinel substituted for the envelope's `timestamp` field before
/// diff. The actual `timestamp` flutters every run.
const TIMESTAMP_SENTINEL: &str = "TIMESTAMP_REDACTED";

/// Hard-coded `tool_version` value on the wire in every committed
/// `expected.json`. Decoupled from `env!("CARGO_PKG_VERSION")` so a
/// version bump on dry4rs doesn't invalidate the entire corpus.
const TOOL_VERSION_SENTINEL: &str = "0.1.0";

// ────────────────────────────────────────────────────────────────────
// Fixture discovery
// ────────────────────────────────────────────────────────────────────

/// Absolute path to `crates/dry-examples/examples/`. The harness uses
/// this as both the discovery root and the spawn-time `current_dir`'s
/// implicit base — the harness `cd`s to `CARGO_MANIFEST_DIR` and
/// passes the `examples/<tier>/<fixture>/` form relatively so dry4rs
/// emits workspace-relative `forms[].file` values.
fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

/// The crate manifest directory — the harness's `current_dir` for
/// dry4rs spawns. Setting this anchors `forms[].file` values to
/// `examples/<tier>/<fixture>/<file>.rs` rather than absolute paths
/// that would diverge across `/Users/...` (macOS dev) and
/// `/home/runner/work/...` (Linux CI).
fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Walk `examples/` for fixture directories. A directory is a fixture
/// if it sits two levels deep (under `examples/<tier>/<fixture>/`)
/// AND contains at least one `.rs` file. The any-`.rs` discipline
/// (not strict `main.rs`) accepts multi-file fixtures like
/// `edge_cases/cross_file_duplicate/` whose files are `producer.rs`
/// + `consumer.rs`.
///
/// Returns fixtures sorted by tier directory name then fixture
/// directory name — both ASCII-lexicographic — so the test output is
/// deterministic across platforms regardless of `read_dir`'s
/// filesystem-defined order.
///
/// Panics if `examples/` is missing or empty. A fixture-less corpus is
/// a hard error — silent zero-fixture passes would mask deletion bugs.
fn discover_fixtures() -> Vec<PathBuf> {
    let root = examples_root();
    let tier_entries = fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read examples root {}: {e}", root.display()));

    let mut fixtures: Vec<PathBuf> = Vec::new();
    let mut tier_dirs: Vec<PathBuf> = tier_entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    tier_dirs.sort();

    for tier_dir in tier_dirs {
        let fixture_entries = fs::read_dir(&tier_dir)
            .unwrap_or_else(|e| panic!("read tier dir {}: {e}", tier_dir.display()));
        let mut fixture_dirs: Vec<PathBuf> = fixture_entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        fixture_dirs.sort();
        for fixture_dir in fixture_dirs {
            if has_rs_file(&fixture_dir) {
                fixtures.push(fixture_dir);
            }
        }
    }

    assert!(
        !fixtures.is_empty(),
        "no fixtures discovered under {} — corpus must be non-empty",
        root.display(),
    );
    fixtures
}

/// True if `dir` contains at least one regular `.rs` file at the top
/// level. Used by `discover_fixtures` to classify a directory as a
/// fixture vs an organizational subdirectory.
fn has_rs_file(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries
        .filter_map(|e| e.ok())
        .any(|e| e.path().is_file() && e.path().extension().is_some_and(|x| x == "rs"))
}

// ────────────────────────────────────────────────────────────────────
// dry4rs binary invocation via cargo run
// ────────────────────────────────────────────────────────────────────

/// Run dry4rs against `fixture_dir` via `cargo run -p dry4rs --` and
/// return the parsed JSON envelope. `current_dir` is set to the dry-
/// examples manifest directory so dry4rs emits workspace-relative
/// `forms[].file` values.
///
/// Why `cargo run` instead of `CARGO_BIN_EXE_dry4rs`: the
/// `CARGO_BIN_EXE_<name>` env var is set by cargo only when the
/// integration test and the bin target live in the same package
/// (cargo PR #8038). dry-examples is a sibling crate to dry4rs, so
/// that env var is not set for tests in this crate. `cargo run -p
/// dry4rs` locates + builds dry4rs the same way it does interactively.
/// `env!("CARGO")` resolves at compile time to the cargo binary path
/// that triggered this build — the standard pattern for cross-package
/// binary invocation from tests.
///
/// Args passed to dry4rs: `report --format json --no-fail
/// --include-ignored <relative_path>`. `--include-ignored` is load-
/// bearing — the corpus crate's `.ignore` file (`*`) opts the
/// directory out of default walks (per ADR-2) so the dry-self snapshot
/// stays untouched; the harness opts back in. `--no-fail` ensures the
/// binary exits 0 regardless of detection verdict so the harness can
/// read stdout without a status check.
fn run_dry4rs_against(fixture_dir: &Path) -> Value {
    let manifest = manifest_dir();
    // Re-express the fixture path as relative-to-manifest so dry4rs
    // emits `examples/<tier>/<fixture>/` rather than the full
    // absolute path. `strip_prefix` is the cleanest reversal:
    // `<manifest>/examples/.../fixture` minus `<manifest>` = the
    // expected relative form.
    let relative = fixture_dir.strip_prefix(&manifest).unwrap_or_else(|_| {
        panic!(
            "fixture {} is not under {}",
            fixture_dir.display(),
            manifest.display()
        )
    });
    // Append a `/` so the path is recognized as a directory by the
    // walker; not strictly required but matches the documented
    // invocation pattern.
    let relative_str = format!("{}/", relative.display());

    let output = Command::new(env!("CARGO"))
        .current_dir(&manifest)
        .args([
            "run",
            "--quiet",
            "--locked",
            "-p",
            "dry4rs",
            "--",
            "report",
            "--format",
            "json",
            "--no-fail",
            "--include-ignored",
            &relative_str,
        ])
        .output()
        .unwrap_or_else(|e| panic!("spawn cargo run dry4rs: {e}"));

    assert!(
        output.status.success(),
        "cargo run -p dry4rs against {} exited non-zero: status={:?}\nstderr:\n{}",
        fixture_dir.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).unwrap_or_else(|e| {
        panic!(
            "dry4rs stdout is not UTF-8 for {}: {e}",
            fixture_dir.display()
        )
    });
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "dry4rs stdout is not valid JSON for {}: {e}\nstdout:\n{stdout}",
            fixture_dir.display()
        )
    })
}

// ────────────────────────────────────────────────────────────────────
// Normalization (per ADR-5)
// ────────────────────────────────────────────────────────────────────

/// In-place normalize the envelope so the resulting `Value` is
/// byte-stable across machines, CI runs, and OS:
///
/// 1. `timestamp` (top-level) → `"TIMESTAMP_REDACTED"`.
/// 2. `tool_version` (top-level) → `"0.1.0"`.
/// 3. Every object with a sibling `span` key has its `file` value
///    rewritten with `\` → `/`. Recursive walk; lives anywhere the
///    nested wire envelope places `forms[].file` (today at
///    `result.matches[].forms[].file`, possibly elsewhere in future
///    schemas).
///
/// Symmetric application: the harness runs this on BOTH the actual
/// envelope (from dry4rs stdout) AND the expected envelope (from
/// `expected.json` on disk) before diff. A committed `expected.json`
/// that already carries sentinels round-trips cleanly.
fn normalize_envelope_for_diff(value: &mut Value) {
    if let Some(obj) = value.as_object_mut() {
        if obj.contains_key("timestamp") {
            obj.insert(
                "timestamp".to_string(),
                Value::String(TIMESTAMP_SENTINEL.to_string()),
            );
        }
        if obj.contains_key("tool_version") {
            obj.insert(
                "tool_version".to_string(),
                Value::String(TOOL_VERSION_SENTINEL.to_string()),
            );
        }
    }
    normalize_file_paths(value);
}

/// Recursive helper: walk `value` and, for every JSON object that
/// holds both a `file` (string) and a `span` key, rewrite the `file`
/// value's backslashes to forward slashes. Catches `FormRef`-shaped
/// objects regardless of where they nest. Other strings carrying
/// `\\` are NOT rewritten — the substitution is targeted to the
/// `file`-with-`span`-sibling shape so unrelated `\\` in error
/// messages or comments stays untouched.
fn normalize_file_paths(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let has_span = map.contains_key("span");
            if has_span {
                if let Some(Value::String(file)) = map.get_mut("file") {
                    *file = file.replace('\\', "/");
                }
            }
            for (_k, v) in map.iter_mut() {
                normalize_file_paths(v);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                normalize_file_paths(item);
            }
        }
        _ => {}
    }
}

// ────────────────────────────────────────────────────────────────────
// expected.json read/write
// ────────────────────────────────────────────────────────────────────

/// Read `<fixture_dir>/expected.json` and parse to `Value`.
fn read_expected(fixture_dir: &Path) -> Value {
    let path = fixture_dir.join("expected.json");
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Pretty-print `value` to `<fixture_dir>/expected.json` with a
/// trailing `\n` (so editor auto-insert-final-newline doesn't churn
/// the file on next save).
fn write_expected(fixture_dir: &Path, value: &Value) {
    let pretty = serde_json::to_string_pretty(value).expect("serialize expected.json");
    let path = fixture_dir.join("expected.json");
    fs::write(&path, format!("{pretty}\n"))
        .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

/// True iff `BLESS=1` (or any truthy non-empty value) is set.
fn bless_mode() -> bool {
    std::env::var("BLESS").is_ok_and(|v| !v.is_empty() && v != "0")
}

// ────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────

/// For every discovered fixture: spawn dry4rs, normalize the envelope,
/// and either bless `expected.json` (if `BLESS=1`) or compare via
/// `serde_json::Value` equality. Accumulates mismatches before
/// asserting so a single bad fixture doesn't hide the others.
///
/// Fixture dirs without an `expected.json` are SKIPPED in verify mode
/// (so a fixture-in-progress doesn't break CI before its golden has
/// been blessed) — but the empty corpus assert in `discover_fixtures`
/// catches the "no fixtures at all" regression.
#[test]
fn fixture_envelopes_match_expected_json() {
    let bless = bless_mode();
    let mut mismatches: Vec<String> = Vec::new();
    let manifest = manifest_dir();

    for fixture_dir in discover_fixtures() {
        let display_name = fixture_dir
            .strip_prefix(&manifest)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| fixture_dir.display().to_string());

        let mut actual = run_dry4rs_against(&fixture_dir);
        normalize_envelope_for_diff(&mut actual);

        if bless {
            write_expected(&fixture_dir, &actual);
            continue;
        }

        let expected_path = fixture_dir.join("expected.json");
        if !expected_path.is_file() {
            mismatches.push(format!(
                "fixture {display_name}: missing expected.json — run \
                 `BLESS=1 cargo test -p dry-examples` to generate the \
                 golden file, then review the diff before committing"
            ));
            continue;
        }

        let mut expected = read_expected(&fixture_dir);
        normalize_envelope_for_diff(&mut expected);

        if actual != expected {
            let actual_pretty = serde_json::to_string_pretty(&actual).unwrap_or_default();
            let expected_pretty = serde_json::to_string_pretty(&expected).unwrap_or_default();
            mismatches.push(format!(
                "fixture {display_name}\n--- expected ---\n{expected_pretty}\n--- actual ---\n{actual_pretty}"
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} fixture(s) drifted; re-run with BLESS=1 to regenerate after \
         review:\n\n{}",
        mismatches.len(),
        mismatches.join("\n\n"),
    );
}

// ────────────────────────────────────────────────────────────────────
// EXPECTED.md sort lint (per ADR-4)
// ────────────────────────────────────────────────────────────────────

/// Tier ordering for EXPECTED.md sort verification. `tier_N_*` rows
/// sort by `N`; `edge_cases` sorts last (sentinel `u32::MAX`).
fn tier_sort_key(tier_prefix: &str) -> u32 {
    if tier_prefix == "edge_cases" {
        return u32::MAX;
    }
    // Expect `tier_<N>_...` shape; strip prefix + suffix to extract N.
    let n_part = tier_prefix.strip_prefix("tier_").unwrap_or(tier_prefix);
    let n_str = n_part.split('_').next().unwrap_or("");
    n_str.parse::<u32>().unwrap_or(u32::MAX - 1)
}

/// Parse the catalogue table in `EXPECTED.md`. Each row's second
/// column (path) is the sort key: split on `/` into `(tier, fixture)`.
/// Returns the list of `(tier, fixture)` pairs in source order so the
/// caller can compare against the sorted form.
///
/// Table rows are matched by the leading pipe + path-shape. The
/// header row, separator row, and prose lines are filtered out.
fn parse_expected_md_paths() -> Option<Vec<(String, String)>> {
    let md_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("EXPECTED.md");
    let text = fs::read_to_string(&md_path).ok()?;
    let mut paths: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(|c| c.trim())
            .collect();
        // Need at least: tier, path, ... — at least 2 cells of data.
        if cells.len() < 2 {
            continue;
        }
        // Skip header row (first column literal "Tier").
        if cells[0].eq_ignore_ascii_case("tier") {
            continue;
        }
        // Skip separator row (cells made of - + : + whitespace).
        if cells.iter().all(|c| {
            !c.is_empty()
                && c.chars()
                    .all(|ch| ch == '-' || ch == ':' || ch.is_whitespace())
        }) {
            continue;
        }
        // Path cell — strip backticks if present.
        let path_cell = cells[1].trim_matches('`');
        // Expected shape: `<tier>/<fixture>` (optional trailing /).
        let path_clean = path_cell.trim_end_matches('/');
        let mut parts = path_clean.splitn(2, '/');
        let tier = parts.next().unwrap_or("").to_string();
        let fixture = parts.next().unwrap_or("").to_string();
        if tier.is_empty() || fixture.is_empty() {
            continue;
        }
        paths.push((tier, fixture));
    }
    Some(paths)
}

/// The canonical comparator for EXPECTED.md rows: sort by
/// `(tier_sort_key, fixture_path_lex)`. Per ADR-4, this makes the
/// catalogue's order deterministic so two contributors adding rows in
/// parallel branches resolve conflicts by re-sorting, not by human
/// judgment.
fn sort_key(row: &(String, String)) -> (u32, String, String) {
    (tier_sort_key(&row.0), row.0.clone(), row.1.clone())
}

/// Assert the catalogue table in EXPECTED.md is sorted by
/// `(tier_number, fixture_path_lex)` with `edge_cases` last.
///
/// EXPECTED.md may not exist yet — Stage 2.6 lands it. When absent,
/// this test no-ops (returns the empty case from
/// `parse_expected_md_paths` and asserts on a length-0 vec, which is
/// trivially sorted). Once EXPECTED.md exists, the lint activates.
#[test]
fn expected_md_table_is_sorted() {
    let Some(rows) = parse_expected_md_paths() else {
        // EXPECTED.md missing — pre-Stage-2.6 state; lint is vacuous.
        return;
    };
    if rows.is_empty() {
        // Table parsed but contained no fixture rows — possibly
        // EXPECTED.md exists but hasn't been populated yet. Treat as
        // vacuous; the fixture-envelope test still catches actual
        // corpus regressions.
        return;
    }

    let mut sorted = rows.clone();
    sorted.sort_by_key(sort_key);

    if rows != sorted {
        let mut report = String::from(
            "EXPECTED.md catalogue rows are not sorted by \
             (tier_number, fixture_path_lex) with edge_cases last.\n\n\
             Per ADR-4, rows must be in this canonical order so parallel \
             branches resolve catalogue conflicts mechanically.\n\n\
             Found order:\n",
        );
        for (tier, fixture) in &rows {
            report.push_str(&format!("  {tier}/{fixture}\n"));
        }
        report.push_str("\nExpected order:\n");
        for (tier, fixture) in &sorted {
            report.push_str(&format!("  {tier}/{fixture}\n"));
        }
        panic!("{report}");
    }
}
