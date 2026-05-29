//! `dry4rs` — Rust-source adapter for the dry structural duplication
//! detector.
//!
//! Owns the [`parser`] module — the syn-based
//! [`dry_core::ports::NormalizerPort`] implementation. Domain types,
//! port traits, comparison engine, reporters, file walker, and the
//! entire CLI surface live in [`dry_core`]; this crate provides only
//! what is genuinely Rust-source-specific (the [`syn`] AST walk and
//! the typed-placeholder fingerprinting rule set).
//!
//! For consumer convenience the `dry_core` modules are re-exported
//! here, so downstream code that wants the full analyzer surface can
//! depend on `dry4rs` alone. This makes `dry4rs`'s public API a
//! strict superset of `dry_core`'s — every module added to `dry_core`
//! becomes immediately public on `dry4rs`. Re-exported types are
//! identical (no newtype wrap): `dry4rs::domain::Match` and
//! `dry_core::domain::Match` are the same type.

#![warn(missing_docs)]
#![warn(clippy::pedantic, clippy::cargo)]

pub mod parser;

pub use dry_core::{adapters, cli, comparison, domain, ports};

use dry_core::cli::{AdapterMeta, Language};

/// dry4rs's [`AdapterMeta`] — supplies binary identity (`tool_name`,
/// version, help text), the config-file name the loader walks for
/// (`dry.toml`), the example-file name `dry4rs init` writes
/// (`dry.example.toml`, dry-rs#77), the JSON schema artifact name
/// (`dry.schema.json`, dry-rs#78), the default extension allowlist
/// (`["rs"]`), the [`Language::Rust`] cascade selector, and the
/// documentation URIs.
///
/// Exposed at lib level (instead of being a private const on
/// `main.rs`) so the sync test in `crates/dry4rs/tests/` can compare
/// the committed `dry.example.toml` to fresh emitter output for the
/// production meta.
pub const DRY4RS_META: AdapterMeta = AdapterMeta {
    tool_name: "dry4rs",
    display_name: "Rust",
    tool_version: env!("CARGO_PKG_VERSION"),
    long_version: env!("CARGO_PKG_VERSION"),
    about: "Structural duplication detector — finds Jaccard-similar subforms across Rust sources.",
    long_about: "dry-rs detects structural duplication via per-subform fingerprinting + Jaccard \
                  similarity. The default invocation analyzes the current directory and emits a \
                  human-friendly report; subcommands `report`/`stats`/`check` drive output \
                  shape, `ignore`/`ignored`/`cleanup` manage the allowlist, `init` writes the \
                  annotated example config + JSON schema. Universal flags `--top`/`--only-failing` \
                  reshape the displayed `view.*` projection; `result.*` stays unaffected per the \
                  truthful-gate ADR.",
    after_help: "",
    config_file_name: "dry.toml",
    example_file_name: "dry.example.toml",
    schema_file_name: "dry.schema.json",
    extensions: &["rs"],
    language: Language::Rust,
    tool_info_uri: "https://github.com/breezy-bays-labs/dry-rs",
    rule_help_uri: "https://github.com/breezy-bays-labs/dry-rs#thresholds",
    default_excludes: &[],
    forced_excludes: &[],
};

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_compiles() {
        assert!(env!("CARGO_PKG_VERSION").starts_with("0."));
    }
}
