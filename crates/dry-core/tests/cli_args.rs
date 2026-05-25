//! CLI argument-parsing tests for `dry_core::cli::Args`.
//!
//! These tests exercise clap's derive output via `Args::try_parse_from`,
//! which is the canonical TDD entry point — it avoids spawning a real
//! binary and lets us assert on parsed values. The full
//! `run<N: NormalizerPort + Default>()` pipeline has its own
//! integration test (`tests/cli_pipeline.rs`); these tests cover the
//! parse layer only.

use std::path::PathBuf;

use clap::Parser;
use dry_core::cli::{Args, Command, Format, ThresholdMode};

#[test]
fn parses_with_no_subcommand_runs_default_report() {
    let args = Args::try_parse_from(["dry4rs"]).expect("must parse with no subcommand");
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
    let args = Args::try_parse_from(["dry4rs", "report"]).expect("report subcommand must parse");
    assert!(matches!(args.command, Some(Command::Report { .. })));
}

#[test]
fn parses_with_stats_subcommand() {
    let args = Args::try_parse_from(["dry4rs", "stats"]).expect("stats subcommand must parse");
    assert!(matches!(args.command, Some(Command::Stats { .. })));
}

#[test]
fn parses_with_check_subcommand() {
    let args = Args::try_parse_from(["dry4rs", "check"]).expect("check subcommand must parse");
    assert!(matches!(args.command, Some(Command::Check { .. })));
}

#[test]
fn parses_with_ignore_subcommand_carrying_fingerprint() {
    let args = Args::try_parse_from(["dry4rs", "ignore", "deadbeef"])
        .expect("ignore subcommand must parse with fingerprint argument");
    match args.command {
        Some(Command::Ignore { fingerprint }) => assert_eq!(fingerprint, "deadbeef"),
        other => panic!("expected Command::Ignore, got {other:?}"),
    }
}

#[test]
fn parses_with_ignored_subcommand() {
    let args = Args::try_parse_from(["dry4rs", "ignored"]).expect("ignored subcommand must parse");
    assert!(matches!(args.command, Some(Command::Ignored)));
}

#[test]
fn parses_with_cleanup_subcommand() {
    let args = Args::try_parse_from(["dry4rs", "cleanup"]).expect("cleanup subcommand must parse");
    assert!(matches!(args.command, Some(Command::Cleanup)));
}

#[test]
fn threshold_defaults_to_0_85() {
    // 0.85 mirrors the comparison engine's `REVIEW_FIRST_FLOOR` —
    // the v0.1 default surfaces review_first / auto_refactor by
    // default; users opt into advisory tier with a lower threshold.
    let args = Args::try_parse_from(["dry4rs"]).expect("must parse");
    assert!(
        (args.threshold - 0.85).abs() < f64::EPSILON,
        "default threshold should be 0.85, got: {}",
        args.threshold
    );
}

#[test]
fn threshold_flag_accepts_user_value() {
    let args = Args::try_parse_from(["dry4rs", "--threshold", "0.75"])
        .expect("--threshold accepts a decimal");
    assert!((args.threshold - 0.75).abs() < f64::EPSILON);
}

#[test]
fn threshold_rejects_zero_and_above_one() {
    // The half-open interval (0.0, 1.0] is the comparison engine's
    // domain. clap value-parses to f64 and our value-parser rejects
    // out-of-band values with a non-zero exit.
    let err = Args::try_parse_from(["dry4rs", "--threshold", "0.0"])
        .expect_err("threshold = 0.0 must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("threshold") || msg.contains("0.0"),
        "rejection should mention threshold, got: {msg}"
    );

    let err = Args::try_parse_from(["dry4rs", "--threshold", "1.5"])
        .expect_err("threshold > 1.0 must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("threshold") || msg.contains("1.5"),
        "rejection should mention threshold, got: {msg}"
    );
}

#[test]
fn format_defaults_to_text() {
    let args = Args::try_parse_from(["dry4rs"]).expect("must parse");
    assert_eq!(args.format, Format::Text);
}

#[test]
fn format_flag_accepts_text() {
    let args = Args::try_parse_from(["dry4rs", "--format", "text"]).expect("--format text parses");
    assert_eq!(args.format, Format::Text);
}

#[test]
fn format_flag_accepts_json() {
    let args = Args::try_parse_from(["dry4rs", "--format", "json"]).expect("--format json parses");
    assert_eq!(args.format, Format::Json);
}

#[test]
fn format_flag_rejects_markdown_at_v0_1() {
    // markdown / html / sarif land in later waves; only text + json
    // are valid at v0.1 (per AC).
    let err = Args::try_parse_from(["dry4rs", "--format", "markdown"])
        .expect_err("--format markdown must reject at v0.1");
    let msg = err.to_string();
    assert!(
        msg.contains("invalid value") || msg.contains("possible"),
        "rejection should explain valid values, got: {msg}"
    );
}

