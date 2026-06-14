//! CLI argument-parsing tests for `dry_core::cli::Args`.
//!
//! These tests exercise the production CLI machinery via
//! `common::parse_test_args` — which routes through
//! `build_command(&TEST_META).try_get_matches_from(...)` +
//! `Args::from_matches(&matches)`. That is the SAME pipeline the
//! production binary uses (`dry4rs::main` at Stage 6 of dry-rs#71),
//! so these tests accurately cover the production parser without
//! spawning a real binary.
//!
//! The full `run<N: NormalizerPort + Default>()` pipeline has its
//! own integration test (`tests/cli_pipeline.rs`); these tests
//! cover the parse layer only.
//!
//! Per the per-tool ADR V3 (`ops/decisions/dry-rs/adr-dry4rs-config-
//! file.md`), this file is INTENTIONALLY EXCLUDED from the layer-4
//! ast-purity gate — `parse_test_args` internally prepends
//! `TEST_META.tool_name` (== `"dry4rs"`) so clap sees a well-formed
//! argv0. The literal lives in `tests/common/mod.rs` (also outside
//! the gate's scope by virtue of being in a subdirectory).

mod common;

use std::path::PathBuf;

use common::parse_test_args;
use dry_core::cli::{Command, Format, ThresholdMode};

#[test]
fn parses_with_no_subcommand_runs_default_report() {
    let args = parse_test_args(&[]).expect("must parse with no subcommand");
    // Per the discovery decision (`report` is the implicit default
    // matching the prior-art convention from crap4rs/scrap-rs).
    assert!(
        matches!(args.command, None | Some(Command::Report { .. })),
        "default invocation should resolve to Command::Report, got: {:?}",
        args.command
    );
}

#[test]
fn parses_with_explicit_report_subcommand() {
    let args = parse_test_args(&["report"]).expect("report subcommand must parse");
    assert!(matches!(args.command, Some(Command::Report { .. })));
}

#[test]
fn parses_with_explore_subcommand() {
    let args = parse_test_args(&["explore"]).expect("explore subcommand must parse");
    assert!(matches!(args.command, Some(Command::Explore { .. })));
    // explore IS an analysis command and defaults paths to "." when none
    // are supplied (same contract as report / stats / check).
    assert_eq!(args.analysis_paths(), vec![PathBuf::from(".")]);
}

#[test]
fn parses_explore_subcommand_with_paths_and_no_open() {
    let args = parse_test_args(&["explore", "--no-open", "src/"])
        .expect("explore subcommand accepts a path + --no-open");
    assert!(args.no_open, "--no-open must set the no_open flag");
    match args.command {
        Some(Command::Explore { paths }) => {
            assert_eq!(paths, vec![PathBuf::from("src/")]);
        }
        other => panic!("expected Explore with paths, got {other:?}"),
    }
}

#[test]
fn no_open_flag_defaults_to_false() {
    // Absent `--no-open`, the flag is false (the browser-open is gated only
    // by the explore path itself + the $DRY_NO_OPEN env escape).
    let args = parse_test_args(&["report"]).expect("must parse");
    assert!(
        !args.no_open,
        "no_open must default to false without the flag"
    );
}

#[test]
fn parses_with_stats_subcommand() {
    let args = parse_test_args(&["stats"]).expect("stats subcommand must parse");
    assert!(matches!(args.command, Some(Command::Stats { .. })));
}

#[test]
fn parses_with_check_subcommand() {
    let args = parse_test_args(&["check"]).expect("check subcommand must parse");
    assert!(matches!(args.command, Some(Command::Check { .. })));
}

#[test]
fn parses_with_ignore_subcommand_carrying_fingerprint() {
    let args = parse_test_args(&["ignore", "deadbeef"])
        .expect("ignore subcommand must parse with fingerprint argument");
    match args.command {
        Some(Command::Ignore { fingerprint }) => assert_eq!(fingerprint, "deadbeef"),
        other => panic!("expected Command::Ignore, got {other:?}"),
    }
}

#[test]
fn parses_with_ignored_subcommand() {
    let args = parse_test_args(&["ignored"]).expect("ignored subcommand must parse");
    assert!(matches!(args.command, Some(Command::Ignored)));
}

