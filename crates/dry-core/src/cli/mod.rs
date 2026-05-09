//! CLI surface for the dry structural duplication detector.
//!
//! Houses the clap derive struct, `AnalysisConfig`, `ExitCode`, and
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
//! The full clap surface lands in PR 8 (CLI surface). The bootstrap
//! ships a placeholder so the workspace builds and `cargo run -p
//! dry4rs` produces non-empty output.

use std::process::ExitCode;

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
    use super::run;

    #[test]
    fn run_returns_without_panic() {
        // Exercises the placeholder body so the coverage gate stays
        // honest at the bootstrap PR. The real `run()` body lands in
        // PR 8 with proper integration coverage.
        let _ = run();
    }
}
