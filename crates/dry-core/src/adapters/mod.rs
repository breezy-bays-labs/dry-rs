//! Language-agnostic adapters for the dry structural duplication detector.
//!
//! Houses two free-function adapter families:
//!
//! - `source::enumerate` — file walker via the `ignore` crate, with
//!   `--include-ignored` flag for fixtures. Free function (no
//!   polymorphism — same walker serves dry4rs Rust files and dry4ts
//!   TypeScript files via per-adapter extension filter).
//! - `reporters::{text, json, markdown, html, sarif}::render` — one
//!   free function per format module. CLI dispatches via `Format`
//!   enum match; no reporter trait (testability via direct fixture
//!   inputs is sufficient).
//!
//! v0.1 ships text + json. Markdown lands at v0.2; HTML + SARIF land
//! at v0.3 / v0.4 per the roadmap wave plan.
//!
//! The actual implementations land in PR 7 (language-agnostic
//! adapters).