#[test]
fn parses_with_cleanup_subcommand() {
    let args = parse_test_args(&["cleanup"]).expect("cleanup subcommand must parse");
    assert!(matches!(args.command, Some(Command::Cleanup)));
}

#[test]
fn threshold_defaults_to_none_when_cli_unset() {
    // The compiled-in default (0.85, `REVIEW_FIRST_FLOOR`) is
    // applied by `merge_effective_inputs` ONLY when neither CLI
    // nor config supplies a value. At the Args layer, absence ==
    // None — the precedence merger sees the None and consults the
    // config / compiled-in fallback chain. See
    // crates/dry-core/tests/config.rs precedence:: tests for the
    // end-to-end behaviour.
    let args = parse_test_args(&[]).expect("must parse");
    assert!(
        args.threshold.is_none(),
        "default invocation should produce threshold = None, got: {:?}",
        args.threshold
    );
}

#[test]
fn threshold_flag_accepts_user_value() {
    let args = parse_test_args(&["--threshold", "0.75"]).expect("--threshold accepts a decimal");
    let t = args.threshold.expect("--threshold sets the field");
    assert!((t - 0.75).abs() < f64::EPSILON);
}

#[test]
fn threshold_rejects_zero_and_above_one() {
    // The half-open interval (0.0, 1.0] is the comparison engine's
    // domain. clap value-parses to f64 and our value-parser rejects
    // out-of-band values with a non-zero exit.
    let err = parse_test_args(&["--threshold", "0.0"]).expect_err("threshold = 0.0 must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("threshold") || msg.contains("0.0"),
        "rejection should mention threshold, got: {msg}"
    );

    let err = parse_test_args(&["--threshold", "1.5"]).expect_err("threshold > 1.0 must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("threshold") || msg.contains("1.5"),
        "rejection should mention threshold, got: {msg}"
    );
}

#[test]
fn format_defaults_to_none_when_cli_unset() {
    // Same Option-as-precedence-input pattern as --threshold:
    // absence at the Args layer is None; the merger applies
    // Format::Text when nothing else supplies it.
    let args = parse_test_args(&[]).expect("must parse");
    assert!(args.format.is_none());
}

#[test]
fn format_flag_accepts_text() {
    let args = parse_test_args(&["--format", "text"]).expect("--format text parses");
    assert_eq!(args.format, Some(Format::Text));
}

#[test]
fn format_flag_accepts_json() {
    let args = parse_test_args(&["--format", "json"]).expect("--format json parses");
    assert_eq!(args.format, Some(Format::Json));
}

#[test]
fn format_flag_accepts_markdown() {
    // markdown joined the `Format` enum at v0.2 (dry-rs#91).
    let args = parse_test_args(&["--format", "markdown"]).expect("--format markdown parses");
    assert_eq!(args.format, Some(Format::Markdown));
}

#[test]
fn format_flag_accepts_html() {
    // html joined the `Format` enum at PR13 (dry-rs#147) — the
    // self-contained single-file HTML explorer reporter.
    let args = parse_test_args(&["--format", "html"]).expect("--format html parses");
    assert_eq!(args.format, Some(Format::Html));
}

#[test]
fn format_flag_rejects_sarif_until_later_wave() {
    // sarif lands in a later wave; text + json + markdown + html are the
    // valid `--format` values now. A still-unsupported value must reject
    // with an actionable "possible values" message rather than silently
    // falling through.
    let err = parse_test_args(&["--format", "sarif"])
        .expect_err("--format sarif must reject until its reporter lands");
    let msg = err.to_string();
    assert!(
        msg.contains("invalid value") || msg.contains("possible"),
        "rejection should explain valid values, got: {msg}"
    );
}

#[test]
fn no_fail_flag_is_false_by_default() {
    let args = parse_test_args(&[]).expect("must parse");
    assert!(!args.no_fail);
}

#[test]
fn no_fail_flag_is_true_when_set() {
    let args = parse_test_args(&["--no-fail"]).expect("--no-fail parses");
    assert!(args.no_fail);
}

#[test]
fn top_flag_defaults_to_none() {
    let args = parse_test_args(&[]).expect("must parse");
    assert!(args.top.is_none());
}

#[test]
fn top_flag_accepts_user_value() {
    let args = parse_test_args(&["--top", "5"]).expect("--top 5 parses");
    assert_eq!(args.top, Some(5));
}

