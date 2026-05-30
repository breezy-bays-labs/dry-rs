//! clap-derive [`Args`] + subcommand enum + value enums for the v0.1
//! CLI surface.
//!
//! Per the dispatch and the cross-tool harmonization doc, the v0.1
//! surface is intentionally minimal: subcommands `report` (implicit
//! default), `stats`, `check`, `ignore <fingerprint>`, `ignored`,
//! `cleanup`; universal flags `--threshold`, `--format`, `--top`,
//! `--only-failing`, `--no-fail`, `--include-ignored`,
//! `--threshold-mode`, `--completions <SHELL>`. The `markdown` reporter
//! joined the `Format` enum at v0.2 (dry-rs#91); HTML / SARIF land in
//! later waves and are still deliberately rejected by the `Format`
//! enum so users get a clear "not yet" message instead of a silent
//! fall-through.
//!
//! The clap derive expansion lives in `dry-core` because the CLI
//! surface is language-agnostic; only `NormalizerPort` differs across
//! adapter binaries.

use std::path::PathBuf;

use clap::ValueEnum;
use clap_complete::Shell;
use serde::{Deserialize, Serialize};

/// Output format selector. `--format` accepts `text` / `json` /
/// `markdown`; `html` and `sarif` land in later waves and are
/// deliberately omitted from the value enum so clap rejects them at
/// parse time with an actionable message.
///
/// `#[non_exhaustive]` per the AGENTS.md `#[non_exhaustive]` discipline
/// — enums YES, result structs NO.
///
/// Serde uses lowercase tags so TOML config files can use `format =
/// "text"` / `format = "json"` / `format = "markdown"` symmetrically
/// with the CLI flag.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Format {
    /// Human-friendly terminal output (default).
    Text,
    /// Locked v0.1 nested wire envelope (`dry_core::adapters::reporters::json`).
    Json,
    /// GitHub-flavored Markdown grouped by tier
    /// (`dry_core::adapters::reporters::markdown`, dry-rs#91) — suitable
    /// for PR comments, issue bodies, or `report.md`.
    Markdown,
}

/// Threshold-mode preset selector. v0.1 accepts the three named
/// presets (`strict` / `default` / `lenient`); v0.2+ may extend with
/// user-configurable labels.
///
/// `#[non_exhaustive]` per the AGENTS.md `#[non_exhaustive]` discipline.
///
/// Serde uses lowercase tags so TOML config files can use
/// `threshold_mode = "strict"` symmetrically with the CLI flag.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
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
///
/// `#[derive(Subcommand)]` was REMOVED at Stage 5 of dry-rs#71 —
/// the imperative `build_command` constructs the subcommand
/// structure directly. `Args::from_matches` produces this enum from
/// parsed `clap::ArgMatches`.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Write the fully-annotated `<tool>.example.toml` reference into
    /// the current directory (Starship-pattern doc-gen, dry-rs#77).
    ///
    /// Emits [`crate::adapters::config_doc_gen::render_example_config`]
    /// to `<cwd>/<AdapterMeta::example_file_name>`. Errors if the file
    /// exists unless `force` is true.
    Init {
        /// Overwrite the example file if it already exists.
        force: bool,
    },
}

impl Command {
    /// Extract the source roots associated with this subcommand
    /// variant, when applicable.
    ///
    /// `Report` / `Stats` / `Check` carry an analysis path list;
    /// `Ignore` / `Ignored` / `Cleanup` / `Init` do not (they don't
    /// trigger the analyzer pipeline at v0.1). Returns an empty slice
    /// for the non-analysis variants so callers can treat the absence
    /// of paths as "no walk required."
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        match self {
            Self::Report { paths } | Self::Stats { paths } | Self::Check { paths } => paths,
            Self::Ignore { .. } | Self::Ignored | Self::Cleanup | Self::Init { .. } => &[],
        }
    }

    /// Whether this subcommand triggers the analyzer pipeline.
    ///
    /// `Report` / `Stats` / `Check` are analysis commands; `Ignore` /
    /// `Ignored` / `Cleanup` manage the allowlist surface; `Init`
    /// emits the example config — all four short-circuit before the
    /// file walker runs.
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
/// Adapter binaries call [`super::run()`] with their `&AdapterMeta`
/// const; `run()` invokes [`super::build_command()`] to construct the
/// `clap::Command`, parses argv, and hydrates this struct via
/// [`Args::from_matches`]. Tests parse via `common::parse_test_args`
/// (in `crates/dry-core/tests/common/mod.rs`), which routes through
/// the same pipeline.
///
/// Per AGENTS.md, public result/config structs do NOT carry
/// `#[non_exhaustive]` — `Args` evolves via additive fields and
/// `Args::from_matches`'s constructor pattern (added flags are
/// non-breaking for existing call sites — `from_matches` reads any
/// new field via `matches.get_one`).
///
/// `#[derive(Parser)]` was REMOVED at Stage 5 of dry-rs#71. The
/// imperative `build_command(meta) -> clap::Command` is the only
/// parser construction path; `Args` is now a pure POD struct.
#[derive(Debug, Clone)]
pub struct Args {
    /// Subcommand to run. Defaults to [`Command::Report`] (with paths
    /// inferred from positional args or `.`) when no subcommand is
    /// supplied. Paths go on the subcommand itself (`dry4rs report
    /// src/`, `dry4rs check src/`).
    pub command: Option<Command>,

