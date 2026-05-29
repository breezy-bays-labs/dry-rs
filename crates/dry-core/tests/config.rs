//! Integration tests for the config-file loader.
//!
//! Per ADR D7, this file MUST NOT contain double-quoted
//! adapter-binary-name literals (`"dry4rs.toml"`, `"dry4rs"`, etc.).
//! All adapter-name plumbing flows through `TEST_META`'s synthetic
//! `"test-adapter.toml"` literal. The layer-4 ast-purity gate
//! (`scripts/check-config-ast-purity.sh`, landing Stage 3) enforces
//! this mechanically.
//!
//! Test coverage:
//! - N60: round-trip property — `parse → to_string_pretty → parse`
//!   produces an equivalent `Config`.
//! - N61: walk-up integration — nested tempdir + config-at-root walks
//!   upward and finds the file.
//! - N62: explicit `--config` missing-is-error — `load_config` of a
//!   nonexistent path produces [`ConfigError::Io`].
//! - N63: auto-discovery missing-is-Ok(None) — `discover_config` of a
//!   tempdir with no ancestor containing the file returns Ok(None).
//! - N64: unknown-key produces a clear error message — plain string
//!   assertion (per CEng M4 audit; `insta` overkill for a single
//!   error contract).

mod common;

use std::fs;

use dry_core::adapters::config::{ConfigError, discover_config, load_config, parse_config};
use dry_core::cli::{Format, ThresholdMode};
use dry_core::domain::Config;

use common::TEST_META;

// =============================================================================
// N60 — round-trip
// =============================================================================

#[test]
fn n60_round_trip_through_serde_with_explicit_values() {
    let mut c = Config::default();
    c.gate.threshold = Some(0.9);
    c.gate.threshold_mode = Some(ThresholdMode::Strict);
    c.output.format = Some(Format::Json);
    c.walk.include_ignored = Some(true);
    c.walk.extensions = Some(vec!["rs".to_string(), "rsi".to_string()]);

    let serialized = toml::to_string_pretty(&c).expect("serialize");
    let parsed: Config = toml::from_str(&serialized).expect("deserialize");
    let reserialized = toml::to_string_pretty(&parsed).expect("re-serialize");
    assert_eq!(
        serialized, reserialized,
        "round-trip MUST be byte-stable; first serialization differs"
    );
    assert_eq!(parsed, c, "round-trip MUST preserve Config equality");
}

#[test]
fn n60_round_trip_empty_default_config() {
    // A default Config serializes to an empty string (every sub-table
    // is skipped via skip_serializing_if). Round-trip MUST still
    // produce a Default-equivalent Config when re-parsed.
    let c = Config::default();
    let serialized = toml::to_string_pretty(&c).expect("serialize");
    let parsed: Config = toml::from_str(&serialized).expect("deserialize");
    assert_eq!(parsed, c);
}

// =============================================================================
// N61 — walk-up integration
// =============================================================================

#[test]
fn n61_discover_config_walks_upward_from_nested_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let config_path = root.join(TEST_META.config_file_name);
    fs::write(&config_path, b"[gate]\nthreshold = 0.9\n").expect("write");

    let nested = root.join("nested").join("deeper");
    fs::create_dir_all(&nested).expect("mkdir");

    let found = discover_config(&nested, TEST_META.config_file_name)
        .expect("discover_config must not error");
    let found_path = found.expect("nested ancestor walk must find the config");
    assert_eq!(
        found_path.canonicalize().expect("canonicalize found"),
        config_path.canonicalize().expect("canonicalize config"),
        "discovered path must be the workspace-root config"
    );

    let cfg = load_config(&found_path).expect("load_config must parse the discovered file");
    assert_eq!(cfg.gate.threshold, Some(0.9));
}

// =============================================================================
// N62 — explicit `--config` missing-is-error
// =============================================================================

#[test]
fn n62_explicit_load_of_missing_path_produces_io_error() {
    let missing = std::path::Path::new("/nonexistent/synthetic/path.toml");
    let err = load_config(missing).expect_err("missing path must produce ConfigError::Io");
    match err {
        ConfigError::Io { path, .. } => {
            assert_eq!(path, missing);
        }
        other => panic!("expected Io error, got: {other:?}"),
    }
}

// =============================================================================
// N63 — auto-discovery missing-is-Ok(None)
// =============================================================================

#[test]
fn n63_auto_discovery_returns_ok_none_when_no_ancestor_contains_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Tempdir is fresh; no ancestor contains the synthetic file.
    let result = discover_config(tmp.path(), TEST_META.config_file_name)
        .expect("walk must not error on missing file");
    assert!(
        result.is_none(),
        "fresh tempdir must produce Ok(None); got: {result:?}"
    );
}

// =============================================================================
// N64 — unknown-key error contract
// =============================================================================

#[test]
fn n64_unknown_top_level_key_produces_parse_error_with_line() {
    let path = std::path::Path::new("synthetic-fixture.toml");
    let bad = "nonsense_top_level = 42\n";
    let err = parse_config(path, bad).expect_err("unknown top-level key must reject");
    let msg = err.to_string();
    // Per ADR D5: top-level error includes the path. The underlying
    // toml::de::Error source carries line info + the offending key.
    // Plain string assertion (per CEng M4) suffices for the contract;
    // no insta snapshot.
    assert!(msg.contains("failed to parse"), "msg: {msg}");

    // The underlying source MUST surface the offending key name.
    let source = std::error::Error::source(&err).expect("ConfigError::Parse carries a source");
    let source_msg = source.to_string();
    assert!(
        source_msg.contains("nonsense_top_level"),
        "source error should name the unknown key; got: {source_msg}"
    );
}

#[test]
fn n64_unknown_nested_key_produces_parse_error() {
    let path = std::path::Path::new("synthetic-fixture.toml");
    let bad = "[gate]\nnonsense_inner = true\n";
    let err = parse_config(path, bad).expect_err("unknown nested key must reject");
    let msg = err.to_string();
    assert!(msg.contains("failed to parse"), "msg: {msg}");
}
