//! Language-agnostic reporters for the dry structural duplication
//! detector.
//!
//! Each reporter is a small free-function module:
//!
//! - [`json::render`] — locked v0.1 wire envelope per
//!   `ops/decisions/dry-rs/adr-nested-json-envelope.md`.
//! - [`text::render`] — human-friendly terminal output grouped by tier.
//! - [`markdown::render`] — GitHub-flavored Markdown grouped by tier
//!   (askama compile-time template), suitable for PR comments / issue
//!   bodies / `report.md` (dry-rs#91).
//! - [`github_annotations::render`] — GitHub Actions workflow commands
//!   (`::error::` / `::warning::` / `::notice::`) so duplications
//!   surface inline on the PR "Files Changed" tab without GHAS / Code
//!   Scanning licensing.
//!
//! No reporter trait at v0.1 — every reporter has the same shape
//! (`render(&Report, ...) -> ...`) but the CLI dispatches via a
//! `Format` enum match in PR 8. Adding a reporter is "new module +
//! enum variant"; a trait abstraction earns its complexity when
//! polymorphism is actually exercised, which it is not here. See
//! `adr-hexagonal-layout.md` §"Three module-roster divergences from
//! scrap-rs".

pub mod github_annotations;
pub mod json;
pub mod markdown;
pub mod text;
