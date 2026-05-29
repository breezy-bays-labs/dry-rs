//! Shared test fixtures for `dry-core` integration tests.
//!
//! Lives in a `common/` subdirectory rather than directly under
//! `tests/` so cargo doesn't compile it as its own test binary
//! (canonical pattern; cargo treats `tests/<subdir>/` as private
//! shared modules). Each consumer integration test does
//! `mod common;` at the top and `use common::{TEST_META, ...};`.
//!
//! Carries:
//! - [`TEST_META`] — synthetic [`AdapterMeta`] mirroring DRY4RS_META
//!   in behavior-affecting fields. The ONLY divergence is
//!   `config_file_name = "test-adapter.toml"` (ast-purity discipline
//!   per ADR D7 — `tests/config*.rs` MUST NOT contain
//!   `"dry.toml"` literal).
//! - `parse_test_args` — added in Stage 4 with the clap rip-out;
//!   internally calls `build_command(&TEST_META).try_get_matches_
//!   from(args)` and constructs `Args::from_matches`. Preserves the
//!   ergonomics of the 38 existing `Args::try_parse_from(["dry4rs",
//!   ...])` test sites without rewriting their assertions.

#![allow(dead_code)]

use dry_core::cli::{AdapterMeta, Args, Language, build_command};

/// Build a [`clap::Command`] over [`TEST_META`] and parse the
/// supplied argv into an [`Args`] instance.
///
/// The argv slice does NOT include a leading binary name —
/// `parse_test_args(&["report", "src/"])` is sufficient, mirroring
/// the ergonomics of the old `Args::try_parse_from(["dry4rs",
/// "report", "src/"])` sites. Internally prepends
/// `TEST_META.tool_name` so clap sees a well-formed argv0.
///
/// Routes through the SAME `build_command + from_matches` pipeline
/// the production binary uses (Stage 5+6 of dry-rs#71), so these
/// tests accurately exercise the production CLI machinery — no
/// `#[cfg(test)] pub` shim on `Args` (per CEng H1).
///
/// # Errors
///
/// Returns the underlying clap error (unparseable flag, missing
/// required arg, unknown subcommand, help/version short-circuit).
pub fn parse_test_args(args: &[&str]) -> Result<Args, clap::Error> {
    // Prepend the synthetic binary name so clap sees a well-formed
    // argv0. The mechanical rewrite of cli_args.rs from
    // `Args::try_parse_from(["dry4rs", ...])` → `parse_test_args
    // ([...])` drops the "dry4rs" literal at the call site; this
    // helper supplies it from `TEST_META`.
    let argv: Vec<&str> = std::iter::once(TEST_META.tool_name)
        .chain(args.iter().copied())
        .collect();
    let matches = build_command(&TEST_META).try_get_matches_from(argv)?;
    Args::from_matches(&matches)
}

/// Synthetic adapter meta used across `dry-core` integration tests.
///
/// MUST MATCH `dry4rs::main::DRY4RS_META` in every field that
/// affects clap behavior or downstream consumer semantics, with the
/// sole exception of `config_file_name = "test-adapter.toml"`
/// (chosen so the layer-4 ast-purity gate landing in Stage 3 doesn't
/// trip on `"dry.toml"` literals appearing in `tests/config.rs`).
///
/// `tool_name = "dry4rs"` is fine here — the ast-purity gate covers
/// `crates/dry-core/src/adapters/config.rs` +
/// `crates/dry-core/tests/config.rs` only. This module's path
/// (`crates/dry-core/tests/common/mod.rs`) is excluded by the gate's
/// hardcoded FILES allowlist; the gate also excludes any line whose
/// first non-whitespace chars are `//` (doc comments, line comments).
pub const TEST_META: AdapterMeta = AdapterMeta {
    tool_name: "dry4rs",
    display_name: "Rust",
    tool_version: "0.0.0",
    long_version: "0.0.0",
    about: "Structural duplication detector — finds Jaccard-similar subforms across Rust sources.",
    long_about: "dry-rs detects structural duplication via per-subform fingerprinting + Jaccard \
                  similarity. The default invocation analyzes the current directory and emits a \
                  human-friendly report; subcommands `report`/`stats`/`check` drive output \
                  shape, `ignore`/`ignored`/`cleanup` manage the allowlist. Universal flags \
                  `--top`/`--only-failing` reshape the displayed `view.*` projection; \
                  `result.*` stays unaffected per the truthful-gate ADR.",
    after_help: "",
    config_file_name: "test-adapter.toml",
    example_file_name: "test-adapter.example.toml",
    schema_file_name: "test-adapter.schema.json",
    extensions: &["rs"],
    language: Language::Rust,
    tool_info_uri: "https://github.com/breezy-bays-labs/dry-rs",
    rule_help_uri: "https://github.com/breezy-bays-labs/dry-rs#thresholds",
    default_excludes: &[],
    forced_excludes: &[],
};
