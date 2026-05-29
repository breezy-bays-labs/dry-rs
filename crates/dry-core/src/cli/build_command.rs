//! Imperative [`clap::Command`] construction parameterized over an
//! [`AdapterMeta`].
//!
//! Stage 5 of dry-rs#71 finishes the migration started here by
//! removing `#[derive(Parser)]` from [`Args`] and routing the
//! production binary through `build_command(&DRY4RS_META) +
//! Args::from_matches`. Stage 4 lands the imperative builder
//! additively — the derive stays on [`Args`] at this stage so
//! `run::<N>()`'s existing `Args::parse()` + `Args::command()`
//! call sites keep compiling.
//!
//! Per cross-tool ADR D1 (`ops/decisions/org/adr-config-file-
//! pattern.md`), each adapter binary supplies its own
//! [`AdapterMeta`] const at startup; this function turns that
//! struct into a fully-constructed `clap::Command` mirroring the
//! existing v0.1 CLI surface PLUS the new `--config:
//! Option<PathBuf>` flag (per the `CEng` audit revision B1).
//!
//! [`AdapterMeta`]: crate::cli::AdapterMeta
//! [`Args`]: crate::cli::Args

use std::path::PathBuf;

use clap::{Arg, ArgAction, Command, value_parser};
use clap_complete::Shell;

use super::AdapterMeta;
use super::args::{Format, ThresholdMode, parse_threshold};

/// Build the top-level [`clap::Command`] for an adapter binary.
///
/// The returned command mirrors the existing clap-derive-produced
/// `Args::command()` surface (every flag, every subcommand, every
/// value-parser) PLUS the new `--config: Option<PathBuf>` flag.
/// `meta` supplies the binary name, version strings, about / help
/// texts.
///
/// # Examples
///
/// ```ignore
/// // production binary:
/// const META: AdapterMeta = AdapterMeta { /* ... */ };
/// let cmd = dry_core::cli::build_command(&META);
/// let matches = cmd.get_matches();
/// let args = dry_core::cli::Args::from_matches(&matches)?;
/// ```
// The function is a single declarative builder chain; splitting it
// across helpers obscures the 1:1 mapping with the existing Args
// derive surface. The clippy::too_many_lines budget (100 lines)
// would force artificial decomposition.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn build_command(meta: &AdapterMeta) -> Command {
    Command::new(meta.tool_name)
        .version(meta.tool_version)
        .long_version(meta.long_version)
        .about(meta.about)
        .long_about(meta.long_about)
        .after_help(meta.after_help)
        // Global universal flags. Each `.global(true)` lets the flag
        // appear before OR after the subcommand on the CLI.
        // No `default_value` on `--threshold` / `--format` /
        // `--threshold-mode` — absence-on-CLI means "let the
        // precedence merger consult [gate]/[output] from
        // dry-rs.toml" (per ADR D3). The compiled-in defaults
        // (0.85 / text / default) live in
        // `dry_core::cli::run::merge_effective_inputs`, applied
        // ONLY when neither CLI nor config supplied a value.
        .arg(
            Arg::new("threshold")
                .long("threshold")
                .global(true)
                .value_parser(parse_threshold)
                .help("Jaccard similarity threshold in the half-open interval (0.0, 1.0]; defaults to 0.85 when neither CLI nor [gate] in dry-rs.toml supplies one"),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .global(true)
                .value_parser(value_parser!(Format))
                .help("Output format (`text` or `json`); defaults to text"),
        )
        .arg(
            Arg::new("threshold_mode")
                .long("threshold-mode")
                .global(true)
                .value_parser(value_parser!(ThresholdMode))
                .help("Threshold-mode preset (`strict` / `default` / `lenient`); defaults to default"),
        )
        .arg(
            Arg::new("top")
                .long("top")
                .global(true)
                .value_name("N")
                .value_parser(value_parser!(u32))
                .help("Limit `view.candidates` to the top N matches"),
        )
        .arg(
            Arg::new("only_failing")
                .long("only-failing")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Filter `view.*` to matches that exceed the threshold gate"),
        )
        .arg(
            Arg::new("no_fail")
                .long("no-fail")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Suppress non-zero exit code when findings exceed the threshold"),
        )
        .arg(
            Arg::new("include_ignored")
                .long("include-ignored")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Walk files normally excluded by .gitignore / .ignore"),
        )
        .arg(
            Arg::new("completions")
                .long("completions")
                .global(true)
                .value_name("SHELL")
                .value_parser(value_parser!(Shell))
                .help("Generate a shell-completion script for the named shell"),
        )
        // New at dry-rs#71: explicit config-file path. Missing-is-error
        // when set (per ADR D2 — explicit `--config` MUST exist).
        .arg(
            Arg::new("config")
                .long("config")
                .global(true)
                .value_name("PATH")
                .value_parser(value_parser!(PathBuf))
                .action(ArgAction::Set)
                .help(
                    "Path to dry-rs.toml; bypasses auto-discovery (missing-is-error)",
                ),
        )
        .subcommand(subcommand_with_paths(
            "report",
            "Full duplication report (default — invokable without an explicit subcommand)",
        ))
        .subcommand(subcommand_with_paths(
            "stats",
            "Summary statistics only (no per-match output)",
        ))
        .subcommand(subcommand_with_paths(
            "check",
            "Exit-code-only mode for CI. Suppresses human-readable output to stdout; `result.passed` drives the exit code as in `report`",
        ))
        .subcommand(
            Command::new("ignore")
                .about("Add a fingerprint to the allowlist (v0.1: parses args; full UX lands at v0.2)")
                .arg(
                    Arg::new("fingerprint")
                        .required(true)
                        .help("The fingerprint to silence"),
                ),
        )
        .subcommand(Command::new("ignored").about("List current allowlist entries (v0.1: stub)"))
        .subcommand(Command::new("cleanup").about("Remove stale allowlist entries (v0.1: stub)"))
}

/// Build a `report` / `stats` / `check` subcommand carrying an
/// optional positional `paths` argument.
fn subcommand_with_paths(name: &'static str, about: &'static str) -> Command {
    Command::new(name).about(about).arg(
        Arg::new("paths")
            .num_args(0..)
            .value_parser(value_parser!(PathBuf))
            .help("Source roots to analyze (defaults to current directory when omitted)"),
    )
}