    /// Jaccard similarity threshold in the half-open interval
    /// `(0.0, 1.0]`. Matches at or above this value surface in the
    /// report.
    ///
    /// `None` when the user did NOT pass `--threshold` (the
    /// precedence merger then consults `[gate] threshold` from
    /// `dry.toml`, falling back to the compiled-in default
    /// [`crate::comparison::REVIEW_FIRST_FLOOR`] = 0.85). `Some(t)`
    /// is the user-supplied value; CLI > config > meta default.
    ///
    /// Out-of-range values (`<= 0.0` or `> 1.0`) reject at parse
    /// time with `ExitCode::from(2)` (clap's standard argument-error
    /// exit).
    pub threshold: Option<f64>,

    /// Output format. v0.1: `text` (default) or `json`; markdown /
    /// html / sarif land in later waves.
    ///
    /// `None` when the user did NOT pass `--format` (the precedence
    /// merger then consults `[output] format` from `dry.toml`,
    /// falling back to `Format::Text`).
    pub format: Option<Format>,

    /// Threshold-mode preset (`strict` / `default` / `lenient`).
    /// Currently informational at v0.1 — the preset is recorded on
    /// the wire envelope's `threshold_mode` field; numeric override
    /// stays the truthful gate.
    ///
    /// `None` when the user did NOT pass `--threshold-mode` (the
    /// precedence merger then consults `[gate] threshold_mode`,
    /// falling back to `ThresholdMode::Default`).
    pub threshold_mode: Option<ThresholdMode>,

    /// Limit `view.candidates` to the top N matches by descending
    /// score. **View-shaping only** — `result.*` stays unaffected
    /// per the truthful-gate ADR. CI parsers reading `result.passed`
    /// are immune to this flag.
    pub top: Option<u32>,

    /// Filter `view.*` to matches that exceed the threshold gate.
    /// **View-shaping only** — `result.*` stays unaffected.
    pub only_failing: bool,

    /// Suppress non-zero exit code when findings exceed the
    /// threshold. `result.passed` remains authoritative in JSON
    /// output; only the process exit code changes. Useful for
    /// advisory CI integration.
    pub no_fail: bool,

    /// Walk files normally excluded by `.gitignore` / `.ignore`.
    /// Intended for fixture corpora that live inside ignored
    /// directories; production usage stays at the default (`false`).
    pub include_ignored: bool,

    /// Generate a shell-completion script for the named shell and
    /// exit 0. When set, the analyzer pipeline is NOT invoked; the
    /// script goes to stdout and the process exits immediately.
    /// Useful for shell init files (`source <(dry4rs --completions
    /// bash)`).
    pub completions: Option<Shell>,

    /// Path to an explicit `dry.toml` config file (bypasses auto-
    /// discovery). When set, the path MUST exist — missing path
    /// produces `ConfigError::Io` at startup. When unset, the loader
    /// auto-discovers a `dry.toml` by walking upward from the
    /// analysis-root (per `org/adr-config-file-pattern.md` D2).
    pub config: Option<PathBuf>,
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

