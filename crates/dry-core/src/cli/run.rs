//! Generic CLI run loop — `run<N: NormalizerPort + Default>() -> ExitCode`.
//!
//! Adapter binaries (`dry4rs`, `dry4ts`) provide a 5-line `main()` that
//! calls `dry_core::cli::run::<MyNormalizer>()`. The function is the
//! single entry point for the v0.1 analyzer pipeline:
//!
//! ```text
//!   1. clap::parse() -> Args                       (exit 2 on arg error)
//!   2. --completions <SHELL>?  -> emit + exit 0
//!   3. Args -> AnalysisConfig (paths + extensions + include_ignored)
//!   4. enumerate(&config) -> SourceOutcome         (exit 2 on NoRoots)
//!   5. for each file: read + normalize             (per-file errors -> stderr)
//!   6. compare(forms, threshold) -> Vec<Match>
//!   7. build Report (truthful gate)
//!   8. apply --top / --only-failing -> Option<ViewProjection>
//!   9. dispatch by subcommand + Format -> stdout output
//!  10. derive ExitCode from report.passed + --no-fail
//! ```
//!
//! Truthful-gate-vs-shapeable-display: `result.*` (`Report`) is built
//! from the unfiltered comparison output; `view.*` (`ViewProjection`)
//! is the post-flag projection. Only the projection participates in
//! human-facing output shaping; the gate verdict is immune.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap_complete::Shell;

use super::AnalysisConfig;
use super::ResolvedScope;
use super::adapter_meta::AdapterMeta;
use super::args::{Args, Command, Format, ThresholdMode};
use super::build_command::build_command;
use super::effective::EffectiveConfig;
use crate::adapters::config::{ConfigError, discover_config, load_config};
use crate::adapters::reporters::json::{Envelope, EnvelopeMeta, ScopeApplied, ViewProjection};
use crate::adapters::reporters::{markdown, text};
use crate::adapters::source::{
    CrateIdResolver, SourceError, SourceOutcome, SourceWarning, enumerate,
};
use crate::comparison::{antiunify, compare_with_paths_scoped};
use crate::domain::{
    Config, FilePath, Match, NormalizedForm, NormalizedTree, Report, Span, Summary, Template,
};
use crate::ports::{NormalizerPort, TreeDeriverPort};

/// Exit-code constant for catastrophic argument / setup failure.
/// Mirrors clap's standard exit shape (`ExitCode::from(2)` on
/// `clap::Error::exit()`).
const EXIT_USAGE: u8 = 2;

/// Generic CLI run loop — the entry point adapter binaries call from
/// `main()`.
///
/// Generic over `N: NormalizerPort + TreeDeriverPort + Default`. The
/// `Default` bound lets `dry_core::cli::run::<SynNormalizer>(&DRY4RS_META)`
/// construct the adapter at the binary's call site without forcing the
/// binary to hand-roll instance construction.
///
/// # The `TreeDeriverPort` bound (epic #107, dry-rs#135)
///
/// The bound widened additively from `N: NormalizerPort + Default` to
/// `N: NormalizerPort + TreeDeriverPort + Default` when on-demand tree
/// re-derivation + anti-unification template attach wired into the run
/// loop. This is a **compile-time, source-compatible** change: every
/// existing caller already names a concrete adapter type
/// (`SynNormalizer`) that implements BOTH ports, so the binary's
/// `main()` call site is UNCHANGED. The **runtime CLI surface is
/// unchanged** — no new flag, no new subcommand, no new argument. Only
/// the generic bound the run loop demands of its type parameter widens.
/// After detection, the run loop calls [`TreeDeriverPort::derive_tree`]
/// for cluster members to attach an anti-unification [`Template`] to
/// each multi-member [`Match`] (see the private `attach_templates`
/// helper).
///
/// [`Template`]: crate::domain::Template
///
/// `meta: &'static AdapterMeta` carries the binary's identity
/// (`tool_name`, `tool_version`, `about` / `long_about` / `after_help`
/// text, `config_file_name` for the loader, `extensions` default).
/// Stage 6 of dry-rs#71 introduces `DRY4RS_META` in `dry4rs::main`
/// and threads it through here.
///
/// # Returns
///
/// - `ExitCode::SUCCESS` when `report.passed == true` OR `--no-fail`
///   was set.
/// - `ExitCode::FAILURE` when `report.passed == false` AND
///   `--no-fail` was NOT set.
/// - `ExitCode::from(2)` when the walker rejects with `NoRoots` or
///   the config-file loader returns `ConfigError`. clap-side argument
///   errors take the same code via `clap::Error::exit()`.
///
/// # Side effects
///
/// Prints the requested report shape to stdout; per-file parse
/// warnings go to stderr. `--completions <SHELL>` emits the
/// completion script to stdout and returns `ExitCode::SUCCESS`
/// without running the analyzer pipeline.
#[must_use]
pub fn run<N: NormalizerPort + TreeDeriverPort + Default>(meta: &'static AdapterMeta) -> ExitCode {
    meta.validate_or_panic();
    // clap auto-exits with code 2 on arg-parse errors, --help, and
    // --version via `.exit()` inside `try_get_matches`. We use the
    // production pipeline (`build_command(meta) +
    // get_matches() + Args::from_matches`); the test fixture goes
    // through the SAME pipeline via `parse_test_args`.
    let matches = build_command(meta).get_matches();
    let args = match Args::from_matches(&matches) {
        Ok(a) => a,
        Err(e) => e.exit(),
    };
    run_with_args::<N>(meta, &args)
}

