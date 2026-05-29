//! End-to-end cascade integration tests for the dry-rs#78
//! multi-language config shape.
//!
//! Exercises the production wiring through
//! `dry_core::cli::merge_effective_inputs` against a tempdir-written
//! `dry.toml`. Verifies that per-language overrides shadow shared
//! values for the matching adapter, fall back to shared values when
//! unset, and stay isolated from the other language section.
//!
//! The unit-level cascade behavior lives in
//! `dry-core::cli::effective::tests`; this file is the broader
//! "Args + config loader + merger" integration story — proving the
//! cascade resolver is actually wired into the production CLI
//! pipeline, not just exposed as a standalone function.

use std::fs;

use dry_core::adapters::config::load_config;
use dry_core::cli::{Args, Format, ThresholdMode, build_command, merge_effective_inputs};
use dry4rs::DRY4RS_META;

/// Build an [`Args`] via the production `build_command + from_matches`
/// pipeline. argv does NOT include argv0 — we prepend the tool name
/// here.
fn parse_args(argv: &[&str]) -> Args {
    let full: Vec<&str> = std::iter::once(DRY4RS_META.tool_name)
        .chain(argv.iter().copied())
        .collect();
    let matches = build_command(&DRY4RS_META)
        .try_get_matches_from(full)
        .expect("argv parses cleanly");
    Args::from_matches(&matches).expect("from_matches succeeds")
}

#[test]
fn rust_section_threshold_overrides_shared_gate_threshold() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg_path = tmp.path().join(DRY4RS_META.config_file_name);
    fs::write(
        &cfg_path,
        b"[gate]\nthreshold = 0.85\n\n[rust]\nthreshold = 0.90\n",
    )
    .expect("write config");
    let cfg = load_config(&cfg_path).expect("parse config");
    let args = parse_args(&["report"]);

    let analysis = merge_effective_inputs(&DRY4RS_META, Some(&cfg), &args);
    assert!(
        (analysis.threshold - 0.90).abs() < f64::EPSILON,
        "rust override should shadow shared gate threshold; got: {}",
        analysis.threshold
    );
}

#[test]
fn shared_gate_threshold_applies_when_rust_unset() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg_path = tmp.path().join(DRY4RS_META.config_file_name);
    fs::write(&cfg_path, b"[gate]\nthreshold = 0.85\n").expect("write config");
    let cfg = load_config(&cfg_path).expect("parse config");
    let args = parse_args(&["report"]);

    let analysis = merge_effective_inputs(&DRY4RS_META, Some(&cfg), &args);
    assert!(
        (analysis.threshold - 0.85).abs() < f64::EPSILON,
        "shared gate threshold should apply when [rust].threshold unset; got: {}",
        analysis.threshold
    );
}

#[test]
fn cli_threshold_overrides_rust_section() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg_path = tmp.path().join(DRY4RS_META.config_file_name);
    fs::write(&cfg_path, b"[rust]\nthreshold = 0.90\n").expect("write config");
    let cfg = load_config(&cfg_path).expect("parse config");
    let args = parse_args(&["--threshold", "0.95", "report"]);

    let analysis = merge_effective_inputs(&DRY4RS_META, Some(&cfg), &args);
    assert!(
        (analysis.threshold - 0.95).abs() < f64::EPSILON,
        "CLI --threshold should override [rust] cascade; got: {}",
        analysis.threshold
    );
}

#[test]
fn typescript_section_does_not_affect_rust_adapter() {
    // dry4rs reads [rust]; [typescript] values must not leak into
    // the resolved cascade for the rust adapter.
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg_path = tmp.path().join(DRY4RS_META.config_file_name);
    fs::write(
        &cfg_path,
        b"[typescript]\nthreshold = 0.50\nformat = \"json\"\n",
    )
    .expect("write config");
    let cfg = load_config(&cfg_path).expect("parse config");
    let args = parse_args(&["report"]);

    let analysis = merge_effective_inputs(&DRY4RS_META, Some(&cfg), &args);
    // Falls back to compiled-in default (0.85).
    assert!(
        (analysis.threshold - 0.85).abs() < f64::EPSILON,
        "typescript override must not leak into rust adapter; got: {}",
        analysis.threshold
    );
    assert_eq!(
        analysis.format,
        Format::Text,
        "typescript override must not leak into rust adapter"
    );
}