#[test]
fn no_fail_flag_is_false_by_default() {
    let args = Args::try_parse_from(["dry4rs"]).expect("must parse");
    assert!(!args.no_fail);
}

#[test]
fn no_fail_flag_is_true_when_set() {
    let args = Args::try_parse_from(["dry4rs", "--no-fail"]).expect("--no-fail parses");
    assert!(args.no_fail);
}

#[test]
fn top_flag_defaults_to_none() {
    let args = Args::try_parse_from(["dry4rs"]).expect("must parse");
    assert!(args.top.is_none());
}

#[test]
fn top_flag_accepts_user_value() {
    let args = Args::try_parse_from(["dry4rs", "--top", "5"]).expect("--top 5 parses");
    assert_eq!(args.top, Some(5));
}

#[test]
fn only_failing_flag_is_false_by_default() {
    let args = Args::try_parse_from(["dry4rs"]).expect("must parse");
    assert!(!args.only_failing);
}

#[test]
fn only_failing_flag_is_true_when_set() {
    let args = Args::try_parse_from(["dry4rs", "--only-failing"]).expect("--only-failing parses");
    assert!(args.only_failing);
}

#[test]
fn include_ignored_flag_is_false_by_default() {
    let args = Args::try_parse_from(["dry4rs"]).expect("must parse");
    assert!(!args.include_ignored);
}

#[test]
fn include_ignored_flag_is_true_when_set() {
    let args =
        Args::try_parse_from(["dry4rs", "--include-ignored"]).expect("--include-ignored parses");
    assert!(args.include_ignored);
}

#[test]
fn threshold_mode_defaults_to_default() {
    let args = Args::try_parse_from(["dry4rs"]).expect("must parse");
    assert_eq!(args.threshold_mode, ThresholdMode::Default);
}

#[test]
fn threshold_mode_accepts_strict() {
    let args = Args::try_parse_from(["dry4rs", "--threshold-mode", "strict"])
        .expect("--threshold-mode strict parses");
    assert_eq!(args.threshold_mode, ThresholdMode::Strict);
}

#[test]
fn threshold_mode_accepts_lenient() {
    let args = Args::try_parse_from(["dry4rs", "--threshold-mode", "lenient"])
        .expect("--threshold-mode lenient parses");
    assert_eq!(args.threshold_mode, ThresholdMode::Lenient);
}

#[test]
fn completions_flag_accepts_known_shells() {
    // `--completions <SHELL>` generates a completion script. Validate
    // that clap_complete's Shell enum is wired in by accepting `bash`.
    let args =
        Args::try_parse_from(["dry4rs", "--completions", "bash"]).expect("--completions parses");
    assert!(args.completions.is_some());
}

#[test]
fn completions_flag_rejects_unknown_shell() {
    let err = Args::try_parse_from(["dry4rs", "--completions", "tcl"])
        .expect_err("--completions tcl must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("invalid value") || msg.contains("possible"),
        "unknown shell should be rejected, got: {msg}"
    );
}

#[test]
fn paths_default_to_current_directory() {
    // Empty paths default to "." so a no-arg run analyzes the cwd.
    let args = Args::try_parse_from(["dry4rs"]).expect("must parse");
    assert_eq!(args.analysis_paths(), vec![PathBuf::from(".")]);
}

#[test]
fn paths_accept_multiple_positional_arguments_via_report_subcommand() {
    // Positional `paths` lives on each analysis-related subcommand
    // (Report / Stats / Check). The implicit-default-Report path
    // can't accept positionals (clap routes them to subcommands), so
    // the explicit `report src/ tests/` form is the way to supply
    // multiple roots in v0.1.
    let args = Args::try_parse_from(["dry4rs", "report", "src/", "tests/"])
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
    let args = Args::try_parse_from(["dry4rs", "check", "src/"]).expect("must parse");
    assert_eq!(args.analysis_paths(), vec![PathBuf::from("src/")]);
}

#[test]
fn analysis_paths_falls_back_to_current_dir_when_no_subcommand_or_paths() {
    let args = Args::try_parse_from(["dry4rs"]).expect("must parse");
    assert_eq!(args.analysis_paths(), vec![PathBuf::from(".")]);
}

#[test]
fn analysis_paths_falls_back_to_current_dir_on_subcommand_without_paths() {
    let args = Args::try_parse_from(["dry4rs", "report"]).expect("must parse");
    assert_eq!(args.analysis_paths(), vec![PathBuf::from(".")]);
}

#[test]
fn check_subcommand_accepts_path_argument() {
    let args =
        Args::try_parse_from(["dry4rs", "check", "src/"]).expect("check subcommand accepts a path");
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
    let err = Args::try_parse_from(["dry4rs", "--help"]).expect_err("--help short-circuits");
    assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
}

#[test]
fn version_flag_does_not_panic() {
    let err = Args::try_parse_from(["dry4rs", "--version"]).expect_err("--version short-circuits");
    assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
}
