//! `dry4rs` binary entry point.
//!
//! Per the hexagonal layering ADR, the CLI surface is
//! language-agnostic; only the [`SynNormalizer`] parser adapter
//! differs from `dry4ts` (and future adapters). This file stays a
//! 5-line entry that hands off to `dry_core::cli::run::<SynNormalizer>()`.

use std::process::ExitCode;

use dry4rs::parser::SynNormalizer;

fn main() -> ExitCode {
    dry_core::cli::run::<SynNormalizer>()
}
