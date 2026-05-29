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
//!   `"dry4rs.toml"` literal).
//! - `parse_test_args` — added in Stage 4 with the clap rip-out;
//!   internally calls `build_command(&TEST_META).try_get_matches_
//!   from(args)` and constructs `Args::from_matches`. Preserves the
//!   ergonomics of the 38 existing `Args::try_parse_from(["dry4rs",
//!   ...])` test sites without rewriting their assertions.

#![allow(dead_code)]

use dry_core::cli::AdapterMeta;

/// Synthetic adapter meta used across `dry-core` integration tests.
///
/// MUST MATCH `dry4rs::main::DRY4RS_META` in every field that affects
/// clap behavior or downstream consumer semantics, with the sole
/// exception of `config_file_name = "test-adapter.toml"` (chosen so
/// the layer-4 ast-purity gate landing in Stage 3 doesn't trip on
/// `"dry4rs.toml"` literals appearing in `tests/config.rs`).
///
/// `tool_name = "dry4rs"` is fine here — the ast-purity gate covers
/// `tests/config*.rs` only (see [`crates/dry-core/tests/common/
/// mod.rs`] is allowed to use the literal; the gate explicitly
/// excludes this file's directory by listing only the loader-specific
/// test file).
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
    extensions: &["rs"],
    tool_info_uri: "https://github.com/breezy-bays-labs/dry-rs",
    rule_help_uri: "https://github.com/breezy-bays-labs/dry-rs#thresholds",
    default_excludes: &[],
    forced_excludes: &[],
};
