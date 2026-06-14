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
        // dry.toml" (per ADR D3). The compiled-in defaults
        // (0.85 / text / default) live in
        // `dry_core::cli::run::merge_effective_inputs`, applied
        // ONLY when neither CLI nor config supplied a value.
        .arg(
            Arg::new("threshold")
                .long("threshold")
                .global(true)
                .value_parser(parse_threshold)
                .help("Jaccard similarity threshold in the half-open interval (0.0, 1.0]; defaults to 0.85 when neither CLI nor [gate] in dry.toml supplies one"),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .global(true)
                .value_parser(value_parser!(Format))
                .help("Output format (`text` / `json` / `markdown` / `html`); defaults to text"),
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
                    "Path to dry.toml; bypasses auto-discovery (missing-is-error)",
                ),
        )
        // New at dry-rs#151: suppress the browser launch on the `explore`
        // path. `global(true)` so it may appear before OR after the
        // subcommand; a no-op for every command except `explore`. The
        // run loop also honors the `$DRY_NO_OPEN` env escape (CI-safe).
        .arg(
            Arg::new("no_open")
                .long("no-open")
                .global(true)
                .action(ArgAction::SetTrue)
                .help(
                    "Do not open the generated HTML in a browser (explore only); the temp file is still written. Also honored via $DRY_NO_OPEN",
                ),
        )
        // New at dry-rs#142: paired relatedness-scoping flags. Each axis
        // is a tri-state `--<axis>` / `--no-<axis>` pair that
        // `overrides_with` its partner (last on the CLI wins).
        // `Args::from_matches` collapses each pair to `Option<bool>`. NO
        // clap default on either member — absence at the CLI means "let
        // the precedence merger consult [scope]/[rust] in dry.toml",
        // falling back to `true` (the clap-defaults-mask rule).
        .args(scope_pair(
            "within_crate",
            "within-crate",
            "no_within_crate",
            "no-within-crate",
            "Cluster duplication WITHIN the same crate / package (default true; negate with --no-within-crate)",
        ))
        .args(scope_pair(
            "across_crate",
            "across-crate",
            "no_across_crate",
            "no-across-crate",
            "Cluster duplication ACROSS different crates (default true; negate with --no-across-crate)",
        ))
        .args(scope_pair(
            "within_module",
            "within-module",
            "no_within_module",
            "no-within-module",
            "Cluster duplication WITHIN the same module path (default true; negate with --no-within-module)",
        ))
        .args(scope_pair(
            "across_module",
            "across-module",
            "no_across_module",
            "no-across-module",
            "Cluster duplication ACROSS different modules (default true; negate with --no-across-module)",
        ))
        .subcommand(subcommand_with_paths(
            "report",
            "Full duplication report (default — invokable without an explicit subcommand)",
        ))
        .subcommand(subcommand_with_paths(
            "explore",
            "Generate the self-contained HTML explorer to a temp file and open it in the browser (always exits 0; --no-open / $DRY_NO_OPEN suppress the launch)",
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
        .subcommand(
            Command::new("init")
                .about(
                    "Write the fully-annotated `<tool>.example.toml` reference into the current directory (Starship-pattern doc-gen)",
                )
                .arg(
                    Arg::new("force")
                        .long("force")
                        .short('f')
                        .action(ArgAction::SetTrue)
                        .help(
                            "Overwrite the example file if it already exists",
                        ),
                ),
        )
}

/// Build a paired `--<long>` / `--no-<long>` boolean flag set for one
/// relatedness-scoping axis (dry-rs#142).
///
/// Both flags are `ArgAction::SetTrue`, `global(true)` (so they may
/// appear before or after the subcommand), and `overrides_with` each
/// other so the LAST one on the command line wins. NEITHER carries a
/// clap default — `Args::from_matches::resolve_paired_bool` collapses
/// the pair to `Option<bool>`, and a default would mask the config tier
/// (the clap-defaults-mask rule).
///
/// Parameters are all `&'static str` (clap 4's `Id` / `Str` builders
/// require static or interned strings): `id` / `long` are the positive
/// flag's arg id + `--<long>`; `neg_id` / `neg_long` are the negative
/// flag's arg id + `--<neg_long>`. Only the positive flag carries
/// `help` — the negative is hidden so `--help` shows one entry per axis.
fn scope_pair(
    id: &'static str,
    long: &'static str,
    neg_id: &'static str,
    neg_long: &'static str,
    help: &'static str,
) -> [Arg; 2] {
    let positive = Arg::new(id)
        .long(long)
        .global(true)
        .action(ArgAction::SetTrue)
        .overrides_with(neg_id)
        .help(help);
    let negative = Arg::new(neg_id)
        .long(neg_long)
        .global(true)
        .action(ArgAction::SetTrue)
        .overrides_with(id)
        .hide(true);
    [positive, negative]
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
