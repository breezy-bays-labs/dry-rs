//! clap-derive [`Args`] + subcommand enum + value enums for the v0.1
//! CLI surface.
//!
//! Per the dispatch and the cross-tool harmonization doc, the v0.1
//! surface is intentionally minimal: subcommands `report` (implicit
//! default), `stats`, `check`, `ignore <fingerprint>`, `ignored`,
//! `cleanup`; universal flags `--threshold`, `--format`, `--top`,
//! `--only-failing`, `--no-fail`, `--include-ignored`,
//! `--threshold-mode`, `--completions <SHELL>`. Markdown / HTML / SARIF
//! reporters land in later waves and are deliberately rejected by the
//! v0.1 `Format` enum so users get a clear "not yet" message instead
//! of a silent fall-through.
//!
//! The clap derive expansion lives in `dry-core` because the CLI
//! surface is language-agnostic; only `NormalizerPort` differs across
//! adapter binaries.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

/// Output format selector. `--format` accepts the v0.1 subset; reporters
/// for `markdown`, `html`, and `sarif` land in later waves and are
/// deliberately omitted from the value enum so clap rejects them at
/// parse time with an actionable message.
///
/// `#[non_exhaustive]` per the AGENTS.md `#[non_exhaustive]` discipline
/// — enums YES, result structs NO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[non_exhaustive]
pub enum Format {
    /// Human-friendly terminal output (default).
    Text,
    /// Locked v0.1 nested wire envelope (`dry_core::adapters::reporters::json`).
    Json,
}

/// Threshold-mode preset selector. v0.1 accepts the three named
/// presets (`strict` / `default` / `lenient`); v0.2+ may extend with
/// user-configurable labels.
///
/// `#[non_exhaustive]` per the AGENTS.md `#[non_exhaustive]` discipline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[non_exhaustive]
pub enum ThresholdMode {
    /// Higher threshold → fewer findings (high-confidence only).
    Strict,
    /// v0.1 baseline. Aligns with the comparison engine's
    /// `REVIEW_FIRST_FLOOR = 0.85`.
    Default,
    /// Lower threshold → more findings, including advisory tier.
    Lenient,
}

/// v0.1 subcommand enum. `report` is the implicit default when no
/// subcommand is supplied (the dispatch / cross-tool harmonization
/// rule); the other subcommands route to dedicated handlers.
///
/// `ignore` / `ignored` / `cleanup` are SKELETAL at v0.1 — they parse
/// args correctly and surface a "not yet implemented" message; full
/// allowlist UX (`.dry-rs-ignore.toml`) lands at v0.2 per the roadmap.
///
/// `#[non_exhaustive]` per the AGENTS.md discipline.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
#[non_exhaustive]
pub enum Command {
    /// Full duplication report (default — invokable without an explicit
    /// subcommand).
    Report {
        /// Source roots to analyze. Defaults to the current directory
        /// when omitted; see [`super::Args::analysis_paths`].
        paths: Vec<PathBuf>,
    },
    /// Summary statistics only (no per-match output).
    Stats {
        /// Source roots to analyze. Defaults to the current directory
        /// when omitted.
        paths: Vec<PathBuf>,
    },
    /// Exit-code-only mode for CI. Suppresses human-readable output to
    /// stdout; `result.passed` drives the exit code as in `report`.
    Check {
        /// Source roots to analyze. Defaults to the current directory
        /// when omitted.
        paths: Vec<PathBuf>,
    },
    /// Add a fingerprint to the allowlist (v0.1: parses args; full UX
    /// lands at v0.2).
    Ignore {
        /// The fingerprint to silence.
        fingerprint: String,
    },
    /// List current allowlist entries (v0.1: stub).
    Ignored,
    /// Remove stale allowlist entries (v0.1: stub).
    Cleanup,
}