#[test]
fn rust_threshold_mode_overrides_shared_gate_mode() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg_path = tmp.path().join(DRY4RS_META.config_file_name);
    fs::write(
        &cfg_path,
        b"[gate]\nthreshold_mode = \"default\"\n\n[rust]\nthreshold_mode = \"strict\"\n",
    )
    .expect("write config");
    let cfg = load_config(&cfg_path).expect("parse config");
    let args = parse_args(&["report"]);

    let analysis = merge_effective_inputs(&DRY4RS_META, Some(&cfg), &args);
    assert_eq!(
        analysis.threshold_mode,
        ThresholdMode::Strict,
        "[rust].threshold_mode should override shared [gate].threshold_mode"
    );
}

#[test]
fn output_title_and_subtitle_cascade_into_analysis_config() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg_path = tmp.path().join(DRY4RS_META.config_file_name);
    fs::write(
        &cfg_path,
        b"[output]\ntitle = \"Shared Title\"\nsubtitle = \"Shared Subtitle\"\n",
    )
    .expect("write config");
    let cfg = load_config(&cfg_path).expect("parse config");
    let args = parse_args(&["report"]);

    let analysis = merge_effective_inputs(&DRY4RS_META, Some(&cfg), &args);
    assert_eq!(
        analysis.title.as_deref(),
        Some("Shared Title"),
        "[output].title should flow into AnalysisConfig.title"
    );
    assert_eq!(
        analysis.subtitle.as_deref(),
        Some("Shared Subtitle"),
        "[output].subtitle should flow into AnalysisConfig.subtitle"
    );
}

#[test]
fn rust_title_overrides_output_title() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg_path = tmp.path().join(DRY4RS_META.config_file_name);
    fs::write(
        &cfg_path,
        b"[output]\ntitle = \"Shared\"\n\n[rust]\ntitle = \"Rust Override\"\n",
    )
    .expect("write config");
    let cfg = load_config(&cfg_path).expect("parse config");
    let args = parse_args(&["report"]);

    let analysis = merge_effective_inputs(&DRY4RS_META, Some(&cfg), &args);
    assert_eq!(
        analysis.title.as_deref(),
        Some("Rust Override"),
        "[rust].title should shadow [output].title for the rust adapter"
    );
}

#[test]
fn dogfood_dry_toml_shape_matches_compiled_in_defaults() {
    // Regression for the dogfood migration: dry.toml carries
    // `[rust] extensions = ["rs"]` so the cascade parser runs on
    // the production CI surface, but the effective AnalysisConfig
    // MUST match what bare-binary defaults produced before the
    // migration (the dry-self-scorecard / dry-corpus-scorecard
    // behavior must NOT regress).
    //
    // The shape this test mimics: workspace-root dry.toml minus
    // ancestor I/O — we synthesize the same TOML inline so the
    // test is self-contained and doesn't depend on filesystem
    // discovery walking out of the tempdir.
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg_path = tmp.path().join(DRY4RS_META.config_file_name);
    fs::write(
        &cfg_path,
        b"[gate]\nthreshold = 0.85\nthreshold_mode = \"default\"\n\n\
          [output]\nformat = \"text\"\n\n\
          [walk]\ninclude_ignored = false\n\n\
          [rust]\nextensions = [\"rs\"]\n",
    )
    .expect("write config");
    let cfg = load_config(&cfg_path).expect("parse config");
    let args = parse_args(&["report"]);

    let analysis = merge_effective_inputs(&DRY4RS_META, Some(&cfg), &args);
    assert!(
        (analysis.threshold - 0.85).abs() < f64::EPSILON,
        "dogfood threshold must match compiled-in 0.85"
    );
    assert_eq!(
        analysis.format,
        Format::Text,
        "dogfood format must match compiled-in Text"
    );
    assert_eq!(
        analysis.threshold_mode,
        ThresholdMode::Default,
        "dogfood threshold_mode must match compiled-in Default"
    );
    assert!(
        !analysis.include_ignored,
        "dogfood include_ignored must remain false"
    );
    assert_eq!(
        analysis.extensions,
        vec!["rs".to_string()],
        "dogfood [rust] extensions must produce the same `[rs]` filter \
         as the pre-cascade AdapterMeta default"
    );
}
