//! CLI surface for the dry structural duplication detector.
//!
//! Houses the clap derive struct, [`AnalysisConfig`], `ExitCode`, and
//! the generic `run<N: NormalizerPort>` loop. Adapter binaries
//! (`dry4rs`, future `dry4ts`) provide a 5-line `main()` that
//! constructs their language-specific normalizer and calls
//! `dry_core::cli::run::<MyNormalizer>()`.
//!
//! v0.1 subcommands: `report` (default), `stats`, `check`, `ignore`,
//! `ignored`, `cleanup`. Universal flags follow the cross-tool
//! harmonization rules in
//! `ops/workspace/dry-rs/20260508-dry-rs-roadmap/cli-harmonization.md`
//! (deferred past v1.0 of all three sensors for full convergence;
//! dry-rs ships in a way that keeps harmonization tractable).
//!
//! The full clap surface lands in PR 8 (CLI surface). PR 7 ships the
//! placeholder `run()` plus the minimal [`AnalysisConfig`] needed to
//! parameterize the file walker (`adapters::source::enumerate`).

use std::path::PathBuf;
use std::process::ExitCode;

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
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            extensions: Vec::new(),
            include_ignored: false,
        }
    }
}

/// CLI entry point — bootstrap placeholder. Returns `ExitCode::SUCCESS`
/// and prints a single line. The real clap-derive surface (analyzer
/// pipeline + `ExitCode` shaping) lands with the CLI sub-issue (PR 8).
#[must_use]
pub fn run() -> ExitCode {
    println!("dry4rs (skeleton) — see https://github.com/breezy-bays-labs/dry-rs");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{AnalysisConfig, run};

    #[test]
    fn run_returns_without_panic() {
        // Exercises the placeholder body so the coverage gate stays
        // honest at the bootstrap PR. The real `run()` body lands in
        // PR 8 with proper integration coverage.
        let _ = run();
    }

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
