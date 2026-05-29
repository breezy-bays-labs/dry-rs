//! CLI surface for the dry structural duplication detector.
//!
//! Houses the clap derive [`Args`] struct, [`AnalysisConfig`], the
//! generic [`run()`] loop, and helpers ([`Format`], [`ThresholdMode`],
//! [`Command`]). Adapter binaries (`dry4rs`, future `dry4ts`) provide
//! a 5-line `main()` that constructs their language-specific
//! normalizer and calls `dry_core::cli::run::<MyNormalizer>()`.
//!
//! v0.1 subcommands: `report` (implicit default), `stats`, `check`,
//! `ignore <fingerprint>`, `ignored`, `cleanup`. Universal flags follow
//! the cross-tool harmonization rules in
//! `ops/workspace/dry-rs/20260508-dry-rs-roadmap/cli-harmonization.md`
//! (deferred past v1.0 of all three sensors for full convergence;
//! dry-rs ships in a way that keeps harmonization tractable).
//!
//! ## Truthful-gate vs shapeable-display
//!
//! Per `ops/decisions/dry-rs/adr-nested-json-envelope.md`, `--top` /
//! `--only-failing` reshape `view.*`; they NEVER mutate `result.*`.
//! `result.passed` is the gate verdict driven by the unfiltered
//! [`crate::domain::Report`]. `--no-fail` suppresses the non-zero
//! exit code but does NOT touch `result.passed`.
//!
//! ## Exit codes
//!
//! - `ExitCode::SUCCESS` — `report.passed == true` OR `--no-fail` set.
//! - `ExitCode::FAILURE` — `report.passed == false` AND `--no-fail` not set.
//! - `ExitCode::from(2)` — argument parse error (clap handles this) or
//!   catastrophic walker error (no roots, fatal I/O before any file
//!   normalizes). Per-file parse errors are diagnostics, not gate
//!   failures.

mod adapter_meta;
mod args;
mod build_command;
mod run;

pub use adapter_meta::AdapterMeta;
pub use args::{Args, Command, Format, ThresholdMode};
pub use build_command::build_command;
pub use run::{
    compute_analysis_root, merge_effective_inputs, render_config_error, resolve_config_path, run,
};

// Test-shim alias — integration tests in
// `crates/dry-core/tests/config.rs` call this name so the helper's
// production name `merge_effective_inputs` stays clearly scoped to
// production use (the test name signals "this is an internal
// helper I'm exercising directly").
#[doc(hidden)]
pub use run::merge_effective_inputs as merge_effective_inputs_for_test;

use std::path::PathBuf;

use crate::domain::FilePath;

/// Default Jaccard similarity threshold — aligns with the comparison
/// engine's [`crate::comparison::REVIEW_FIRST_FLOOR`] (0.85). The
/// [`Args`] clap-derive default matches this constant so the two
/// configuration paths produce the same baseline.
pub const DEFAULT_THRESHOLD: f64 = 0.85;

/// Output-destination selector — where the renderer writes its output.
///
/// v0.1 supports only `Stdout`; the variant exists so the surface
/// stays additive (`--output /path/to/file` lands at v0.2+ alongside
/// the markdown/html reporters per the roadmap).
///
/// `#[non_exhaustive]` per the AGENTS.md `#[non_exhaustive]`
/// discipline — enums YES, result structs NO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OutputDestination {
    /// Write to stdout. The v0.1 default.
    #[default]
    Stdout,
}

/// Full analysis configuration consumed by the v0.1 CLI pipeline.
///
/// This is the load-bearing config struct flowing through
/// [`run()`]: paths and `extensions` feed the file walker
/// ([`crate::adapters::source::enumerate`]); `threshold` feeds the
/// comparison engine; `format` + `output` drive the reporter
/// dispatch; `threshold_mode` lands on the wire envelope's
/// `threshold_mode` metadata field.
///
/// Adapter binaries do NOT construct `AnalysisConfig` directly — the
/// run loop builds it from [`Args`] via clap parse. The struct is
/// public so library callers (e.g. mokumo embedding dry-core
/// programmatically rather than spawning the binary) can drive the
/// pipeline without going through clap.
///
/// The struct deliberately does **NOT** carry `#[non_exhaustive]` —
/// per the wire-envelope ADR's "enums-yes-structs-no" rule, configuration
/// structs evolve via constructors and `Default`. New fields land
/// additively; callers construct via [`AnalysisConfig::new`] (which
/// defaults absent fields) or via the builder chain
/// (`.with_threshold(...)`, `.with_format(...)`, …).
#[derive(Debug, Clone, PartialEq)]
pub struct AnalysisConfig {
    /// Input roots the walker enumerates. Each root is walked
    /// recursively via the `ignore` crate (which honors `.gitignore`,
    /// `.ignore`, and `.git/info/exclude` like `rg` / `fd`).
    pub roots: Vec<FilePath>,
    /// Jaccard similarity threshold in the half-open interval
    /// `(0.0, 1.0]`. Defaults to [`DEFAULT_THRESHOLD`].
    pub threshold: f64,
    /// Output format (text / json at v0.1; markdown / html / sarif
    /// land at v0.2+).
    pub format: Format,
    /// Output destination. v0.1 always writes to stdout; `--output`
    /// lands at v0.2.
    pub output: OutputDestination,
    /// Whitelist of file extensions (without the leading dot —
    /// `"rs"`, `"ts"`, `"tsx"`). When empty, every regular file under
    /// the roots is yielded. Adapter binaries source this from
    /// [`crate::ports::NormalizerPort::extensions`].
    pub extensions: Vec<String>,
    /// Walk files normally excluded by `.gitignore` / `.ignore`.
    /// Intended for fixture corpora that live inside ignored
    /// directories; production usage stays at the default (`false`).
    pub include_ignored: bool,
    /// Threshold-mode preset label (`strict` / `default` / `lenient`).
    /// Currently informational at v0.1 — the preset is recorded on the
    /// wire envelope's `threshold_mode` field; the numeric `threshold`
    /// is the truthful gate.
    pub threshold_mode: ThresholdMode,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            threshold: DEFAULT_THRESHOLD,
            format: Format::Text,
            output: OutputDestination::Stdout,
            extensions: Vec::new(),
            include_ignored: false,
            threshold_mode: ThresholdMode::Default,
        }
    }
}

