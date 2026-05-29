//! `dry4rs` binary entry point.
//!
//! Per the hexagonal layering ADR, the CLI surface is language-
//! agnostic; only the [`SynNormalizer`] parser adapter differs from
//! `dry4ts` (and future adapters). The binary declares its
//! [`AdapterMeta`] const and hands off to
//! `dry_core::cli::run::<SynNormalizer>(&DRY4RS_META)`.

use std::process::ExitCode;

use dry_core::cli::AdapterMeta;
use dry4rs::parser::SynNormalizer;

/// dry4rs's [`AdapterMeta`] — supplies binary identity (`tool_name`,
/// version, help text), the config-file name the loader walks for
/// (`dry-rs.toml`), the default extension allowlist (`["rs"]`), and
/// the documentation URIs.
///
/// `long_about` text matches the previous clap-derive
/// `long_about = "..."` literal so `--help` output is preserved
/// byte-for-byte through the Stage 5 + Stage 6 migration.
const DRY4RS_META: AdapterMeta = AdapterMeta {
    tool_name: "dry4rs",
    display_name: "Rust",
    tool_version: env!("CARGO_PKG_VERSION"),
    long_version: env!("CARGO_PKG_VERSION"),
    about: "Structural duplication detector — finds Jaccard-similar subforms across Rust sources.",
    long_about: "dry-rs detects structural duplication via per-subform fingerprinting + Jaccard \
                  similarity. The default invocation analyzes the current directory and emits a \
                  human-friendly report; subcommands `report`/`stats`/`check` drive output \
                  shape, `ignore`/`ignored`/`cleanup` manage the allowlist. Universal flags \
                  `--top`/`--only-failing` reshape the displayed `view.*` projection; \
                  `result.*` stays unaffected per the truthful-gate ADR.",
    after_help: "",
    config_file_name: "dry-rs.toml",
    extensions: &["rs"],
    tool_info_uri: "https://github.com/breezy-bays-labs/dry-rs",
    rule_help_uri: "https://github.com/breezy-bays-labs/dry-rs#thresholds",
    default_excludes: &[],
    forced_excludes: &[],
};

fn main() -> ExitCode {
    dry_core::cli::run::<SynNormalizer>(&DRY4RS_META)
}