/// Inner helper that takes a pre-parsed [`Args`] — separated for
/// testability. Production calls go through [`run`], which uses
/// `build_command(meta) + Args::from_matches`; tests invoke this
/// directly with a synthesized [`Args`].
fn run_with_args<N: NormalizerPort + TreeDeriverPort + Default>(
    meta: &AdapterMeta,
    args: &Args,
) -> ExitCode {
    let normalizer = N::default();

    // `--completions <SHELL>` short-circuits — emit the script and
    // exit 0 without running the analyzer pipeline.
    if let Some(shell) = args.completions {
        emit_completions(meta, shell);
        return ExitCode::SUCCESS;
    }

    // `init` short-circuits — write the annotated `<tool>.example.toml`
    // to the current directory (Starship-pattern doc-gen, dry-rs#77)
    // and exit 0. No analyzer pipeline; no config-file discovery.
    if let Some(Command::Init { force }) = args.command.as_ref() {
        return handle_init(meta, *force);
    }

    // Allowlist-management subcommands short-circuit — they DO NOT
    // run the analyzer pipeline at v0.1 (skeletal per the discovery
    // decision; full UX lands at v0.2 with `.dry-rs-ignore.toml`).
    // The dispatch surfaces the deferral note on stderr and exits 0.
    if let Some(cmd @ (Command::Ignore { .. } | Command::Ignored | Command::Cleanup)) =
        args.command.as_ref()
    {
        emit_allowlist_stub_note(cmd);
        return ExitCode::SUCCESS;
    }

    // Resolve + load the config file (when present). Per cross-tool
    // ADR D2: explicit `--config <path>` is missing-is-error; auto-
    // discovery walks upward from the analysis root and missing is
    // Ok(None) (no config file → AdapterMeta defaults apply).
    let analysis_root = compute_analysis_root(args);
    let config_path = match resolve_config_path(args, &analysis_root, meta) {
        Ok(p) => p,
        Err(err) => {
            render_config_error(&err);
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let file_config = match config_path {
        Some(ref path) => match load_config(path) {
            Ok(c) => Some(c),
            Err(err) => {
                render_config_error(&err);
                return ExitCode::from(EXIT_USAGE);
            }
        },
        None => None,
    };

    // Apply precedence: CLI > config > AdapterMeta default > compiled
    // fallback (per ADR D3). The merger consumes args, the optional
    // file config, AND the adapter meta. Mutable so the run loop can
    // overwrite `scope.crate_aware` with the runtime fact once crate-ids
    // are enriched (dry-rs#124).
    let mut config = merge_effective_inputs(meta, file_config.as_ref(), args);

    // Enumerate the source tree. The walker rejects empty roots with
    // `NoRoots`; clap defaults `paths` to `.` so this is unreachable
    // in practice. Catastrophic walker errors (file I/O failures, etc.)
    // surface as exit code 2 for CI clarity.
    let outcome = match enumerate(&config) {
        Ok(o) => o,
        Err(SourceError::NoRoots) => {
            eprintln!("error: no source roots configured");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    // Surface walker warnings on stderr so the user sees them but they
    // don't pollute structured stdout (especially `--format json`).
    emit_warnings(&outcome);

    // Normalize every enumerated file. Per-file parse errors go to
    // stderr; the adapter's skip-on-parse-error policy keeps the
    // pipeline running on a corrupt input.
    let (mut forms, form_paths) = normalize_files(&normalizer, &outcome);

    // Crate-id enrichment (dry-rs#141). The adapter walker resolves each
    // form's `module_path` (intra-file AST context) but leaves `crate_id`
    // as `None` — the crate axis is FilePath-vs-analysis-root work the
    // per-file `normalize` surface deliberately cannot see. Derive it
    // here (nearest-ancestor `Cargo.toml` `[package].name`, else top
    // directory segment under the analysis root, else `None`) and merge
    // onto each form's existing `location`, preserving the walker's
    // `module_path`.
    enrich_crate_ids(&mut forms, &form_paths, &config);

    // Relatedness-scoping `crate_aware` runtime fact (dry-rs#124). The
    // merger left `config.scope.crate_aware` at its `true` default; flip
    // it to the RUNTIME truth now that crate-ids are enriched — `true`
    // iff ANY form resolved a crate-id this run. When `false` (single-dir
    // run, no `Cargo.toml`), `ResolvedScope::allows` no-ops the two crate
    // axes so the run never silently drops every pair. Stored on `config`
    // as the single source of truth: the comparison engine prunes with
    // it AND the JSON envelope echoes it via `Envelope.scope`.
    config.scope.crate_aware = forms.iter().any(|f| f.location.crate_id.is_some());

    // Compare. The comparison engine is `debug_assert!`-only on
    // threshold range; clap's value parser is the production-build
    // input-validation boundary. We use the scoped, path-aware entry
    // point so each emitted `FormRef.file` carries the real source path
    // (not the qualified-name fallback that the library-facing
    // `compare()` synthesizes) AND the relatedness-scoping predicate
    // prunes pairs by crate / module boundary (Pass 1 + Pass 2). The
    // all-true default scope is a no-op (byte-identical to the
    // pre-scoping engine), so an unscoped run is unaffected.
    let mut matches =
        compare_with_paths_scoped(&forms, &form_paths, config.threshold, config.scope);

    // Anti-unification overlay (epic #107, dry-rs#135). For every
    // MULTI-member match, re-read each cluster member's source file
    // (members only — no global cache; the normalizer.rs IO boundary
    // is respected), re-derive each member's ordered `NormalizedTree`
    // via the second port, run first-order LGG, and attach the
    // resulting `Template`. Graceful degradation: any re-derive failure
    // or fingerprint drift leaves `template` as `None` — never a wrong
    // template, never a panic. Single-member matches carry no template
    // (a singleton is its own LGG with no divergence to generalize).
    attach_templates(&mut matches, &normalizer, &forms, &form_paths);

    // Build the truthful-gate Report (unfiltered). `result.passed`
    // comes from this; `--top` / `--only-failing` cannot reshape it.
    let summary = build_summary(&forms, &matches);
    let passed = matches.is_empty();
    let report = Report::new(matches.clone(), summary, passed);

    // Build the shapeable-display ViewProjection ONLY when a shaping
    // flag is active. When no flag is set, the JSON envelope's `view`
    // field stays `None` (omitted via skip_serializing_if), matching
    // the wire-envelope snapshot lock.
    let view = build_view(&report, args.top, args.only_failing, config.threshold);

    // Dispatch by subcommand. The default-when-None is
    // `Command::Report` (with paths defaulted to "." via
    // `Args::analysis_paths`) per the cross-tool convention. Clone to
    // avoid partial-move of `args.command`.
    let command = args
        .command
        .clone()
        .unwrap_or_else(|| Command::Report { paths: Vec::new() });
    dispatch_output(&command, &config, &normalizer, &report, view);

    // Exit-code derivation: --no-fail suppresses FAILURE; otherwise
    // `result.passed` is authoritative.
    if args.no_fail || report.passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Emit the v0.1 deferral note for an allowlist-management subcommand.
/// Mirrors the dispatch-arm messages; called from the short-circuit
/// branch in [`run_with_args`] so the analyzer pipeline is bypassed.
fn emit_allowlist_stub_note(command: &Command) {
    match command {
        Command::Ignore { fingerprint } => eprintln!(
            "note: `ignore` subcommand is a v0.1 stub — \
             allowlist UX (.dry-rs-ignore.toml) lands at v0.2. \
             Recorded request to ignore fingerprint: {fingerprint}"
        ),
        Command::Ignored => eprintln!(
            "note: `ignored` subcommand is a v0.1 stub — \
             allowlist UX (.dry-rs-ignore.toml) lands at v0.2."
        ),
        Command::Cleanup => eprintln!(
            "note: `cleanup` subcommand is a v0.1 stub — \
             allowlist UX (.dry-rs-ignore.toml) lands at v0.2."
        ),
        // Other variants are not allowlist-management; the caller
        // gated this match in `run_with_args` so we never reach here
        // in production. The fall-through is a defensive no-op.
        Command::Report { .. }
        | Command::Stats { .. }
        | Command::Check { .. }
        | Command::Init { .. } => {}
    }
}

/// `init` handler — writes `render_example_config(meta)` to
/// `<cwd>/<meta.example_file_name>` (Starship-pattern doc-gen,
/// dry-rs#77).
///
/// Thin wrapper around [`handle_init_in_dir`] that supplies the
/// current working directory. The inner helper exists so unit tests
/// can target a tempdir without `set_current_dir` (which serializes
/// the test suite — every CWD-mutating test forces single-threaded
/// execution).
fn handle_init(meta: &AdapterMeta, force: bool) -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    handle_init_in_dir(meta, force, &cwd)
}

/// Inner `init` handler — writes BOTH the annotated example reference
/// AND the JSON schema artifact to `<base>` (dry-rs#78).
///
/// Targets:
/// - `<base>/<meta.example_file_name>` (`render_example_config`)
/// - `<base>/<meta.schema_file_name>` (`render_json_schema`)
///
/// Atomicity: BOTH targets must be writable. When `force` is `false`,
/// the handler refuses if EITHER target exists (no partial write).
/// When `force` is `true`, both files are overwritten unconditionally.
/// This mirrors the `cargo deny init` convention extended to two
/// artifacts; the all-or-nothing semantic keeps `dry.example.toml`
/// and `dry.schema.json` in lockstep — both are generated from the
/// same annotated [`Config`] type, and a single-file half-update
/// would silently desync `$schema`-aware editors from the schema
/// they validate against.
///
/// Splits the CWD lookup out of the production [`handle_init`] so
/// unit tests can drive the happy path / `--force` overwrite path /
/// file-exists-error path against an isolated tempdir.
///
/// Returns `EXIT_USAGE` on existing-without-force, on `fs::write`
/// failures, and on filesystem errors; `ExitCode::SUCCESS` on a
/// clean dual write.
fn handle_init_in_dir(meta: &AdapterMeta, force: bool, base: &Path) -> ExitCode {
    let example_path = base.join(meta.example_file_name);
    let schema_path = base.join(meta.schema_file_name);

    if !force {
        // Refuse atomically — name the first existing file so the user
        // doesn't run --force only to discover a second collision.
        if example_path.exists() {
            eprintln!(
                "error: `{}` already exists; pass `--force` to overwrite",
                meta.example_file_name
            );
            return ExitCode::from(EXIT_USAGE);
        }
        if schema_path.exists() {
            eprintln!(
                "error: `{}` already exists; pass `--force` to overwrite",
                meta.schema_file_name
            );
            return ExitCode::from(EXIT_USAGE);
        }
    }

    let example_body = crate::adapters::config_doc_gen::render_example_config(meta);
    let schema_body = crate::adapters::config_schema_gen::render_json_schema(meta);

    if let Err(err) = fs::write(&example_path, &example_body) {
        eprintln!("error: failed to write `{}`: {err}", meta.example_file_name);
        return ExitCode::from(EXIT_USAGE);
    }
    if let Err(err) = fs::write(&schema_path, &schema_body) {
        eprintln!("error: failed to write `{}`: {err}", meta.schema_file_name);
        return ExitCode::from(EXIT_USAGE);
    }

    eprintln!(
        "wrote `{}` ({} bytes)",
        meta.example_file_name,
        example_body.len()
    );
    eprintln!(
        "wrote `{}` ({} bytes)",
        meta.schema_file_name,
        schema_body.len()
    );
    ExitCode::SUCCESS
}

/// Emit a shell-completion script for `shell` to stdout. Uses
/// `build_command(meta)` rather than the old `Args::command()` (which
/// the clap-derive `CommandFactory` impl provided before Stage 5 of
/// dry-rs#71 ripped out the derive).
fn emit_completions(meta: &AdapterMeta, shell: Shell) {
    let mut cmd = build_command(meta);
    let name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
}

/// Determine the analysis root used for upward config-file
/// discovery (N5 in the breadboard / per-tool ADR V1).
///
/// Returns the first positional analysis path supplied on the
/// active subcommand. When no subcommand is supplied (the implicit
/// default `report` path) or when the active subcommand has no
/// positional path, falls back to `std::env::current_dir()`. If even
/// `current_dir()` fails, returns `PathBuf::from(".")` — that's a
/// degraded but never-panicking fallback (relative ancestor walk
/// still works against the process's cwd).
///
/// dry-rs has no `--src` flag; analysis roots are positional
/// subcommand args (`dry4rs report crates/foo/`). This diverges
/// from crap-rs / scrap-rs's `--src`-based discovery (see per-tool
/// ADR V1).
#[must_use]
pub fn compute_analysis_root(args: &Args) -> PathBuf {
    if let Some(cmd) = &args.command {
        let paths = cmd.paths();
        if let Some(first) = paths.first() {
            return first.clone();
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Resolve the config-file path to load (N19 in the breadboard).
///
/// Branches:
/// 1. `args.config = Some(path)` (explicit `--config`) — validate
///    the path exists (else `ConfigError::Io`). Auto-discovery is
///    bypassed.
/// 2. `args.config = None` — call [`discover_config`] from the
///    analysis root with `meta.config_file_name`. `Ok(None)` here
///    means "no config file found anywhere above the analysis
///    root" — NOT an error per ADR D2.
///
/// # Errors
///
/// Returns [`ConfigError::Io`] when explicit `--config` is set but
/// the path doesn't exist on disk. Auto-discovery's
/// [`discover_config`] returns its own errors only on filesystem
/// permission failures (never on missing-file).
pub fn resolve_config_path(
    args: &Args,
    analysis_root: &Path,
    meta: &AdapterMeta,
) -> Result<Option<PathBuf>, ConfigError> {
    if let Some(path) = &args.config {
        if !path.exists() {
            return Err(ConfigError::Io {
                path: path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "explicit --config path does not exist",
                ),
            });
        }
        return Ok(Some(path.clone()));
    }
    discover_config(analysis_root, meta.config_file_name)
}

/// Format + emit a [`ConfigError`] to stderr (N6 in the breadboard).
///
/// Uses the typed error's `Display` impl (which surfaces the
/// `path:line:key` form for parse errors via `thiserror`'s
/// `#[source]` chaining). Returns void; the caller maps this to
/// `ExitCode::from(2)` (matches the clap argument-error exit shape).
pub fn render_config_error(err: &ConfigError) {
    eprintln!("error: {err}");
    let mut source = std::error::Error::source(err);
    while let Some(s) = source {
        eprintln!("  caused by: {s}");
        source = s.source();
    }
}

/// Merge CLI args, optional file config, and adapter defaults into
/// the [`AnalysisConfig`] consumed by the analyzer (N40 in the
/// breadboard). Precedence (per ADR D3 + dry-rs#78 cascade):
///
/// 1. CLI flag (`Some(v)` on `Args` for `--threshold` / `--format` /
///    `--threshold-mode`; `Args.include_ignored == true`)
/// 2. Per-language override (`[rust]` / `[typescript]`) — selected
///    by [`AdapterMeta::language`] via [`EffectiveConfig::resolve`]
/// 3. Shared file-config field (`[gate]` / `[output]` / `[walk]`)
/// 4. [`AdapterMeta`] default (e.g., `extensions`)
/// 5. Compiled-in fallback (`REVIEW_FIRST_FLOOR` = 0.85,
///    `Format::Text`, `ThresholdMode::Default`)
///
/// `threshold` / `format` / `threshold_mode` are `Option<T>` on
/// `Args` — `build_command` deliberately does NOT register a clap
/// default for these flags so absence at the CLI semantically means
/// "let the merger consult the next tier". This is what makes
/// `[rust] threshold = 0.9` actually take effect when the user
/// invokes `dry4rs report` without `--threshold`.
#[must_use]
pub fn merge_effective_inputs(
    meta: &AdapterMeta,
    config: Option<&Config>,
    args: &Args,
) -> AnalysisConfig {
    // Cascade-resolve the per-language overrides on top of shared
    // [gate]/[output]/[walk]. An absent config file (None) collapses
    // to the empty resolved tier — every knob None, every CLI flag
    // wins over None, every AdapterMeta default applies.
    let resolved = config
        .map(|c| EffectiveConfig::resolve(c, meta))
        .unwrap_or_default();

    // Relatedness scoping (dry-rs#142). Overlay the four CLI tri-state
    // flags onto the cascade-resolved `[scope]` / per-language knobs,
    // then collapse to a concrete `ResolvedScope`. Per-axis precedence:
    // CLI `Some` > cascade-resolved `Some` > compiled-in `true`. The
    // `crate_aware` runtime flag stays at its `true` default here; the
    // run loop adjusts it once it knows whether ANY form's crate-id was
    // resolvable this run (PR 11 threads the predicate into the engine).
    // Computed before the moves below (`extensions` consumes
    // `resolved.walk.extensions`); `resolve_scope` only reads the Copy
    // `Option<bool>` scope axes.
    let scope = resolve_scope(&resolved, args);

    // Extensions: resolved > AdapterMeta default. CLI override lands
    // in a future PR (no `--extensions` flag at v0.1).
    let extensions = resolved
        .walk
        .extensions
        .unwrap_or_else(|| meta.extensions_owned());

    // include_ignored: CLI > resolved > false. The CLI default is
    // `false`; if the user explicitly set `--include-ignored`, that
    // wins. Otherwise the resolved value applies.
    let include_ignored = if args.include_ignored {
        true
    } else {
        resolved.walk.include_ignored.unwrap_or(false)
    };

    // threshold / format / threshold_mode now produce Option<T>
    // from clap (no built-in default; per dry-rs#71 the precedence
    // merger owns the compiled-in fallback). Precedence chain per
    // ADR D3 + dry-rs#78 cascade: CLI > resolved > compiled-in default.
    let threshold = args
        .threshold
        .or(resolved.gate.threshold)
        .unwrap_or(crate::comparison::REVIEW_FIRST_FLOOR);
    let format = args
        .format
        .or(resolved.output.format)
        .unwrap_or(Format::Text);
    let threshold_mode = args
        .threshold_mode
        .or(resolved.gate.threshold_mode)
        .unwrap_or(ThresholdMode::Default);

    let mut analysis = AnalysisConfig::new(args.analysis_paths())
        .with_extensions(extensions)
        .with_include_ignored(include_ignored)
        .with_threshold(threshold)
        .with_format(format)
        .with_threshold_mode(threshold_mode)
        .with_scope(scope);

    // Scorecard labels — Option<String> on AnalysisConfig + the
    // wire envelope. Resolved-tier only at v0.1 (no CLI flag).
    if let Some(title) = resolved.output.title {
        analysis = analysis.with_title(title);
    }
    if let Some(subtitle) = resolved.output.subtitle {
        analysis = analysis.with_subtitle(subtitle);
    }

    analysis
}

/// Collapse the relatedness-scoping precedence chain into a concrete
/// [`ResolvedScope`] (dry-rs#142).
///
/// Per axis: CLI flag (`Some`) beats the cascade-resolved `[scope]` /
/// per-language value (`Some`), which beats the compiled-in `true` (the
/// no-op identity that clusters every pair). The four axes are
/// independent — each is resolved by the same `cli.or(resolved).unwrap_or(true)`
/// fold via the [`Args`] tri-state fields.
///
/// `crate_aware` is set to `true` here (the [`ResolvedScope::default`]
/// value); it is a RUNTIME fact, not a config knob — the run loop will
/// flip it to `false` when no form's crate-id was resolvable this run
/// (PR 11 of the dry-rs build-plan, when the predicate is threaded into
/// `compare_with`). Holding it `true` keeps the v0.x merger output
/// equivalent to the engine's pre-scoping behavior.
fn resolve_scope(resolved: &EffectiveConfig, args: &Args) -> ResolvedScope {
    let cascade = &resolved.scope;
    ResolvedScope {
        within_crate: args.within_crate.or(cascade.within_crate).unwrap_or(true),
        across_crate: args.across_crate.or(cascade.across_crate).unwrap_or(true),
        within_module: args.within_module.or(cascade.within_module).unwrap_or(true),
        across_module: args.across_module.or(cascade.across_module).unwrap_or(true),
        crate_aware: true,
    }
}

/// Emit any walker warnings to stderr. The walker accumulates these
/// rather than failing the whole enumeration on a single unreadable
/// path; the CLI surfaces them so the user notices.
fn emit_warnings(outcome: &SourceOutcome) {
    for warning in &outcome.warnings {
        match warning {
            SourceWarning::Unreadable { path, message } => {
                eprintln!("warning: unreadable {}: {}", path.display(), message);
            }
        }
    }
}

/// Read + normalize every file in `outcome`. Per-file errors emit a
/// stderr line and are skipped; the comparison engine sees only
/// successfully-normalized forms.
///
/// Returns parallel arrays `(forms, paths)` indexed identically:
/// `paths[i]` is the source [`FilePath`] of `forms[i]`. The CLI run
/// loop threads both into [`compare_with_paths_scoped`] so the emitted
/// matches carry real paths on each [`crate::domain::FormRef`].
fn normalize_files<N: NormalizerPort>(
    normalizer: &N,
    outcome: &SourceOutcome,
) -> (Vec<crate::domain::NormalizedForm>, Vec<FilePath>) {
    let mut forms = Vec::new();
    let mut paths = Vec::new();
    for path in &outcome.files {
        match fs::read_to_string(path.as_path()) {
            Ok(source) => match normalizer.normalize(&source, path) {
                Ok(file_forms) => {
                    paths.extend(std::iter::repeat_n(path.clone(), file_forms.len()));
                    forms.extend(file_forms);
                }
                Err(err) => {
                    eprintln!("warning: {path} failed to normalize: {err}");
                }
            },
            Err(err) => {
                eprintln!("warning: {path} failed to read: {err}");
            }
        }
    }
    debug_assert_eq!(
        forms.len(),
        paths.len(),
        "normalize_files() must produce parallel forms+paths arrays"
    );
    (forms, paths)
}

/// Enrich each form's [`StructuralLocation`] with a derived `crate_id`
/// (dry-rs#141).
///
/// The adapter walker resolves `module_path` (the enclosing-module
/// segments — intra-file AST context) but cannot resolve `crate_id`: the
/// crate axis is FilePath-vs-analysis-root work the per-file `normalize`
/// surface deliberately does not see. This function derives the crate id
/// from each form's source path (via [`CrateIdResolver`]) and writes it
/// onto the form's existing `location`, leaving the walker-supplied
/// `module_path` intact.
///
/// `forms[i]` and `form_paths[i]` are the parallel arrays
/// [`normalize_files`] produces. The resolver memoizes the
/// nearest-`Cargo.toml` walk per directory, so a corpus with many forms
/// per file pays the filesystem cost once per directory, not per form.
///
/// [`StructuralLocation`]: crate::domain::StructuralLocation
fn enrich_crate_ids(
    forms: &mut [NormalizedForm],
    form_paths: &[FilePath],
    config: &AnalysisConfig,
) {
    let mut resolver = CrateIdResolver::new(&config.roots);
    for (form, path) in forms.iter_mut().zip(form_paths.iter()) {
        form.location.crate_id = resolver.resolve(path);
    }
}

/// Attach an anti-unification [`Template`] to every MULTI-member match
/// (epic #107, dry-rs#135).
///
/// For each match with two or more forms, this re-derives each member's
/// ordered [`NormalizedTree`] via [`TreeDeriverPort::derive_tree`] —
/// re-reading the member's source file on demand — runs first-order LGG
/// over the member trees ([`antiunify`]), and attaches the resulting
/// template via [`Match::with_template`] (which also derives the three
/// reserved score slots from the template's holes).
///
/// ## IO boundary
///
/// The run loop deliberately does NOT retain the per-file source it read
/// during normalization. This function re-reads the source for cluster-
/// MEMBER files only, on demand, honoring the `normalizer.rs` IO
/// boundary (the detection path owns its reads; the overlay owns its
/// own). Within a single match, each distinct member file is read at
/// most once (members sharing a file reuse the read), but no source is
/// cached across matches — clusters are tiny, so re-reads are cheap.
///
/// ## Graceful degradation
///
/// The decision to attach (or leave `None`) is made by the pure
/// [`decide_template`]: a member whose source no longer parses, whose
/// span no longer addresses a form (edited on disk between detection and
/// re-derive), or whose re-derived tree's top-level fingerprints have
/// drifted from the form's stored `fingerprint_set` blocks attachment
/// for the WHOLE match — the template stays `None`. This never attaches
/// a wrong template and never panics; a degraded match is
/// byte-identical on the wire to a pre-overlay v0.1 match.
fn attach_templates<N: TreeDeriverPort>(
    matches: &mut [Match],
    tree_deriver: &N,
    forms: &[NormalizedForm],
    form_paths: &[FilePath],
) {
    // Identity lookup `(file, span) -> fingerprint_set`, mapping each
    // emitted `FormRef` (file + span) back to the original
    // `NormalizedForm`'s stored bag. Form identity is `(file, span)`
    // per the comparison-engine determinism contract.
    let bag_by_identity = build_bag_lookup(forms, form_paths);

    for m in matches.iter_mut() {
        // Singletons carry no template — a single tree is its own LGG
        // with no divergence to generalize. Only multi-member clusters
        // produce a meaningful template.
        if m.forms.len() < 2 {
            continue;
        }
        if let Some(template) = derive_match_template(m, tree_deriver, &bag_by_identity) {
            // `with_template` consumes `self`; swap through a temporary
            // so we can mutate in place behind the `&mut`.
            let attached =
                std::mem::replace(m, Match::new(Vec::new(), 0.0, m.tier)).with_template(template);
            *m = attached;
        }
    }
}

/// Build the `(file, span) -> fingerprint_set` identity lookup from the
/// parallel `forms` / `form_paths` arrays. Keyed by the same
/// `(file, span)` identity the comparison engine stamps onto each
/// emitted [`crate::domain::FormRef`].
fn build_bag_lookup<'a>(
    forms: &'a [NormalizedForm],
    form_paths: &'a [FilePath],
) -> std::collections::HashMap<(&'a FilePath, Span), &'a std::collections::HashSet<u64>> {
    let mut lookup = std::collections::HashMap::with_capacity(forms.len());
    for (form, path) in forms.iter().zip(form_paths.iter()) {
        // On a duplicate `(file, span)` key (should not occur — form
        // identity is unique) the first wins; the fp-gate tolerates
        // either, since same-identity forms carry the same bag.
        lookup
            .entry((path, form.span))
            .or_insert(&form.fingerprint_set);
    }
    lookup
}

/// Re-derive every member's tree for one multi-member match (I/O) and
/// decide whether a template attaches (pure).
///
/// Returns `Some(template)` only when every member re-derives cleanly
/// AND passes the fingerprint gate; otherwise `None` (graceful
/// degradation). The I/O (file reads + `derive_tree`) lives here; the
/// attach/skip decision is delegated to the pure [`decide_template`].
fn derive_match_template<N: TreeDeriverPort>(
    m: &Match,
    tree_deriver: &N,
    bag_by_identity: &std::collections::HashMap<(&FilePath, Span), &std::collections::HashSet<u64>>,
) -> Option<Template> {
    // Cache reads WITHIN this match only (members frequently share a
    // file). Dropped when the match is done — no cross-match retention.
    let mut source_cache: std::collections::HashMap<&FilePath, Option<String>> =
        std::collections::HashMap::new();

    let mut derived: Vec<Option<DerivedMember<'_>>> = Vec::with_capacity(m.forms.len());
    for form_ref in &m.forms {
        let source = source_cache
            .entry(&form_ref.file)
            .or_insert_with(|| fs::read_to_string(form_ref.file.as_path()).ok());
        let member = source.as_ref().and_then(|src| {
            let tree = tree_deriver.derive_tree(src, form_ref.span).ok()?;
            let bag = bag_by_identity
                .get(&(&form_ref.file, form_ref.span))
                .copied();
            Some(DerivedMember { tree, bag })
        });
        derived.push(member);
    }

    decide_template(derived)
}

/// One cluster member after re-derivation: its re-derived tree plus the
/// stored `fingerprint_set` (bag) of the original detected form (if the
/// `(file, span)` identity resolved).
struct DerivedMember<'a> {
    tree: NormalizedTree,
    bag: Option<&'a std::collections::HashSet<u64>>,
}

/// Decide whether a multi-member cluster's template attaches — the
/// single PURE decision function (no I/O).
///
/// Returns `Some(antiunify(member_trees))` only when EVERY member:
///
/// 1. re-derived successfully (`Some(_)` — its source parsed and its
///    span still addresses a form), AND
/// 2. resolved its stored fingerprint bag (`bag.is_some()`), AND
/// 3. passes the fingerprint gate ([`tree_top_level_fps_in_bag`]) — the
///    re-derived tree's top-level subform fingerprints are all present
///    in the form's stored `fingerprint_set`.
///
/// Otherwise returns `None`: a single failing member degrades the whole
/// match to no-template (never a partial or wrong template). This is the
/// P3 anti-drift bridge applied at the run-loop boundary — the derived
/// `root.fp` folds deterministically over the top-level child fps, so
/// "every top-level child fp is a bag member" is the bag-recoverable
/// form of "`root.fp` equals the form's stored top-level fold". An
/// edited-on-disk source produces top-level fps absent from the stored
/// bag, failing the gate.
fn decide_template(members: Vec<Option<DerivedMember<'_>>>) -> Option<Template> {
    let mut trees: Vec<NormalizedTree> = Vec::with_capacity(members.len());
    for member in members {
        let DerivedMember { tree, bag } = member?;
        let bag = bag?;
        if !tree_top_level_fps_in_bag(&tree, bag) {
            return None;
        }
        trees.push(tree);
    }
    Some(antiunify(&trees))
}

/// Fingerprint gate (pure): whether a re-derived tree's TOP-LEVEL
/// subform fingerprints are all members of the form's stored
/// `fingerprint_set` (the bag).
///
/// The synthetic `"Form"` root's `fp` is a top-level fold that is NOT a
/// bag member by itself (it is computed from the children, not sealed
/// into the bag). Its direct children — the `Attrs?` / `Sig` / `Block`
/// subform seals — ARE bag members. Because `root.fp` folds
/// deterministically over those child fps, "every top-level child fp is
/// in the bag" is exactly equivalent to "`root.fp` equals the form's
/// stored top-level fold" while staying checkable from the bag alone
/// (`dry-core` has no access to the adapter's private fold helper). A
/// child fp absent from the bag means the source changed between
/// detection and re-derive — the gate fails, the template degrades.
///
/// A childless root (degenerate) trivially passes — there is nothing to
/// contradict the stored bag.
fn tree_top_level_fps_in_bag(root: &NormalizedTree, bag: &std::collections::HashSet<u64>) -> bool {
    root.children.iter().all(|child| bag.contains(&child.fp))
}

/// Build the [`Summary`] aggregator over the unfiltered forms + matches.
fn build_summary(forms: &[crate::domain::NormalizedForm], matches: &[Match]) -> Summary {
    use std::collections::BTreeMap;
    let mut by_tier: BTreeMap<crate::domain::Tier, u32> = BTreeMap::new();
    let mut by_kind: BTreeMap<crate::domain::FormKind, u32> = BTreeMap::new();
    for m in matches {
        *by_tier.entry(m.tier).or_default() += 1;
        if let Some(f) = m.forms.first() {
            *by_kind.entry(f.kind).or_default() += 1;
        }
    }
    Summary {
        // `forms.len()` is bounded by file count × forms-per-file;
        // saturating cast preserves the contract that an absurdly large
        // form count maps to u32::MAX rather than overflowing.
        total_forms: u32::try_from(forms.len()).unwrap_or(u32::MAX),
        by_tier,
        by_kind,
    }
}

/// Build the optional shapeable-display projection from `--top` /
/// `--only-failing`. Returns `None` when no shaping flag is active
/// (the JSON envelope omits the `view` field in that case).
///
/// `--only-failing` filters to matches at or above the threshold gate;
/// `--top N` truncates to the top N by score. The combination applies
/// both filters; the order doesn't matter for the result set (both
/// project monotonically).
fn build_view(
    report: &Report,
    top: Option<u32>,
    only_failing: bool,
    threshold: f64,
) -> Option<ViewProjection> {
    if top.is_none() && !only_failing {
        return None;
    }
    let mut filtered: Vec<Match> = if only_failing {
        report
            .matches
            .iter()
            .filter(|m| m.score >= threshold)
            .cloned()
            .collect()
    } else {
        report.matches.clone()
    };
    // Sort by descending score; secondary key already encoded by
    // compare()'s deterministic sort, but we apply a stable score
    // ordering here so `--top N` picks the highest-scoring matches.
    filtered.sort_by(|a, b| b.score.total_cmp(&a.score));
    if let Some(n) = top {
        filtered.truncate(n as usize);
    }
    // Synthesize a view-summary from the filtered list. The view's
    // `passed` mirrors the truthful gate per the wire ADR — the view
    // never overrides the gate verdict.
    //
    // `total_forms` is a per-RUN total (the count of normalized forms
    // surveyed pre-filtering), NOT a per-match aggregate — view filters
    // happen AFTER the survey, so `view.summary.total_forms` mirrors
    // `result.summary.total_forms` rather than re-counting.
    let view_summary = build_view_summary(&filtered, report.summary.total_forms);
    Some(ViewProjection {
        matches: filtered,
        summary: view_summary,
        passed: report.passed,
    })
}

/// Build a Summary over a filtered set of matches. Mirrors
/// [`build_summary`] but with `total_forms` supplied from the
/// truthful-gate counter (the view's `total_forms` mirrors the run's
/// pre-filter survey total — view shaping doesn't change the count of
/// forms surveyed).
fn build_view_summary(filtered: &[Match], total_forms: u32) -> Summary {
    use std::collections::BTreeMap;
    let mut by_tier: BTreeMap<crate::domain::Tier, u32> = BTreeMap::new();
    let mut by_kind: BTreeMap<crate::domain::FormKind, u32> = BTreeMap::new();
    for m in filtered {
        *by_tier.entry(m.tier).or_default() += 1;
        if let Some(f) = m.forms.first() {
            *by_kind.entry(f.kind).or_default() += 1;
        }
    }
    Summary {
        total_forms,
        by_tier,
        by_kind,
    }
}

/// Dispatch the rendered output to stdout based on the subcommand +
/// format combination.
///
/// - `report` / default: full duplication report. Format obeys `--format`.
/// - `stats`: summary statistics only. Format obeys `--format`.
/// - `check`: exit-code-only. No stdout output regardless of format.
/// - `ignore` / `ignored` / `cleanup`: v0.1 stubs. Emit a "not yet
///   implemented" message on stderr; exit-code derivation still
///   follows the report verdict (none, since no analysis runs in those
///   modes — they return SUCCESS).
fn dispatch_output<N: NormalizerPort>(
    command: &Command,
    config: &AnalysisConfig,
    normalizer: &N,
    report: &Report,
    view: Option<ViewProjection>,
) {
    match command {
        Command::Report { .. } => emit_full_report(config, normalizer, report, view),
        Command::Stats { .. } => emit_stats(config, normalizer, report, view),
        // `Check` is exit-code-only — no stdout. The gate verdict drives
        // the exit code in `run_with_args` after `dispatch_output`
        // returns. `Ignore` / `Ignored` / `Cleanup` / `Init`
        // short-circuit BEFORE the analyzer pipeline runs (see
        // [`run_with_args`]); their inclusion here is a defensive
        // fallback for the exhaustive match.
        Command::Check { .. }
        | Command::Ignore { .. }
        | Command::Ignored
        | Command::Cleanup
        | Command::Init { .. } => {}
    }
}

/// Emit the full report — every match in the requested format.
fn emit_full_report<N: NormalizerPort>(
    config: &AnalysisConfig,
    normalizer: &N,
    report: &Report,
    view: Option<ViewProjection>,
) {
    match config.format {
        Format::Text => {
            // Text reporter reads from view when set (the user asked
            // for the shaped projection); otherwise from result.
            let to_render = view_as_report(view, report);
            print!("{}", text::render(&to_render));
        }
        Format::Markdown => {
            // Markdown reporter shares the text reporter's view/result
            // contract — render the shaped projection when present,
            // else the truthful report.
            let to_render = view_as_report(view, report);
            print!("{}", markdown::render(&to_render));
        }
        Format::Json => {
            // JSON envelope carries BOTH result and view; the truthful
            // gate stays parseable from `result.*` regardless of flag
            // settings.
            print_json_envelope(config, normalizer, report, view);
        }
    }
}

/// Emit the summary statistics. v0.1 emits a short text/json shape;
/// downstream tools (mokumo scorecard) consume the JSON shape.
fn emit_stats<N: NormalizerPort>(
    config: &AnalysisConfig,
    normalizer: &N,
    report: &Report,
    view: Option<ViewProjection>,
) {
    match config.format {
        // `stats` is a summary view, not the full per-match report —
        // its plain `key: value` counter shape is format-agnostic, so
        // `markdown` shares the `text` path here rather than cloning
        // the counter block or piping an empty markdown table. The
        // markdown reporter's value is in the `report` command, which
        // carries the per-match tier sections + fenced blocks.
        Format::Text | Format::Markdown => {
            // Render the summary block as plain ASCII. The text
            // reporter doesn't have a stats-only mode; we emit the
            // counters directly to keep the dispatch surface narrow.
            println!("total_forms: {}", report.summary.total_forms);
            println!("matches: {}", report.matches.len());
            for (tier, count) in &report.summary.by_tier {
                let label = match *tier {
                    crate::domain::Tier::AutoRefactor => "auto_refactor",
                    crate::domain::Tier::ReviewFirst => "review_first",
                    crate::domain::Tier::Advisory => "advisory",
                };
                println!("by_tier.{label}: {count}");
            }
            for (kind, count) in &report.summary.by_kind {
                let label = match *kind {
                    crate::domain::FormKind::Production => "production",
                    crate::domain::FormKind::Test => "test",
                    crate::domain::FormKind::Doctest => "doctest",
                };
                println!("by_kind.{label}: {count}");
            }
            println!("passed: {}", report.passed);
        }
        Format::Json => {
            // Reuse the full envelope path — consumers parsing
            // `result.summary` get the same shape they'd get from
            // `report --format json`.
            print_json_envelope(config, normalizer, report, view);
        }
    }
}

/// Convert an optional view projection back into a borrowed Report-like
/// shape for the text reporter. When `view` is `Some`, return a
/// freshly-built `Report` over the view's matches + summary (text
/// reporter doesn't differentiate result/view — it just renders what
/// it's given); when `None`, render the truthful Report.
fn view_as_report(view: Option<ViewProjection>, report: &Report) -> Report {
    match view {
        Some(v) => Report::new(v.matches, v.summary, v.passed),
        None => report.clone(),
    }
}

/// Serialize + print the full wire envelope, including the optional
/// view projection.
fn print_json_envelope<N: NormalizerPort>(
    config: &AnalysisConfig,
    normalizer: &N,
    report: &Report,
    view: Option<ViewProjection>,
) {
    let meta = EnvelopeMeta::new(
        normalizer.tool_name().to_string(),
        normalizer.tool_version().to_string(),
        normalizer.language().to_string(),
        current_timestamp(),
        threshold_mode_label(config.threshold_mode).to_string(),
    );
    // Construct the envelope directly so the view projection can be
    // attached. The json::render helper takes a Report-only path; we
    // bypass it here and serialize the Envelope ourselves with serde.
    let envelope = Envelope {
        schema_version: crate::adapters::reporters::json::SCHEMA_VERSION,
        tool: meta.tool,
        tool_version: meta.tool_version,
        language: meta.language,
        timestamp: meta.timestamp,
        threshold_mode: meta.threshold_mode,
        result: report.clone(),
        view,
        delta: None,
        diagnostics: None,
        // Scorecard labels — populated when the cascade resolved a
        // `[output].title` / `[output].subtitle` (or per-language
        // override). Stay `None` (and serialize-omitted) otherwise,
        // preserving the v0.1 wire-envelope snapshot.
        title: config.title.clone(),
        subtitle: config.subtitle.clone(),
        // Relatedness-scoping echo (dry-rs#124, Track B). Mirrors the
        // resolved predicate the comparison engine pruned with — the four
        // axes plus the runtime `crate_aware` flag — so reporters render a
        // read-only scope banner without re-deriving it. Always populated
        // by the run loop (the predicate is always resolved); the `None`
        // omission case exists for the library-facing `Envelope::new`
        // constructor and unit-test envelopes, which keep the v0.1
        // snapshot byte-identical.
        scope: Some(scope_applied(config)),
    };
    match serde_json::to_string_pretty(&envelope) {
        Ok(json) => println!("{json}"),
        Err(err) => eprintln!("error: failed to serialize JSON envelope: {err}"),
    }
}

/// Project the run's resolved [`ResolvedScope`] onto the wire
/// [`ScopeApplied`] echo (dry-rs#124). A flat field-for-field copy of the
/// five axes the comparison engine pruned with; reporters read it to
/// render a read-only scope banner.
const fn scope_applied(config: &AnalysisConfig) -> ScopeApplied {
    let s = config.scope;
    ScopeApplied {
        within_crate: s.within_crate,
        across_crate: s.across_crate,
        within_module: s.within_module,
        across_module: s.across_module,
        crate_aware: s.crate_aware,
    }
}

/// Map the threshold-mode enum to its wire label.
const fn threshold_mode_label(mode: ThresholdMode) -> &'static str {
    match mode {
        ThresholdMode::Strict => "strict",
        ThresholdMode::Default => "default",
        ThresholdMode::Lenient => "lenient",
    }
}

/// Capture an ISO-8601 UTC timestamp at the run-loop wrapper boundary.
///
/// The JSON reporter takes the timestamp as a caller-supplied string so
/// the wire-envelope snapshot stays byte-stable across runs. Production
/// callers (this function) construct from `SystemTime::now()`; tests
/// pass a fixed string.
///
/// Format: `"YYYY-MM-DDTHH:MM:SSZ"` — no fractional seconds, no timezone
/// offset (always UTC). Computed via a small lookup that does NOT pull
/// in `chrono` / `time` / `jiff` (per the dep budget — the timestamp
/// is wire metadata only, not a critical path).
fn current_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format_unix_seconds_iso8601(secs)
}

/// Format Unix seconds as ISO-8601 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
///
/// Pure function over `u64`; testable in isolation. Computes calendar
/// fields via the standard "days-from-Unix-epoch" algorithm (Howard
/// Hinnant's algorithm 8; the same one `time` and `chrono` use under
/// the hood). The internal arithmetic is intentionally `u64`-only —
/// since 1970 the algorithm has no negative-era cases for any value
/// `SystemTime::duration_since(UNIX_EPOCH)` can produce, so we stay in
/// the unsigned domain and saturate the small downcast at the end.
fn format_unix_seconds_iso8601(secs: u64) -> String {
    // Decompose into time-of-day + days-from-epoch.
    let day_secs: u64 = 86_400;
    let total_days = secs / day_secs;
    let time_of_day = secs % day_secs;
    let hours = time_of_day / 3_600;
    let minutes = (time_of_day % 3_600) / 60;
    let seconds = time_of_day % 60;

    // Compute (year, month, day) from days-since-1970-01-01 via
    // Howard Hinnant's algorithm (u64 variant). 719_468 is the day
    // count from 0000-03-01 to 1970-01-01; adding it shifts the epoch
    // origin to the start of the algorithm's "era" cycle.
    let z = total_days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097; // [0, 146_096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = y + u64::from(m <= 2);

    format!("{year:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        FilePath, FormKind, FormRef, LineColumn, Match, NormalizedForm, Report, Span, Summary, Tier,
    };
    use crate::ports::{NormalizeError, NormalizerPort, PlaceholderPolicy};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// Stub adapter for in-process tests of CLI-layer helpers
    /// (`emit_stats`, dispatch routing). The unit tests in this module
    /// don't exercise normalization — they validate the dispatch
    /// surface — so `normalize` is a sentinel `unreachable!()` and
    /// the identity hooks return fixed strings.
    struct StubNormalizer;

    impl NormalizerPort for StubNormalizer {
        fn extensions(&self) -> &'static [&'static str] {
            &[".rs"]
        }
        fn normalize(
            &self,
            _source: &str,
            _path: &FilePath,
        ) -> Result<Vec<NormalizedForm>, NormalizeError> {
            unreachable!("StubNormalizer.normalize is not exercised by emit_stats tests");
        }
        fn placeholder_policy(&self) -> PlaceholderPolicy {
            PlaceholderPolicy::default()
        }
        fn tool_name(&self) -> &'static str {
            "stub"
        }
        fn tool_version(&self) -> &'static str {
            "0.0.0"
        }
        fn language(&self) -> &'static str {
            "stub"
        }
    }

    fn make_form_ref(path: &str, line: u32) -> FormRef {
        FormRef::new(
            FilePath::from(PathBuf::from(path)),
            Span::try_new(LineColumn::new(line, 0), LineColumn::new(line + 2, 5)).unwrap(),
            FormKind::Production,
        )
    }

    fn make_match(score: f64, tier: Tier) -> Match {
        Match::new(vec![make_form_ref("src/a.rs", 10)], score, tier)
    }

    fn make_match_with_kind(kind: FormKind, tier: Tier) -> Match {
        let span = Span::try_new(LineColumn::new(1, 0), LineColumn::new(3, 5)).unwrap();
        Match::new(
            vec![FormRef::new(
                FilePath::from(PathBuf::from("src/a.rs")),
                span,
                kind,
            )],
            0.85,
            tier,
        )
    }

    fn stats_config(format: Format) -> AnalysisConfig {
        AnalysisConfig::new([PathBuf::from(".")]).with_format(format)
    }

    fn three_tier_summary() -> Summary {
        let mut by_tier: BTreeMap<Tier, u32> = BTreeMap::new();
        by_tier.insert(Tier::AutoRefactor, 1);
        by_tier.insert(Tier::ReviewFirst, 2);
        by_tier.insert(Tier::Advisory, 3);
        let mut by_kind: BTreeMap<FormKind, u32> = BTreeMap::new();
        by_kind.insert(FormKind::Production, 4);
        by_kind.insert(FormKind::Test, 5);
        by_kind.insert(FormKind::Doctest, 6);
        Summary {
            total_forms: 18,
            by_tier,
            by_kind,
        }
    }

    /// In-process exercise of every tier-label arm in `emit_stats`'s
    /// text path. Covers `AutoRefactor` / `ReviewFirst` / `Advisory`
    /// inside the inner `match *tier` branch.
    #[test]
    fn emit_stats_text_path_renders_every_tier_label() {
        let summary = three_tier_summary();
        let matches = vec![
            make_match(0.99, Tier::AutoRefactor),
            make_match(0.92, Tier::ReviewFirst),
            make_match(0.70, Tier::Advisory),
        ];
        let report = Report::new(matches, summary, false);
        emit_stats(&stats_config(Format::Text), &StubNormalizer, &report, None);
        // No stdout capture here — the smoke-coverage value is the
        // function executing every match arm without panicking.
        // The cli_pipeline binary test asserts the rendered labels.
    }

    /// Every form-kind arm in the text path: `Production` / `Test` /
    /// `Doctest`.
    #[test]
    fn emit_stats_text_path_renders_every_form_kind_label() {
        let summary = three_tier_summary();
        let matches = vec![
            make_match_with_kind(FormKind::Production, Tier::Advisory),
            make_match_with_kind(FormKind::Test, Tier::Advisory),
            make_match_with_kind(FormKind::Doctest, Tier::Advisory),
        ];
        let report = Report::new(matches, summary, false);
        emit_stats(&stats_config(Format::Text), &StubNormalizer, &report, None);
    }

    /// The JSON arm in `emit_stats` delegates to `print_json_envelope`
    /// — exercising it in-process closes the previously-uncovered
    /// dispatch branch.
    #[test]
    fn emit_stats_json_path_renders_envelope_without_panic() {
        let report = Report::new(
            vec![make_match(0.92, Tier::ReviewFirst)],
            Summary::new(),
            false,
        );
        emit_stats(&stats_config(Format::Json), &StubNormalizer, &report, None);
    }

    /// JSON path with a populated `ViewProjection` — exercises the
    /// `view.is_some()` branch inside `print_json_envelope`.
    #[test]
    fn emit_stats_json_path_with_view_renders_without_panic() {
        let report = Report::new(
            vec![make_match(0.92, Tier::ReviewFirst)],
            Summary::new(),
            false,
        );
        let view = ViewProjection {
            matches: vec![make_match(0.92, Tier::ReviewFirst)],
            summary: Summary::new(),
            passed: false,
        };
        emit_stats(
            &stats_config(Format::Json),
            &StubNormalizer,
            &report,
            Some(view),
        );
    }

    /// Empty report on the text path — `passed: true`, no tier or
    /// kind entries to iterate. Covers the empty `BTreeMap` exits of
    /// the inner `for` loops.
    #[test]
    fn emit_stats_text_path_handles_empty_passing_report() {
        let report = Report::new(vec![], Summary::new(), true);
        emit_stats(&stats_config(Format::Text), &StubNormalizer, &report, None);
    }

    #[test]
    fn build_view_returns_none_when_no_flags_set() {
        let report = Report::new(
            vec![make_match(0.92, Tier::ReviewFirst)],
            Summary::new(),
            false,
        );
        assert!(build_view(&report, None, false, 0.85).is_none());
    }

    #[test]
    fn build_view_returns_some_when_top_is_set() {
        let report = Report::new(
            vec![
                make_match(0.92, Tier::ReviewFirst),
                make_match(0.88, Tier::ReviewFirst),
            ],
            Summary::new(),
            false,
        );
        let view = build_view(&report, Some(1), false, 0.85).expect("view must populate");
        assert_eq!(view.matches.len(), 1);
        // Top by descending score → 0.92 wins.
        assert!((view.matches[0].score - 0.92).abs() < f64::EPSILON);
    }

    #[test]
    fn build_view_returns_some_when_only_failing_is_set() {
        let report = Report::new(
            vec![
                make_match(0.92, Tier::ReviewFirst),
                make_match(0.50, Tier::Advisory),
            ],
            Summary::new(),
            false,
        );
        let view = build_view(&report, None, true, 0.85).expect("view must populate");
        assert_eq!(
            view.matches.len(),
            1,
            "only the 0.92 match should pass --only-failing at threshold 0.85"
        );
    }

    #[test]
    fn build_view_combines_top_and_only_failing() {
        // 3 matches: two above threshold, one below; top=1 picks the
        // highest-scoring among the survivors.
        let report = Report::new(
            vec![
                make_match(0.92, Tier::ReviewFirst),
                make_match(0.88, Tier::ReviewFirst),
                make_match(0.50, Tier::Advisory),
            ],
            Summary::new(),
            false,
        );
        let view = build_view(&report, Some(1), true, 0.85).expect("view must populate");
        assert_eq!(view.matches.len(), 1);
        assert!((view.matches[0].score - 0.92).abs() < f64::EPSILON);
    }

    #[test]
    fn build_view_passed_mirrors_truthful_gate() {
        // Per the ADR: the view never overrides the gate verdict; it
        // carries the same value for symmetry.
        let report = Report::new(
            vec![make_match(0.92, Tier::ReviewFirst)],
            Summary::new(),
            false,
        );
        let view = build_view(&report, Some(10), false, 0.85).expect("view must populate");
        assert!(
            !view.passed,
            "view.passed must mirror result.passed (false)"
        );

        let report_passed = Report::new(vec![], Summary::new(), true);
        let view = build_view(&report_passed, Some(10), false, 0.85);
        // No matches → view filter still runs because the flag is set,
        // but the result.passed flag carries through.
        if let Some(v) = view {
            assert!(v.passed);
        }
    }

    #[test]
    fn build_summary_aggregates_tier_counts() {
        let forms: Vec<crate::domain::NormalizedForm> = (0..5)
            .map(|_| {
                crate::domain::NormalizedForm::new(
                    FormKind::Production,
                    std::collections::HashSet::new(),
                    Span::try_new(LineColumn::new(1, 0), LineColumn::new(1, 0)).unwrap(),
                    1,
                    1,
                )
            })
            .collect();
        let matches = vec![
            make_match(0.92, Tier::ReviewFirst),
            make_match(0.88, Tier::ReviewFirst),
            make_match(0.95, Tier::AutoRefactor),
        ];
        let summary = build_summary(&forms, &matches);
        assert_eq!(summary.total_forms, 5);
        assert_eq!(summary.by_tier.get(&Tier::ReviewFirst), Some(&2));
        assert_eq!(summary.by_tier.get(&Tier::AutoRefactor), Some(&1));
    }

    #[test]
    fn threshold_mode_labels_round_trip() {
        assert_eq!(threshold_mode_label(ThresholdMode::Strict), "strict");
        assert_eq!(threshold_mode_label(ThresholdMode::Default), "default");
        assert_eq!(threshold_mode_label(ThresholdMode::Lenient), "lenient");
    }

    #[test]
    fn format_unix_seconds_iso8601_handles_epoch() {
        assert_eq!(format_unix_seconds_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn format_unix_seconds_iso8601_handles_known_timestamps() {
        // Cross-checked with `date -u -j`. The dates pinned here are
        // anchored on the wire-envelope snapshot's fixed timestamp
        // (2026-05-24T22:00:00Z) plus the v0.x-canonical 2000-01-01.
        assert_eq!(
            format_unix_seconds_iso8601(1_779_660_000),
            "2026-05-24T22:00:00Z"
        );
        // 2000-01-01T00:00:00Z = 946684800 unix seconds.
        assert_eq!(
            format_unix_seconds_iso8601(946_684_800),
            "2000-01-01T00:00:00Z"
        );
    }

    #[test]
    fn current_timestamp_is_iso8601_z_format() {
        // We can't pin the exact value (it's the wall clock) but we
        // can verify the format. Expect 20 chars: "YYYY-MM-DDTHH:MM:SSZ".
        let ts = current_timestamp();
        assert_eq!(ts.len(), 20, "got: {ts}");
        assert!(ts.ends_with('Z'), "got: {ts}");
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }

    #[test]
    fn view_as_report_returns_truthful_when_view_is_none() {
        let report = Report::new(
            vec![make_match(0.92, Tier::ReviewFirst)],
            Summary::new(),
            false,
        );
        let restored = view_as_report(None, &report);
        assert_eq!(restored, report);
    }

    #[test]
    fn view_as_report_returns_view_data_when_view_is_some() {
        let report = Report::new(
            vec![
                make_match(0.92, Tier::ReviewFirst),
                make_match(0.88, Tier::ReviewFirst),
            ],
            Summary::new(),
            false,
        );
        let view = ViewProjection {
            matches: vec![make_match(0.92, Tier::ReviewFirst)],
            summary: Summary::new(),
            passed: false,
        };
        let restored = view_as_report(Some(view), &report);
        assert_eq!(restored.matches.len(), 1);
    }

    /// Synthetic adapter meta for `handle_init_in_dir` tests. Mirrors
    /// the production `DRY4RS_META` shape but uses fixture-only file
    /// names so the layer-4 ast-purity gate doesn't trip.
    const HANDLE_INIT_META: AdapterMeta = AdapterMeta {
        tool_name: "test-adapter",
        display_name: "TestLang",
        tool_version: "0.0.0",
        long_version: "0.0.0",
        about: "test about",
        long_about: "test long about",
        after_help: "",
        config_file_name: "test-adapter.toml",
        example_file_name: "test-adapter.example.toml",
        schema_file_name: "test-adapter.schema.json",
        extensions: &["rs"],
        language: crate::cli::Language::Rust,
        tool_info_uri: "https://example.test/info",
        rule_help_uri: "https://example.test/rules",
        default_excludes: &[],
        forced_excludes: &[],
    };

    #[test]
    fn handle_init_in_dir_writes_example_and_schema_when_targets_missing() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let _ = handle_init_in_dir(&HANDLE_INIT_META, false, dir.path());

        let example = dir.path().join(HANDLE_INIT_META.example_file_name);
        let example_body = fs::read_to_string(&example).expect("init writes the example file");
        assert!(
            example_body.starts_with("# test-adapter.example.toml"),
            "example file should begin with the header naming the tool; got:\n{}",
            example_body.lines().next().unwrap_or("(empty)")
        );
        assert!(
            example_body.contains("[gate]"),
            "example file should carry the full schema (got len={})",
            example_body.len()
        );

        let schema = dir.path().join(HANDLE_INIT_META.schema_file_name);
        let schema_body = fs::read_to_string(&schema).expect("init writes the schema file");
        assert!(
            schema_body.contains("\"title\": \"Config\""),
            "schema file should carry the Config root schema; first 200 chars:\n{}",
            &schema_body.chars().take(200).collect::<String>()
        );
    }

    #[test]
    fn handle_init_in_dir_refuses_when_example_exists_without_force() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let example = dir.path().join(HANDLE_INIT_META.example_file_name);
        fs::write(&example, b"pre-existing user content").expect("pre-seed file");
        let _ = handle_init_in_dir(&HANDLE_INIT_META, false, dir.path());
        let after = fs::read_to_string(&example).expect("file still readable");
        assert_eq!(
            after, "pre-existing user content",
            "example must NOT be overwritten without --force"
        );
        // Schema must NOT be partially written when refused.
        let schema = dir.path().join(HANDLE_INIT_META.schema_file_name);
        assert!(
            !schema.exists(),
            "schema must NOT be written when example collision refuses init"
        );
    }

    #[test]
    fn handle_init_in_dir_refuses_when_schema_exists_without_force() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let schema = dir.path().join(HANDLE_INIT_META.schema_file_name);
        fs::write(&schema, b"pre-existing schema content").expect("pre-seed file");
        let _ = handle_init_in_dir(&HANDLE_INIT_META, false, dir.path());
        let after = fs::read_to_string(&schema).expect("file still readable");
        assert_eq!(
            after, "pre-existing schema content",
            "schema must NOT be overwritten without --force"
        );
        // Example must NOT have been written when refused on schema.
        let example = dir.path().join(HANDLE_INIT_META.example_file_name);
        assert!(
            !example.exists(),
            "example must NOT be written when schema collision refuses init"
        );
    }

    #[test]
    fn handle_init_in_dir_force_overwrites_both_files() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let example = dir.path().join(HANDLE_INIT_META.example_file_name);
        let schema = dir.path().join(HANDLE_INIT_META.schema_file_name);
        fs::write(&example, b"stale example").expect("pre-seed example");
        fs::write(&schema, b"stale schema").expect("pre-seed schema");

        let _ = handle_init_in_dir(&HANDLE_INIT_META, true, dir.path());

        let example_after = fs::read_to_string(&example).expect("example readable");
        let schema_after = fs::read_to_string(&schema).expect("schema readable");
        assert!(
            example_after.starts_with("# test-adapter.example.toml"),
            "example should be replaced with fresh emitter output under --force"
        );
        assert!(
            !example_after.contains("stale example"),
            "stale example content should be gone after --force overwrite"
        );
        assert!(
            schema_after.contains("\"title\": \"Config\""),
            "schema should be replaced with fresh schemars output under --force"
        );
        assert!(
            !schema_after.contains("stale schema"),
            "stale schema content should be gone after --force overwrite"
        );
    }

    // ---- dry-rs#135: template-attach run-loop wiring ----
    //
    // These unit tests cover the PURE decision surface
    // (`decide_template`, `tree_top_level_fps_in_bag`, `build_bag_lookup`,
    // `attach_templates`) directly. The end-to-end re-derive path (real
    // syn re-parse + LGG against dry4rs's own clusters) is exercised by
    // the integration tests in `crates/dry4rs/tests/run_loop_template.rs`
    // and the self-check snapshot.

    use std::collections::HashSet;

    use crate::domain::{LeafClass, LeafToken, NormalizedTree};

    /// Span helper for tree/form fixtures.
    fn span_at(line: u32) -> Span {
        Span::try_new(LineColumn::new(line, 0), LineColumn::new(line + 2, 1)).unwrap()
    }

    /// A re-derived "Form" root with two top-level subform children whose
    /// fingerprints are `child_fps`. Mirrors the `SynTreeDeriver` shape
    /// (synthetic `"Form"` root over `Sig`/`Block` subform seals).
    fn form_tree(span: Span, child_fps: &[u64]) -> NormalizedTree {
        let children = child_fps
            .iter()
            .enumerate()
            .map(|(i, &fp)| {
                NormalizedTree::leaf(
                    "Subform".to_string(),
                    fp,
                    LeafToken::new(LeafClass::Ident, format!("c{i}")),
                    span,
                )
            })
            .collect();
        NormalizedTree::new("Form".to_string(), 0xF00D, children, span)
    }

    fn bag_of(fps: &[u64]) -> HashSet<u64> {
        fps.iter().copied().collect()
    }

    #[test]
    fn tree_top_level_fps_in_bag_passes_when_all_children_present() {
        let tree = form_tree(span_at(1), &[11, 22]);
        let bag = bag_of(&[11, 22, 33]); // superset is fine
        assert!(tree_top_level_fps_in_bag(&tree, &bag));
    }

    #[test]
    fn tree_top_level_fps_in_bag_fails_when_a_child_drifted() {
        // Source edited on disk -> a re-derived top-level fp is absent
        // from the stored bag.
        let tree = form_tree(span_at(1), &[11, 99]);
        let bag = bag_of(&[11, 22]);
        assert!(!tree_top_level_fps_in_bag(&tree, &bag));
    }

    #[test]
    fn tree_top_level_fps_in_bag_childless_root_trivially_passes() {
        // A degenerate childless root has nothing to contradict the bag.
        let tree = NormalizedTree::new("Form".to_string(), 0xF00D, Vec::new(), span_at(1));
        assert!(tree_top_level_fps_in_bag(&tree, &bag_of(&[])));
    }

    #[test]
    fn decide_template_attaches_when_all_members_match_bag() {
        // The MATCH arm: every member re-derived AND fp-gate passes ->
        // antiunify runs and a template is returned.
        let bag_a = bag_of(&[11, 22]);
        let bag_b = bag_of(&[11, 22]);
        let members = vec![
            Some(DerivedMember {
                tree: form_tree(span_at(1), &[11, 22]),
                bag: Some(&bag_a),
            }),
            Some(DerivedMember {
                tree: form_tree(span_at(10), &[11, 22]),
                bag: Some(&bag_b),
            }),
        ];
        let template = decide_template(members).expect("template must attach");
        // The two member trees are structurally identical (same child
        // fps) -> a hole-free template (LGG of identical trees).
        assert!(
            template.holes.is_empty(),
            "identical members yield a hole-free template, got {} holes",
            template.holes.len()
        );
    }

    #[test]
    fn decide_template_none_when_a_member_fp_mismatches() {
        // The fp-MISMATCH arm: one member's re-derived top-level fp is
        // not in its stored bag (edited on disk) -> whole match degrades
        // to None.
        let bag_a = bag_of(&[11, 22]);
        let bag_b = bag_of(&[11, 22]);
        let members = vec![
            Some(DerivedMember {
                tree: form_tree(span_at(1), &[11, 22]),
                bag: Some(&bag_a),
            }),
            Some(DerivedMember {
                tree: form_tree(span_at(10), &[11, 999]), // 999 drifted
                bag: Some(&bag_b),
            }),
        ];
        assert!(
            decide_template(members).is_none(),
            "a single fp-drifted member must degrade the whole match to None"
        );
    }

    #[test]
    fn decide_template_none_when_a_member_failed_to_derive() {
        // The re-derive-FAILURE arm: one member is `None` (parse error or
        // span no longer addresses a form) -> whole match None.
        let bag_a = bag_of(&[11, 22]);
        let members = vec![
            Some(DerivedMember {
                tree: form_tree(span_at(1), &[11, 22]),
                bag: Some(&bag_a),
            }),
            None,
        ];
        assert!(decide_template(members).is_none());
    }

    #[test]
    fn decide_template_none_when_bag_unresolved() {
        // The bag-MISSING arm: a member re-derived but its (file, span)
        // identity did not resolve to a stored bag -> None (we cannot
        // verify the fp gate without the bag, so we refuse to attach).
        let members = vec![
            Some(DerivedMember {
                tree: form_tree(span_at(1), &[11, 22]),
                bag: None,
            }),
            Some(DerivedMember {
                tree: form_tree(span_at(10), &[11, 22]),
                bag: None,
            }),
        ];
        assert!(decide_template(members).is_none());
    }

    #[test]
    fn build_bag_lookup_keys_on_file_and_span() {
        let span = span_at(5);
        let path = FilePath::from(PathBuf::from("src/x.rs"));
        let form = NormalizedForm::new(FormKind::Production, bag_of(&[7, 8, 9]), span, 3, 2);
        let forms = vec![form];
        let paths = vec![path.clone()];
        let lookup = build_bag_lookup(&forms, &paths);
        let bag = lookup
            .get(&(&path, span))
            .expect("identity (file, span) must resolve");
        assert!(bag.contains(&7) && bag.contains(&8) && bag.contains(&9));
        // A different span on the same file does NOT resolve.
        assert!(!lookup.contains_key(&(&path, span_at(99))));
    }

    /// A `TreeDeriverPort` stub that returns a fixed tree for any
    /// `(source, span)` — exercises `attach_templates` without a real
    /// syn re-parse. Reads happen against real temp files so the IO
    /// boundary (re-read on demand) is genuinely traversed.
    struct FixedTreeDeriver {
        child_fps: Vec<u64>,
    }

    impl TreeDeriverPort for FixedTreeDeriver {
        fn derive_tree(&self, _source: &str, span: Span) -> Result<NormalizedTree, NormalizeError> {
            Ok(form_tree(span, &self.child_fps))
        }
    }

    #[test]
    fn attach_templates_skips_single_member_matches() {
        // A singleton match is never given a template (no divergence to
        // generalize); `attach_templates` must leave it untouched.
        let mut matches = vec![Match::new(
            vec![make_form_ref("src/a.rs", 1)],
            1.0,
            Tier::AutoRefactor,
        )];
        let deriver = FixedTreeDeriver {
            child_fps: vec![11, 22],
        };
        attach_templates(&mut matches, &deriver, &[], &[]);
        assert!(
            matches[0].template.is_none(),
            "single-member match must not carry a template"
        );
    }

    #[test]
    fn attach_templates_none_when_member_file_unreadable() {
        // A multi-member match whose member files do not exist on disk:
        // the re-read fails -> derive_tree never runs -> template None,
        // no panic. Exercises the run-loop re-read failure path.
        let mut matches = vec![Match::new(
            vec![
                make_form_ref("/nonexistent/dir/aaa.rs", 1),
                make_form_ref("/nonexistent/dir/bbb.rs", 1),
            ],
            0.9,
            Tier::ReviewFirst,
        )];
        let deriver = FixedTreeDeriver {
            child_fps: vec![11, 22],
        };
        attach_templates(&mut matches, &deriver, &[], &[]);
        assert!(
            matches[0].template.is_none(),
            "unreadable member files must degrade to template None without panicking"
        );
    }

    #[test]
    fn attach_templates_attaches_for_readable_multi_member_match() {
        // End-to-end of the pure+IO seam with a stub deriver and real
        // temp files: two members, readable, fp-gate passes -> template
        // attaches and the reserved score slots derive.
        let dir = tempfile::tempdir().expect("tempdir");
        let file_a = dir.path().join("a.rs");
        let file_b = dir.path().join("b.rs");
        fs::write(&file_a, "fn a() {}").expect("write a");
        fs::write(&file_b, "fn b() {}").expect("write b");

        let span_a = span_at(1);
        let span_b = span_at(1);
        let ref_a = FormRef::new(FilePath::from(file_a.clone()), span_a, FormKind::Production);
        let ref_b = FormRef::new(FilePath::from(file_b.clone()), span_b, FormKind::Production);

        // Stored forms/paths so the (file, span) bag lookup resolves and
        // the fp-gate (children [11, 22] subset of bag) passes.
        let forms = vec![
            NormalizedForm::new(FormKind::Production, bag_of(&[11, 22]), span_a, 3, 1),
            NormalizedForm::new(FormKind::Production, bag_of(&[11, 22]), span_b, 3, 1),
        ];
        let form_paths = vec![FilePath::from(file_a), FilePath::from(file_b)];

        let mut matches = vec![Match::new(vec![ref_a, ref_b], 0.9, Tier::ReviewFirst)];
        let deriver = FixedTreeDeriver {
            child_fps: vec![11, 22],
        };
        attach_templates(&mut matches, &deriver, &forms, &form_paths);
        assert!(
            matches[0].template.is_some(),
            "readable, fp-passing multi-member match must carry a template"
        );
        // `with_template` derives the reserved slots from the holes.
        assert!(
            matches[0].structural_score.is_some(),
            "structural_score must be derived once a template attaches"
        );
        assert!(matches[0].rename_count.is_some());
    }

    // ---- enrich_crate_ids (dry-rs#141) ----

    #[test]
    fn enrich_crate_ids_resolves_package_name_and_preserves_module_path() {
        use std::fs;

        use crate::domain::StructuralLocation;

        // A workspace file under crates/foo/src/: crate_id resolves to the
        // `[package].name` (`foo`) while the walker-supplied module_path
        // is left intact.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("crates/foo/src")).unwrap();
        fs::write(
            root.join("crates/foo/Cargo.toml"),
            "[package]\nname = \"foo\"\n",
        )
        .unwrap();
        let file = root.join("crates/foo/src/lib.rs");
        fs::write(&file, "fn a() {}").unwrap();

        let span = Span::try_new(LineColumn::new(1, 0), LineColumn::new(1, 9)).unwrap();
        // Seed the form with a walker-style module_path; crate_id None.
        let form = NormalizedForm::new(
            FormKind::Production,
            std::collections::HashSet::new(),
            span,
            1,
            1,
        )
        .with_location(StructuralLocation {
            crate_id: None,
            module_path: vec!["inner".to_string()],
        });
        let mut forms = vec![form];
        let form_paths = vec![FilePath::from(file)];
        let config = AnalysisConfig::new([root.to_path_buf()]);

        enrich_crate_ids(&mut forms, &form_paths, &config);

        assert_eq!(forms[0].location.crate_id, Some("foo".to_string()));
        assert_eq!(
            forms[0].location.module_path,
            vec!["inner".to_string()],
            "crate-id enrichment must preserve the walker's module_path"
        );
    }

    #[test]
    fn enrich_crate_ids_yields_none_for_single_dir_run_with_no_cargo_toml() {
        use std::fs;

        // A flat directory with no Cargo.toml: crate_id stays None so
        // crate_aware=false downstream never drops every pair.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let file = root.join("a.rs");
        fs::write(&file, "fn a() {}").unwrap();

        let span = Span::try_new(LineColumn::new(1, 0), LineColumn::new(1, 9)).unwrap();
        let mut forms = vec![NormalizedForm::new(
            FormKind::Production,
            std::collections::HashSet::new(),
            span,
            1,
            1,
        )];
        let form_paths = vec![FilePath::from(file)];
        let config = AnalysisConfig::new([root.to_path_buf()]);

        enrich_crate_ids(&mut forms, &form_paths, &config);

        assert_eq!(
            forms[0].location.crate_id, None,
            "single-dir no-Cargo.toml run must leave crate_id None"
        );
    }
}