#[test]
fn only_failing_flag_is_false_by_default() {
    let args = parse_test_args(&[]).expect("must parse");
    assert!(!args.only_failing);
}

#[test]
fn only_failing_flag_is_true_when_set() {
    let args = parse_test_args(&["--only-failing"]).expect("--only-failing parses");
    assert!(args.only_failing);
}

#[test]
fn include_ignored_flag_is_false_by_default() {
    let args = parse_test_args(&[]).expect("must parse");
    assert!(!args.include_ignored);
}

#[test]
fn include_ignored_flag_is_true_when_set() {
    let args = parse_test_args(&["--include-ignored"]).expect("--include-ignored parses");
    assert!(args.include_ignored);
}

#[test]
fn threshold_mode_defaults_to_none_when_cli_unset() {
    // Same Option-as-precedence-input pattern: merger applies
    // ThresholdMode::Default when CLI nor config supplies one.
    let args = parse_test_args(&[]).expect("must parse");
    assert!(args.threshold_mode.is_none());
}

#[test]
fn threshold_mode_accepts_strict() {
    let args =
        parse_test_args(&["--threshold-mode", "strict"]).expect("--threshold-mode strict parses");
    assert_eq!(args.threshold_mode, Some(ThresholdMode::Strict));
}

#[test]
fn threshold_mode_accepts_lenient() {
    let args =
        parse_test_args(&["--threshold-mode", "lenient"]).expect("--threshold-mode lenient parses");
    assert_eq!(args.threshold_mode, Some(ThresholdMode::Lenient));
}

// =============================================================================
// dry-rs#142 — paired scope flags (`--[no-]within-crate` etc.)
// =============================================================================
//
// Each axis is a tri-state `Option<bool>`: `--within-crate` -> Some(true),
// `--no-within-crate` -> Some(false), neither -> None (the precedence
// merger then consults [scope]/[rust] in dry.toml, falling back to true).
// NO clap default — a default would mask the config tier (the
// clap-defaults-mask rule).

#[test]
fn scope_flags_default_to_none_when_cli_unset() {
    let args = parse_test_args(&[]).expect("must parse");
    assert_eq!(args.within_crate, None);
    assert_eq!(args.across_crate, None);
    assert_eq!(args.within_module, None);
    assert_eq!(args.across_module, None);
}

#[test]
fn within_crate_flag_sets_some_true() {
    let args = parse_test_args(&["--within-crate"]).expect("--within-crate parses");
    assert_eq!(args.within_crate, Some(true));
}

#[test]
fn no_within_crate_flag_sets_some_false() {
    let args = parse_test_args(&["--no-within-crate"]).expect("--no-within-crate parses");
    assert_eq!(args.within_crate, Some(false));
}

#[test]
fn across_crate_flag_sets_some_true() {
    let args = parse_test_args(&["--across-crate"]).expect("--across-crate parses");
    assert_eq!(args.across_crate, Some(true));
}

#[test]
fn no_across_crate_flag_sets_some_false() {
    let args = parse_test_args(&["--no-across-crate"]).expect("--no-across-crate parses");
    assert_eq!(args.across_crate, Some(false));
}

#[test]
fn within_module_flag_sets_some_true() {
    let args = parse_test_args(&["--within-module"]).expect("--within-module parses");
    assert_eq!(args.within_module, Some(true));
}

#[test]
fn no_within_module_flag_sets_some_false() {
    let args = parse_test_args(&["--no-within-module"]).expect("--no-within-module parses");
    assert_eq!(args.within_module, Some(false));
}

#[test]
fn across_module_flag_sets_some_true() {
    let args = parse_test_args(&["--across-module"]).expect("--across-module parses");
    assert_eq!(args.across_module, Some(true));
}

#[test]
fn no_across_module_flag_sets_some_false() {
    let args = parse_test_args(&["--no-across-module"]).expect("--no-across-module parses");
    assert_eq!(args.across_module, Some(false));
}