impl Command {
    /// Extract the source roots associated with this subcommand
    /// variant, when applicable.
    ///
    /// `Report` / `Stats` / `Check` carry an analysis path list;
    /// `Ignore` / `Ignored` / `Cleanup` do not (they don't trigger
    /// the analyzer pipeline at v0.1). Returns an empty slice for
    /// the non-analysis variants so callers can treat the absence of
    /// paths as "no walk required."
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        match self {
            Self::Report { paths } | Self::Stats { paths } | Self::Check { paths } => paths,
            Self::Ignore { .. } | Self::Ignored | Self::Cleanup => &[],
        }
    }

    /// Whether this subcommand triggers the analyzer pipeline.
    ///
    /// `Report` / `Stats` / `Check` are analysis commands; `Ignore` /
    /// `Ignored` / `Cleanup` manage the allowlist surface and short-
    /// circuit before the file walker runs.
    #[must_use]
    pub const fn is_analysis(&self) -> bool {
        matches!(
            self,
            Self::Report { .. } | Self::Stats { .. } | Self::Check { .. }
        )
    }
}

/// Top-level CLI argument struct for `dry4rs` (and future `dry4ts` /
/// other adapter binaries).
///
/// Adapter binaries call [`super::run()`] which constructs this via
/// `Args::parse()` internally; tests parse explicitly via
/// `Args::try_parse_from(["dry4rs", "--threshold", "0.9", ...])`.
///
/// Per AGENTS.md, public result/config structs do NOT carry
/// `#[non_exhaustive]` — `Args` evolves via additive `Default` fields
/// and clap's own arg-discovery (added flags are non-breaking for
/// existing call sites).
#[derive(Debug, Clone, Parser)]
#[command(
    name = "dry4rs",
    version,
    about = "Structural duplication detector — finds Jaccard-similar subforms across Rust sources.",
    long_about = "dry-rs detects structural duplication via per-subform fingerprinting + Jaccard \
                  similarity. The default invocation analyzes the current directory and emits a \
                  human-friendly report; subcommands `report`/`stats`/`check` drive output \
                  shape, `ignore`/`ignored`/`cleanup` manage the allowlist. Universal flags \
                  `--top`/`--only-failing` reshape the displayed `view.*` projection; \
                  `result.*` stays unaffected per the truthful-gate ADR."
)]
pub struct Args {
    /// Subcommand to run. Defaults to [`Command::Report`] (with paths
    /// inferred from `--paths` or `.`) when no subcommand is supplied.
    /// Paths go on the subcommand itself (`dry4rs report src/`,
    /// `dry4rs check src/`).
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Jaccard similarity threshold in the half-open interval `(0.0, 1.0]`.
    /// Matches at or above this value surface in the report.
    ///
    /// The default `0.85` matches the comparison engine's
    /// [`crate::comparison::REVIEW_FIRST_FLOOR`] — v0.1 surfaces
    /// `review_first` / `auto_refactor` by default; users opt into the
    /// advisory tier with a lower threshold.
    ///
    /// Out-of-range values (`<= 0.0` or `> 1.0`) reject at parse time
    /// with `ExitCode::from(2)` (clap's standard argument-error exit).
    #[arg(long, global = true, default_value_t = 0.85, value_parser = parse_threshold)]
    pub threshold: f64,

    /// Output format. v0.1: `text` (default) or `json`; markdown / html /
    /// sarif land in later waves.
    #[arg(long, global = true, value_enum, default_value_t = Format::Text)]
    pub format: Format,

    /// Threshold-mode preset (`strict` / `default` / `lenient`).
    /// Currently informational at v0.1 — the preset is recorded on the
    /// wire envelope's `threshold_mode` field; numeric override stays
    /// the truthful gate.
    #[arg(long, global = true, value_enum, default_value_t = ThresholdMode::Default)]
    pub threshold_mode: ThresholdMode,

    /// Limit `view.candidates` to the top N matches by descending
    /// score. **View-shaping only** — `result.*` stays unaffected per
    /// the truthful-gate ADR. CI parsers reading `result.passed` are
    /// immune to this flag.
    #[arg(long, global = true, value_name = "N")]
    pub top: Option<u32>,

    /// Filter `view.*` to matches that exceed the threshold gate.
    /// **View-shaping only** — `result.*` stays unaffected.
    #[arg(long, global = true)]
    pub only_failing: bool,

    /// Suppress non-zero exit code when findings exceed the threshold.
    /// `result.passed` remains authoritative in JSON output; only the
    /// process exit code changes. Useful for advisory CI integration.
    #[arg(long, global = true)]
    pub no_fail: bool,

    /// Walk files normally excluded by `.gitignore` / `.ignore`.
    /// Intended for fixture corpora that live inside ignored directories;
    /// production usage stays at the default (`false`).
    #[arg(long, global = true)]
    pub include_ignored: bool,

