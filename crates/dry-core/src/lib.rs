//! `dry-core` — language-agnostic library for the dry structural
//! duplication detector ecosystem.
//!
//! Houses the domain types, port traits, comparison engine,
//! language-agnostic adapters (file walker, reporters), and the CLI
//! surface (clap derive, `AnalysisConfig`, `ExitCode`, generic run
//! loop). Every adapter binary in the workspace — `dry4rs` for Rust
//! source via `syn`, `dry4ts` for TypeScript source via `swc`/`oxc`,
//! future adapters for additional source languages — links against
//! this crate and provides only its language-specific parser adapter.
//!
//! Per the hexagonal layering ADR
//! (`ops/decisions/dry-rs/adr-hexagonal-layout.md`, filed in PR 2),
//! this crate must never depend on an AST library. `syn`, `swc_*`,
//! `oxc_*`, `tree-sitter*`, `proc-macro2`, and `quote` are banned
//! from this crate's source. The ban is enforced structurally
//! (`Cargo.toml` does not list any AST library, so a wrong `use`
//! line cannot resolve) and via a source-level `ast-purity` CI grep
//! plus the matching `lefthook` pre-push hook.
//!
//! Module roster:
//! - [`domain`] — core types: `NormalizedForm`, `Fingerprint`, `Match`, `Score`, `Span`, `FilePath`, `FormKind`, `Tier`, `Severity`.
//! - [`ports`] — `NormalizerPort` trait + per-port error enums (file enumeration and reporters are free functions, not traits).
//! - [`comparison`] — single comparison engine: hash-bucket clustering for exact matches, sliding-window Jaccard for near-duplicates.
//! - [`adapters`] — language-agnostic adapters: file walker (free function), reporters (free functions per format module).
//! - [`cli`] — CLI surface: clap derive struct, `AnalysisConfig`, `ExitCode`, generic run loop.
//!
//! The single comparison module replaces scrap-rs's parallel
//! `core/` + `detectors/` split — dry-rs has one algorithm
//! (Jaccard on subform fingerprints), not a detector taxonomy.

#![warn(missing_docs)]
#![warn(clippy::pedantic, clippy::cargo)]

pub mod adapters;
pub mod cli;
pub mod comparison;
pub mod domain;
pub mod ports;

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_compiles() {
        // Bootstrap smoke test. Real domain tests live in
        // `crates/dry-core/tests/` and the per-module
        // `#[cfg(test)] mod tests` blocks under `domain/`.
        assert!(env!("CARGO_PKG_VERSION").starts_with("0."));
    }
}