#[test]
fn paired_scope_flag_last_one_wins() {
    // `overrides_with` makes the later flag on the command line win when
    // both members of a pair are supplied. `--within-crate
    // --no-within-crate` resolves to the negative (last specified).
    let args = parse_test_args(&["--within-crate", "--no-within-crate"])
        .expect("conflicting pair parses (last wins)");
    assert_eq!(args.within_crate, Some(false));

    let args = parse_test_args(&["--no-within-crate", "--within-crate"])
        .expect("conflicting pair parses (last wins)");
    assert_eq!(args.within_crate, Some(true));
}

#[test]
fn completions_flag_accepts_known_shells() {
    // `--completions <SHELL>` generates a completion script. Validate
    // that clap_complete's Shell enum is wired in by accepting `bash`.
    let args = parse_test_args(&["--completions", "bash"]).expect("--completions parses");
    assert!(args.completions.is_some());
}

#[test]
fn completions_flag_rejects_unknown_shell() {
    let err =
        parse_test_args(&["--completions", "tcl"]).expect_err("--completions tcl must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("invalid value") || msg.contains("possible"),
        "unknown shell should be rejected, got: {msg}"
    );
}

#[test]
fn paths_default_to_current_directory() {
    // Empty paths default to "." so a no-arg run analyzes the cwd.
    let args = parse_test_args(&[]).expect("must parse");
    assert_eq!(args.analysis_paths(), vec![PathBuf::from(".")]);
}

#[test]
fn paths_accept_multiple_positional_arguments_via_report_subcommand() {
    // Positional `paths` lives on each analysis-related subcommand
    // (Report / Stats / Check). The implicit-default-Report path
    // can't accept positionals (clap routes them to subcommands), so
    // the explicit `report src/ tests/` form is the way to supply
    // multiple roots in v0.1.
    let args = parse_test_args(&["report", "src/", "tests/"])
        .expect("multiple positional paths parse on `report`");
    match args.command {
        Some(Command::Report { paths }) => {
            assert_eq!(paths, vec![PathBuf::from("src/"), PathBuf::from("tests/")]);
        }
        other => panic!("expected Report with paths, got {other:?}"),
    }
}

#[test]
fn analysis_paths_returns_subcommand_paths_when_set() {
    let args = parse_test_args(&["check", "src/"]).expect("must parse");
    assert_eq!(args.analysis_paths(), vec![PathBuf::from("src/")]);
}

#[test]
fn analysis_paths_falls_back_to_current_dir_when_no_subcommand_or_paths() {
    let args = parse_test_args(&[]).expect("must parse");
    assert_eq!(args.analysis_paths(), vec![PathBuf::from(".")]);
}

#[test]
fn analysis_paths_falls_back_to_current_dir_on_subcommand_without_paths() {
    let args = parse_test_args(&["report"]).expect("must parse");
    assert_eq!(args.analysis_paths(), vec![PathBuf::from(".")]);
}

#[test]
fn analysis_paths_returns_empty_for_non_analysis_subcommands() {
    // `Ignore` / `Ignored` / `Cleanup` short-circuit before the
    // analyzer runs (allowlist-management surface, no file walk).
    // `analysis_paths()` must NOT silently default to `.` for them —
    // a future caller that forgets the short-circuit would otherwise
    // start walking the cwd unexpectedly.
    for argv in [vec!["ignore", "deadbeef"], vec!["ignored"], vec!["cleanup"]] {
        let args = parse_test_args(&argv).expect("must parse");
        assert_eq!(
            args.analysis_paths(),
            Vec::<PathBuf>::new(),
            "non-analysis subcommand {argv:?} must return empty analysis paths"
        );
    }
}

#[test]
fn check_subcommand_accepts_path_argument() {
    let args = parse_test_args(&["check", "src/"]).expect("check subcommand accepts a path");
    match args.command {
        Some(Command::Check { paths }) => {
            assert_eq!(paths, vec![PathBuf::from("src/")]);
        }
        other => panic!("expected Check with paths, got {other:?}"),
    }
}

#[test]
fn help_flag_does_not_panic() {
    // clap's auto-derived `--help` exits with kind=DisplayHelp; we
    // just need to verify that asking for help doesn't trigger a
    // panic and the resulting error is recognizable.
    let err = parse_test_args(&["--help"]).expect_err("--help short-circuits");
    assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
}

#[test]
fn version_flag_does_not_panic() {
    let err = parse_test_args(&["--version"]).expect_err("--version short-circuits");
    assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
}