    /// Generate a shell-completion script for the named shell and exit
    /// 0. When set, the analyzer pipeline is NOT invoked; the script
    /// goes to stdout and the process exits immediately. Useful for
    /// shell init files (`source <(dry4rs --completions bash)`).
    #[arg(long, global = true, value_name = "SHELL")]
    pub completions: Option<Shell>,
}

impl Args {
    /// Return the analysis paths the user requested, defaulting to the
    /// current directory when none were provided. The caller decides
    /// whether the active subcommand uses paths (e.g., `report` /
    /// `stats` / `check` do; `ignored` / `cleanup` do not — see
    /// [`Command::paths`]).
    ///
    /// Resolution order:
    /// 1. Non-analysis subcommand (`ignore` / `ignored` / `cleanup`) →
    ///    empty vec; callers should short-circuit before walking files.
    /// 2. Subcommand-attached paths (`dry4rs report src/`, `dry4rs check src/`)
    /// 3. Default — current directory `.` (used when no subcommand is
    ///    given, or when an analysis subcommand is invoked without
    ///    explicit paths).
    #[must_use]
    pub fn analysis_paths(&self) -> Vec<PathBuf> {
        if let Some(cmd) = &self.command {
            if !cmd.is_analysis() {
                return Vec::new();
            }
            let cmd_paths = cmd.paths();
            if !cmd_paths.is_empty() {
                return cmd_paths.to_vec();
            }
        }
        vec![PathBuf::from(".")]
    }
}

/// Custom clap value parser for `--threshold`.
///
/// Accepts any `f64` in the half-open interval `(0.0, 1.0]` (the
/// comparison engine's domain). Out-of-range values reject at parse
/// time so the comparison engine never receives a degenerate threshold.
fn parse_threshold(s: &str) -> Result<f64, String> {
    let value: f64 = s
        .parse()
        .map_err(|err| format!("threshold must be a decimal number: {err}"))?;
    if value.is_nan() || value <= 0.0 || value > 1.0 {
        return Err(format!(
            "threshold must lie in the half-open interval (0.0, 1.0]; got {value}"
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_threshold_accepts_in_range_values() {
        assert!((parse_threshold("0.5").unwrap() - 0.5).abs() < f64::EPSILON);
        assert!((parse_threshold("1.0").unwrap() - 1.0).abs() < f64::EPSILON);
        // Smallest positive
        let small = parse_threshold("0.001").unwrap();
        assert!(small > 0.0 && small < 0.01);
    }

    #[test]
    fn parse_threshold_rejects_zero() {
        let err = parse_threshold("0.0").expect_err("zero must reject");
        assert!(err.contains("(0.0, 1.0]"), "msg: {err}");
    }

    #[test]
    fn parse_threshold_rejects_negative() {
        let err = parse_threshold("-0.1").expect_err("negative must reject");
        assert!(err.contains("(0.0, 1.0]"), "msg: {err}");
    }

    #[test]
    fn parse_threshold_rejects_above_one() {
        let err = parse_threshold("1.5").expect_err("> 1.0 must reject");
        assert!(err.contains("(0.0, 1.0]"), "msg: {err}");
    }

    #[test]
    fn parse_threshold_rejects_nan() {
        let err = parse_threshold("NaN").expect_err("NaN must reject");
        // The error message format depends on whether NaN parses as
        // f64::NAN (which it does — `"NaN".parse::<f64>()` succeeds) or
        // bails out at the parse step. Either way, the rejection
        // message must mention the threshold range.
        assert!(
            err.contains("(0.0, 1.0]") || err.contains("decimal"),
            "msg: {err}"
        );
    }

    #[test]
    fn parse_threshold_rejects_non_numeric() {
        let err = parse_threshold("not-a-number").expect_err("non-numeric must reject");
        assert!(err.contains("decimal"), "msg: {err}");
    }

    #[test]
    fn args_verifies_clap_invariants() {
        // clap's `debug_assert!` checks the command/arg invariants;
        // running it once at compile-test time surfaces any wiring
        // mistakes (duplicate args, missing default_value_t types,
        // value-parser type mismatches) before they hit users.
        use clap::CommandFactory;
        Args::command().debug_assert();
    }
}
