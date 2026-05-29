//! `dry4rs` binary entry point.
//!
//! Per the hexagonal layering ADR, the CLI surface is language-
//! agnostic; only the [`SynNormalizer`] parser adapter differs from
//! `dry4ts` (and future adapters). The binary hands off to
//! `dry_core::cli::run::<SynNormalizer>(&dry4rs::DRY4RS_META)` —
//! the meta const lives in the lib crate so the sync test in
//! `tests/dry_example_sync.rs` can consume it.

use std::process::ExitCode;

use dry4rs::DRY4RS_META;
use dry4rs::parser::SynNormalizer;

fn main() -> ExitCode {
    dry_core::cli::run::<SynNormalizer>(&DRY4RS_META)
}
