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

mod args;
mod run;

pub use args::{Args, Command, Format, ThresholdMode};
pub use run::run;

use std::path::PathBuf;

use crate::domain::FilePath;

/// Minimal configuration consumed by the file walker
/// ([`crate::adapters::source::enumerate`]) and the comparison-engine
/// run loop.
///
/// **v0.1 surface**: only the fields the walker actually needs land
/// here. PR 8's clap-derive layer extends this struct with
/// `--threshold`, `--format`, `--top`, `--color`, etc. Adding fields
/// to `AnalysisConfig` is purely additive — callers construct via
/// [`AnalysisConfig::new`] (which defaults absent fields).
///
/// The struct deliberately does **NOT** carry `#[non_exhaustive]` —
/// per the wire-envelope ADR's "enums-yes-structs-no" rule, configuration
/// structs evolve via constructors and `Default`. This keeps
/// hand-construction in tests cheap (`AnalysisConfig::new(roots)`
/// without `..Default::default()` ceremony).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnalysisConfig {
    /// Input roots the walker enumerates. Each root is walked
    /// recursively via the `ignore` crate (which honors `.gitignore`,
    /// `.ignore`, and `.git/info/exclude` like `rg` / `fd`).
    pub roots: Vec<FilePath>,
    /// Whitelist of file extensions (without the leading dot —
    /// `"rs"`, `"ts"`, `"tsx"`). When empty, every regular file under
    /// the roots is yielded. Adapter binaries source this from
    /// [`crate::ports::NormalizerPort::extensions`].
    pub extensions: Vec<String>,
    /// Walk files normally excluded by `.gitignore` / `.ignore`.
    /// Intended for fixture corpora that live inside ignored
    /// directories; production usage stays at the default (`false`).
    pub include_ignored: bool,
}

impl AnalysisConfig {
    /// Construct an [`AnalysisConfig`] over the given roots, with
    /// every other field defaulted (empty extension allowlist,
    /// `include_ignored = false`).
    #[must_use]
    pub fn new<I>(roots: I) -> Self
    where
        I: IntoIterator<Item = PathBuf>,
    {
        Self {
            roots: roots.into_iter().map(FilePath::from).collect(),
            extensions: Vec::new(),
            include_ignored: false,
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
}

#[cfg(test)]
mod tests {
    use super::AnalysisConfig;

    #[test]
    fn analysis_config_new_stores_roots_with_defaults() {
        let config = AnalysisConfig::new([std::path::PathBuf::from("src")]);
        assert_eq!(config.roots.len(), 1);
        assert!(config.extensions.is_empty());
        assert!(!config.include_ignored);
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
    fn analysis_config_default_is_empty() {
        let config = AnalysisConfig::default();
        assert!(config.roots.is_empty());
        assert!(config.extensions.is_empty());
        assert!(!config.include_ignored);
    }
}