    /// Construct an [`Args`] from an already-parsed [`clap::ArgMatches`].
    ///
    /// The companion to [`super::build_command()`] — together they form
    /// the imperative pipeline `build_command(meta) ->
    /// get_matches() -> from_matches() -> Args`. The production
    /// binary (`dry4rs::main` at Stage 6) and the test fixture
    /// (`crates/dry-core/tests/common/mod.rs::parse_test_args`) both
    /// route through this pair.
    ///
    /// Stage 4 lands this method additively; the existing
    /// `#[derive(Parser)]` keeps working until Stage 5's atomic
    /// rip-out. Field extraction below MUST stay in lockstep with the
    /// `Arg::new(...)` declarations in `build_command`.
    ///
    /// # Errors
    ///
    /// Returns the clap error verbatim when subcommand-arg extraction
    /// fails. Top-level flag extraction uses `get_one::<T>` /
    /// `get_flag` which cannot fail on a well-formed
    /// [`build_command`][bc] output (the value-parser machinery already
    /// validated types at `get_matches` time).
    ///
    /// [bc]: super::build_command()
    pub fn from_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        // `--threshold` / `--format` / `--threshold-mode` produce
        // `Option<T>` — absence means "let the precedence merger
        // consult [gate]/[output] from dry.toml" (per ADR D3).
        // The compiled-in defaults (0.85 / Format::Text /
        // ThresholdMode::Default) apply ONLY when neither CLI nor
        // config supplied a value.
        let threshold = matches.get_one::<f64>("threshold").copied();
        let format = matches.get_one::<Format>("format").copied();
        let threshold_mode = matches.get_one::<ThresholdMode>("threshold_mode").copied();
        let top = matches.get_one::<u32>("top").copied();
        let only_failing = matches.get_flag("only_failing");
        let no_fail = matches.get_flag("no_fail");
        let include_ignored = matches.get_flag("include_ignored");
        let completions = matches.get_one::<Shell>("completions").copied();
        let config = matches.get_one::<PathBuf>("config").cloned();

        let command = match matches.subcommand() {
            Some(("report", sub)) => Some(Command::Report {
                paths: sub
                    .get_many::<PathBuf>("paths")
                    .map(|vals| vals.cloned().collect())
                    .unwrap_or_default(),
            }),
            Some(("stats", sub)) => Some(Command::Stats {
                paths: sub
                    .get_many::<PathBuf>("paths")
                    .map(|vals| vals.cloned().collect())
                    .unwrap_or_default(),
            }),
            Some(("check", sub)) => Some(Command::Check {
                paths: sub
                    .get_many::<PathBuf>("paths")
                    .map(|vals| vals.cloned().collect())
                    .unwrap_or_default(),
            }),
            Some(("ignore", sub)) => Some(Command::Ignore {
                fingerprint: sub
                    .get_one::<String>("fingerprint")
                    .ok_or_else(|| {
                        clap::Error::raw(
                            clap::error::ErrorKind::MissingRequiredArgument,
                            "ignore: missing fingerprint argument",
                        )
                    })?
                    .clone(),
            }),
            Some(("ignored", _)) => Some(Command::Ignored),
            Some(("cleanup", _)) => Some(Command::Cleanup),
            Some(("init", sub)) => Some(Command::Init {
                force: sub.get_flag("force"),
            }),
            Some((other, _)) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::InvalidSubcommand,
                    format!("unknown subcommand: {other}"),
                ));
            }
            None => None,
        };

        Ok(Self {
            command,
            threshold,
            format,
            threshold_mode,
            top,
            only_failing,
            no_fail,
            include_ignored,
            completions,
            config,
        })
    }
}

/// Custom clap value parser for `--threshold`.
///
/// Accepts any `f64` in the half-open interval `(0.0, 1.0]` (the
/// comparison engine's domain). Out-of-range values reject at parse
/// time so the comparison engine never receives a degenerate threshold.
///
/// `pub(crate)` so `build_command` can reuse the parser when
/// constructing the imperative `clap::Command`.
pub(crate) fn parse_threshold(s: &str) -> Result<f64, String> {
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
    fn build_command_verifies_clap_invariants() {
        // clap's `debug_assert!` checks the command/arg invariants;
        // running it once at compile-test time surfaces any wiring
        // mistakes (duplicate args, missing default_value parsing,
        // value-parser type mismatches) before they hit users.
        //
        // Stage 5 of dry-rs#71 replaced the clap-derive
        // `Args::command()` entry point with an imperative
        // `build_command(meta)` — this test now goes through the
        // production builder + a synthetic AdapterMeta from the
        // common test fixture (inline here to avoid pulling
        // tests/common/mod.rs into the unit-test scope).
        use crate::cli::{AdapterMeta, Language, build_command};
        const FIXTURE_META: AdapterMeta = AdapterMeta {
            tool_name: "test-adapter",
            display_name: "TestLang",
            tool_version: "0.0.0",
            long_version: "0.0.0",
            about: "test about",
            long_about: "test long about",
            after_help: "",
            config_file_name: "test-adapter.toml",
            example_file_name: "test-adapter.example.toml",
            schema_file_name: "test-adapter.schema.json",
            extensions: &["x"],
            language: Language::Rust,
            tool_info_uri: "https://example.test/info",
            rule_help_uri: "https://example.test/rules",
            default_excludes: &[],
            forced_excludes: &[],
        };
        build_command(&FIXTURE_META).debug_assert();
    }
}
