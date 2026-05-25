//! Language-agnostic adapters for the dry structural duplication detector.
//!
//! Houses two free-function adapter families:
//!
//! - [`source::enumerate`] — file walker via the `ignore` crate, with
//!   `--include-ignored` switch for fixtures. Free function (no
//!   polymorphism axis — the same walker serves dry4rs Rust files and
//!   dry4ts TypeScript files via the per-adapter extension filter on
//!   [`crate::cli::AnalysisConfig`]).
//! - [`reporters::{text, json, github_annotations}::render`] — one
//!   free function per format module. The CLI dispatches via a
//!   `Format` enum match in PR 8; no reporter trait. Markdown lands at
//!   v0.2; HTML + SARIF at v0.3 / v0.4 per the roadmap.
//!
//! v0.1 ships text + json + github-annotations. No `Format` enum yet
//! (the CLI flag lives in PR 8); call sites pick the reporter directly
//! during the v0.1 walking-skeleton phase.

pub mod reporters;
pub mod source;