impl AnalysisConfig {
    /// Construct an [`AnalysisConfig`] over the given roots, with
    /// every other field defaulted (empty extension allowlist,
    /// `include_ignored = false`, threshold = [`DEFAULT_THRESHOLD`],
    /// format = [`Format::Text`], output = stdout, `threshold_mode` =
    /// [`ThresholdMode::Default`]).
    #[must_use]
    pub fn new<I>(roots: I) -> Self
    where
        I: IntoIterator<Item = PathBuf>,
    {
        Self {
            roots: roots.into_iter().map(FilePath::from).collect(),
            ..Self::default()
        }
    }

    /// Replace the extension allowlist; returns `self` for chaining.
    #[must_use]
    pub fn with_extensions<I, S>(mut self, extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.extensions = extensions.into_iter().map(Into::into).collect();
        self
    }

    /// Toggle the `--include-ignored` switch on this config; returns
    /// `self` for chaining.
    #[must_use]
    pub const fn with_include_ignored(mut self, include_ignored: bool) -> Self {
        self.include_ignored = include_ignored;
        self
    }

    /// Set the Jaccard threshold; returns `self` for chaining. Callers
    /// are responsible for keeping `threshold` in the half-open interval
    /// `(0.0, 1.0]`; the clap parser is the production-build
    /// input-validation boundary.
    #[must_use]
    pub const fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold;
        self
    }

    /// Set the output format; returns `self` for chaining.
    #[must_use]
    pub const fn with_format(mut self, format: Format) -> Self {
        self.format = format;
        self
    }

    /// Set the output destination; returns `self` for chaining.
    #[must_use]
    pub const fn with_output(mut self, output: OutputDestination) -> Self {
        self.output = output;
        self
    }

    /// Set the threshold-mode preset; returns `self` for chaining.
    #[must_use]
    pub const fn with_threshold_mode(mut self, threshold_mode: ThresholdMode) -> Self {
        self.threshold_mode = threshold_mode;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{AnalysisConfig, DEFAULT_THRESHOLD, Format, OutputDestination, ThresholdMode};

    #[test]
    fn analysis_config_new_stores_roots_with_defaults() {
        let config = AnalysisConfig::new([std::path::PathBuf::from("src")]);
        assert_eq!(config.roots.len(), 1);
        assert!(config.extensions.is_empty());
        assert!(!config.include_ignored);
        assert!((config.threshold - DEFAULT_THRESHOLD).abs() < f64::EPSILON);
        assert_eq!(config.format, Format::Text);
        assert_eq!(config.output, OutputDestination::Stdout);
        assert_eq!(config.threshold_mode, ThresholdMode::Default);
    }

    #[test]
    fn analysis_config_with_extensions_replaces_allowlist() {
        let config = AnalysisConfig::default().with_extensions(["rs"]);
        assert_eq!(config.extensions, vec!["rs".to_string()]);
    }

    #[test]
    fn analysis_config_with_include_ignored_toggles_field() {
        let config = AnalysisConfig::default().with_include_ignored(true);
        assert!(config.include_ignored);
    }

    #[test]
    fn analysis_config_default_is_empty_with_documented_defaults() {
        let config = AnalysisConfig::default();
        assert!(config.roots.is_empty());
        assert!(config.extensions.is_empty());
        assert!(!config.include_ignored);
        assert!((config.threshold - DEFAULT_THRESHOLD).abs() < f64::EPSILON);
        assert_eq!(config.format, Format::Text);
        assert_eq!(config.output, OutputDestination::Stdout);
        assert_eq!(config.threshold_mode, ThresholdMode::Default);
    }

    #[test]
    fn analysis_config_builder_chain_threads_threshold_format_mode() {
        let config = AnalysisConfig::default()
            .with_threshold(0.75)
            .with_format(Format::Json)
            .with_threshold_mode(ThresholdMode::Strict)
            .with_output(OutputDestination::Stdout);
        assert!((config.threshold - 0.75).abs() < f64::EPSILON);
        assert_eq!(config.format, Format::Json);
        assert_eq!(config.threshold_mode, ThresholdMode::Strict);
        assert_eq!(config.output, OutputDestination::Stdout);
    }

    #[test]
    fn default_threshold_constant_matches_review_first_floor() {
        // Cross-module sanity check: the CLI default threshold aligns
        // with the comparison engine's REVIEW_FIRST_FLOOR. If either
        // moves, the other should be updated in the same PR.
        assert!((DEFAULT_THRESHOLD - crate::comparison::REVIEW_FIRST_FLOOR).abs() < f64::EPSILON);
    }
}
