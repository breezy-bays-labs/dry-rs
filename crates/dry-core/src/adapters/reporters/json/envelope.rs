//! v0.1 wire envelope — locked shape per
//! `ops/decisions/dry-rs/adr-nested-json-envelope.md`.
//!
//! The envelope is a serialization-layer concern, NOT a domain type
//! (per the ADR's "It is not a domain type" rule, line 49). It wraps a
//! [`Report`] — the truthful-gate domain shape — plus versioning
//! metadata (`schema_version`, `tool`, `tool_version`, `language`,
//! `timestamp`, `threshold_mode`) and the optional [`ViewProjection`]
//! / `delta` / `diagnostics` blocks.
//!
//! ## Locked shape (do not modify without a `schema_version` bump)
//!
//! ```jsonc
//! {
//!   "schema_version": 1,
//!   "tool":          "dry4rs",
//!   "tool_version":  "0.1.0",
//!   "language":      "rust",
//!   "timestamp":     "2026-05-24T22:00:00Z",
//!   "threshold_mode": "default",
//!   "result":        { /* domain::Report */ },
//!   "view":          null                     // omitted at v0.1
//!   // delta + diagnostics: omitted via skip_serializing_if when None
//! }
//! ```
//!
//! The locked shape is mechanically enforced by the wire-envelope
//! insta snapshot at `crates/dry-core/tests/wire_envelope_snapshot.rs`.
//! See [`Envelope::new`] for the v0.1 constructor; the snapshot
//! supplies a fixed timestamp so the asserted bytes are reproducible.
//!
//! ## Forward-compat
//!
//! - Reserved-by-ADR fields (`schema_version`, `tool`, `tool_version`,
//!   `language`, `timestamp`, `threshold_mode`) are always present.
//! - `view`: at v0.1, omitted via `skip_serializing_if`. PR 8 wires it
//!   to `--top` / `--only-failing` filters; until then it remains
//!   `None` for every emit site.
//! - `delta` / `diagnostics`: also `skip_serializing_if`. Land at
//!   v0.3+ per the roadmap; reserved here so the shape is consistent.
//!
//! Per the ADR, additive fields ship without bumping `schema_version`;
//! renames, removals, and type changes bump it. The forward-compat
//! table in the ADR is the source of truth — this module is the
//! reified locked shape, not the rule.

use serde::Serialize;

use crate::domain::Report;

/// Schema version of the locked v0.1 wire envelope. Bumps on any
/// breaking change to the envelope shape (rename, removal, type
/// change). Additive changes do not bump.
pub const SCHEMA_VERSION: u32 = 1;

/// Tool name emitted by the dry4rs adapter binary.
pub const TOOL_NAME_DRY4RS: &str = "dry4rs";

/// Language identifier emitted by the dry4rs adapter binary.
pub const LANGUAGE_RUST: &str = "rust";

/// Default threshold mode label — applied when the CLI does not
/// override.
pub const THRESHOLD_MODE_DEFAULT: &str = "default";

/// Per-emit-site metadata threaded into the envelope by the adapter
/// binary's wrapper that invokes the JSON reporter.
///
/// **Deterministic-test contract**: the [`EnvelopeMeta::timestamp`]
/// field is supplied by the caller, NEVER pulled from
/// `SystemTime::now()` inside the reporter. This is how the
/// wire-envelope insta snapshot stays byte-stable across runs. The
/// adapter binary's run-loop wrapper (lands in PR 8) calls
/// `time::OffsetDateTime::now_utc()` (or equivalent) before invoking
/// the reporter; tests pass a fixed string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeMeta {
    /// Tool identity, e.g. `"dry4rs"`. The adapter binary owns this.
    pub tool: String,
    /// Tool version, e.g. `"0.1.0"`. Threaded from
    /// `env!("CARGO_PKG_VERSION")` at the binary entry point.
    pub tool_version: String,
    /// Language identifier, e.g. `"rust"`. The adapter binary owns
    /// this; cross-adapter renderers parse this field to differentiate
    /// Rust-source vs TypeScript-source results.
    pub language: String,
    /// RFC3339 / ISO-8601 UTC timestamp. Supplied by the caller (NOT
    /// pulled inside the reporter) so the snapshot stays
    /// reproducible.
    pub timestamp: String,
    /// Threshold mode label. v0.1 emits `"default"`; PR 8 may add
    /// `"strict"` / `"advisory"` / user-configured labels.
    pub threshold_mode: String,
}

impl EnvelopeMeta {
    /// Construct a meta with every field explicitly supplied.
    #[must_use]
    pub const fn new(
        tool: String,
        tool_version: String,
        language: String,
        timestamp: String,
        threshold_mode: String,
    ) -> Self {
        Self {
            tool,
            tool_version,
            language,
            timestamp,
            threshold_mode,
        }
    }
}

/// Shapeable display projection — reserved by the ADR's
/// "truthful-gate vs shapeable-display" split.
///
/// At v0.1 the run loop does not yet shape a view (the CLI flags that
/// drive view-shaping — `--top`, `--only-failing`, `--no-fail` —
/// land in PR 8), so every emit site passes `None` for the `view`
/// field on [`Envelope`]. The struct exists so the wire shape is
/// stable when v0.x populates it.
///
/// The fields mirror [`Report`] field-for-field; the comparison
/// engine constructs them by projecting the truthful `Report` through
/// the active view filters.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewProjection {
    /// The shaped match list.
    pub matches: Vec<crate::domain::Match>,
    /// The shaped summary aggregates.
    pub summary: crate::domain::Summary,
    /// Mirrors `Report::passed` from the truthful gate — the view
    /// never overrides the gate verdict; it carries the same value
    /// for symmetry.
    pub passed: bool,
}

/// The v0.1 wire envelope. Constructed by the JSON reporter
/// ([`crate::adapters::reporters::json::render`]) and serialized to
/// pretty JSON.
///
/// **Field order matters** for the snapshot — serde emits fields in
/// declaration order on structs. Keep this struct in lockstep with
/// the ADR's locked shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Envelope {
    /// Schema version. Bumps only on breaking changes; additive
    /// changes (new optional fields, new struct fields, new enum
    /// variants on `#[non_exhaustive]` enums) do NOT bump.
    pub schema_version: u32,
    /// Tool identity, e.g. `"dry4rs"`.
    pub tool: String,
    /// Tool version, e.g. `"0.1.0"`.
    pub tool_version: String,
    /// Source language, e.g. `"rust"` / `"typescript"`.
    pub language: String,
    /// RFC3339 / ISO-8601 UTC timestamp supplied by the caller.
    pub timestamp: String,
    /// Threshold mode label.
    pub threshold_mode: String,
    /// Truthful-gate domain shape — the unfiltered, unshapeable
    /// source of truth. CI parsers reading `result.passed` are
    /// immune to view-side reshaping.
    pub result: Report,
    /// Shapeable display projection — reserved at v0.1. Omitted from
    /// the wire output when `None` (PR 8 wires the populating path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<ViewProjection>,
    /// Baseline-diff block — reserved at v0.1, lands at v0.3+ per
    /// the roadmap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<serde_json::Value>,
    /// Verbose-mode pipeline diagnostics — reserved at v0.1, lands at
    /// v0.1+ when the parser-error path needs structured surfacing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<serde_json::Value>,
    /// Scorecard title supplied by `[output].title` (or a per-language
    /// override via the dry-rs#78 cascade). Consumed by external
    /// rendering surfaces (e.g., the dry-scorecard GitHub Action's
    /// sticky PR comment header). Omitted from the wire output when
    /// `None`; declared at the END of the struct to keep additive
    /// snapshot stability (declaration order = serialization order).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Scorecard subtitle (second header line). Companion to
    /// [`title`](Self::title); same cascade source, same omission
    /// behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Resolved relatedness-scoping facts applied to this run (dry-rs#124,
    /// Track B). Echoes the four scope axes plus the runtime `crate_aware`
    /// flag the comparison engine pruned with, so reporters / the HTML
    /// explorer can render a read-only scope banner without re-deriving
    /// the predicate. Additive, declared at the END of the struct;
    /// omitted from the wire when `None` so a run that does not populate
    /// it stays byte-identical to the v0.1 snapshot. The run loop always
    /// supplies it once scoping is wired (dry-rs#124); `None` is the
    /// pre-scoping / unit-test default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeApplied>,
    /// Presentation mode hint for the single-file HTML explorer (dry-rs#147,
    /// epic #111). `Some(Mode::Report)` is the read-only CI-artifact view
    /// (30-second PR verdict); `Some(Mode::Explore)` is the local
    /// refactoring-triage view (the `explore` subcommand, landing in a
    /// later PR). Other reporters leave it `None` — only the HTML reporter
    /// populates it. Additive, declared AFTER [`scope`](Self::scope);
    /// omitted from the wire when `None` so JSON / text / markdown runs
    /// stay byte-identical to the v0.1 snapshot. The HTML frontend reads
    /// this to pick which views to surface; it MUST degrade gracefully
    /// when the field is absent (treat as `report`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    /// Declarative feature flags telling the HTML frontend which payload-
    /// backed views are renderable for this run (dry-rs#147, epic #111).
    /// At PR13 the bare reporter advertises the overview + cluster views;
    /// the substitution grid / d-slider / scope banner flip on in a later
    /// PR as their backing wire fields populate. Additive, declared AFTER
    /// [`mode`](Self::mode); omitted from the wire when `None` so non-HTML
    /// runs stay byte-identical to the v0.1 snapshot. The frontend reads
    /// the flags to decide which UI affordances to mount; it MUST treat
    /// an absent block as "overview only".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Capabilities>,
}

/// Presentation mode the single-file HTML explorer renders in (dry-rs#147,
/// epic #111), echoed onto the wire as `Envelope.mode`.
///
/// The two jobs-to-be-done the dual-mode explorer serves:
///
/// - [`Report`](Self::Report) — read-only 30-second PR verdict; opens from
///   a CI Pages URL or artifact; the deep-link target from a sticky PR
///   comment. The only mode PR13 emits.
/// - [`Explore`](Self::Explore) — local refactoring triage ("what's the
///   same vs different across N sites"); opens on the hottest cluster.
///   Emitted by the `explore` subcommand landing in a later PR.
///
/// `#[non_exhaustive]` per the AGENTS.md `#[non_exhaustive]` discipline —
/// enums YES, result structs NO. New presentation modes land additively.
///
/// Serde uses `snake_case` tags (`report` / `explore`) matching the
/// envelope's vocabulary so the frontend reads a stable wire string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Mode {
    /// Read-only CI-artifact view — the 30-second PR verdict.
    Report,
    /// Local refactoring-triage view — opens on the hottest cluster.
    Explore,
}

impl Mode {
    /// Stable wire label for this mode — the single source of truth the
    /// frontend keys on.
    ///
    /// Byte-identical to the serde rendering (`#[serde(rename_all =
    /// "snake_case")]`): `report` / `explore`. A reporter that needs the
    /// label in Rust calls this instead of re-spelling the mapping, so a
    /// new variant breaks exactly one match arm.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::Explore => "explore",
        }
    }
}

/// Declarative capability flags advertising which payload-backed views the
/// HTML frontend can mount for this run (dry-rs#147, epic #111), echoed
/// onto the wire as `Envelope.capabilities`.
///
/// The frontend reads these to decide which UI affordances to show without
/// re-deriving them from the payload shape. PR13 (the bare reporter) ships
/// `overview` + `clusters` true and the richer views false; later PRs flip
/// `substitution_grid` / `d_slider` / `scope_banner` on as their backing
/// wire fields (`Match.template`, `Envelope.scope`) populate.
///
/// Result struct (AGENTS.md `#[non_exhaustive]` discipline — structs NO):
/// no `#[non_exhaustive]`; evolves via the [`Capabilities::report`] /
/// [`Capabilities::new`] constructors. `clippy::struct_excessive_bools` is
/// allowed — these are orthogonal feature flags, the frontend's mental
/// model, NOT a bitflag candidate (same rationale as [`ScopeApplied`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Capabilities {
    /// The overview panel (totals + per-tier table) is renderable. Always
    /// `true` — the overview is backed by `result.summary`, present on
    /// every run.
    pub overview: bool,
    /// The cluster list + cluster-detail views are renderable. Always
    /// `true` at PR13 — backed by `result.matches`.
    pub clusters: bool,
    /// The per-cluster substitution grid is renderable. Backed by
    /// `Match.template`'s hole-indexed substitutions; `false` until the
    /// showcase PR wires it.
    pub substitution_grid: bool,
    /// The divergence (d-) slider is renderable. Backed by per-hole
    /// `Divergence`; `false` until the showcase PR wires it.
    pub d_slider: bool,
    /// The read-only scope banner is renderable. Backed by `Envelope.scope`;
    /// `false` until the showcase PR surfaces it.
    pub scope_banner: bool,
}

impl Capabilities {
    /// Capabilities for the PR13 bare REPORT reporter: overview + clusters
    /// on, the showcase views (substitution grid / d-slider / scope banner)
    /// off until their backing wire fields populate.
    #[must_use]
    pub const fn report() -> Self {
        Self {
            overview: true,
            clusters: true,
            substitution_grid: false,
            d_slider: false,
            scope_banner: false,
        }
    }

    /// Construct a capability set with every flag explicitly supplied —
    /// the constructor path later PRs use as they flip the showcase views
    /// on.
    ///
    /// `clippy::fn_params_excessive_bools` is allowed for the same reason
    /// `struct_excessive_bools` is on the struct: these are orthogonal
    /// feature flags (the frontend's mental model), not a two-variant-enum
    /// candidate. Later PRs call this with named locals at the run-loop
    /// boundary, so the positional-bool readability concern does not bite.
    #[must_use]
    #[allow(clippy::fn_params_excessive_bools)]
    pub const fn new(
        overview: bool,
        clusters: bool,
        substitution_grid: bool,
        d_slider: bool,
        scope_banner: bool,
    ) -> Self {
        Self {
            overview,
            clusters,
            substitution_grid,
            d_slider,
            scope_banner,
        }
    }
}

/// The relatedness-scoping facts the comparison engine applied this run
/// (dry-rs#124, Track B), echoed onto the wire as `Envelope.scope`.
///
/// A flat snapshot of the resolved predicate
/// ([`crate::domain::ResolvedScope`]): the four orthogonal axes plus the
/// runtime `crate_aware` flag (whether ANY form's crate-id was resolvable
/// this run). Reporters and the HTML explorer read it to render a
/// read-only scope banner — when `crate_aware == false` the two crate
/// toggles are gray (the crate axis no-ops, since no crate-id was
/// derivable).
///
/// Result struct (AGENTS.md `#[non_exhaustive]` discipline — structs NO):
/// no `#[non_exhaustive]`; evolves via construction. Five `bool`s map 1:1
/// to the locked `[scope]` config knobs + [`crate::domain::ResolvedScope`];
/// `clippy::struct_excessive_bools` is allowed here for the same reason
/// as on `ResolvedScope` — the orthogonal axes are the user's mental
/// model, not a bitflag candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ScopeApplied {
    /// Whether same-crate / same-package pairs were allowed to cluster.
    pub within_crate: bool,
    /// Whether cross-crate / cross-package pairs were allowed to cluster.
    pub across_crate: bool,
    /// Whether same-module pairs were allowed to cluster.
    pub within_module: bool,
    /// Whether cross-module pairs were allowed to cluster.
    pub across_module: bool,
    /// Whether ANY form's crate-id was resolvable this run. When `false`,
    /// the two crate axes were no-ops (always-allowed) so a single-dir
    /// run never dropped every pair.
    pub crate_aware: bool,
}

impl Envelope {
    /// Construct a v0.1 envelope wrapping `report`, with no view /
    /// delta / diagnostics / title / subtitle block.
    ///
    /// The adapter binary's wrapper supplies `meta` (timestamp included).
    /// `view` is always `None` at v0.1 because the CLI shaping flags
    /// (`--top`, `--only-failing`) land in PR 8. `title` / `subtitle`
    /// stay `None` when `[output].title` / `[output].subtitle` (or
    /// per-language overrides per dry-rs#78) are unset; consumers
    /// drive population through the run-loop wrapper, not this
    /// constructor.
    #[must_use]
    pub fn new(report: Report, meta: EnvelopeMeta) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            tool: meta.tool,
            tool_version: meta.tool_version,
            language: meta.language,
            timestamp: meta.timestamp,
            threshold_mode: meta.threshold_mode,
            result: report,
            view: None,
            delta: None,
            diagnostics: None,
            title: None,
            subtitle: None,
            scope: None,
            mode: None,
            capabilities: None,
        }
    }

    /// Attach the HTML-explorer presentation hints (dry-rs#147) — the
    /// `mode` and `capabilities` blocks the single-file frontend reads to
    /// pick its views. Returns `self` for chaining.
    ///
    /// Only the HTML reporter calls this; the JSON / text / markdown paths
    /// leave both `None` so their wire output stays byte-identical to the
    /// v0.1 snapshot.
    #[must_use]
    pub fn with_presentation(mut self, mode: Mode, capabilities: Capabilities) -> Self {
        self.mode = Some(mode);
        self.capabilities = Some(capabilities);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Report;

    fn fixed_meta() -> EnvelopeMeta {
        EnvelopeMeta::new(
            TOOL_NAME_DRY4RS.into(),
            "0.1.0".into(),
            LANGUAGE_RUST.into(),
            "2026-05-24T22:00:00Z".into(),
            THRESHOLD_MODE_DEFAULT.into(),
        )
    }

    #[test]
    fn mode_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Mode::Report).unwrap(), "\"report\"");
        assert_eq!(
            serde_json::to_string(&Mode::Explore).unwrap(),
            "\"explore\""
        );
    }

    #[test]
    fn mode_as_str_matches_serde_label() {
        for mode in [Mode::Report, Mode::Explore] {
            let json = serde_json::to_string(&mode).unwrap();
            let unquoted = json.trim_matches('"');
            assert_eq!(mode.as_str(), unquoted, "mode label drifted from serde");
        }
    }

    #[test]
    fn capabilities_report_advertises_overview_and_clusters_only() {
        let caps = Capabilities::report();
        assert!(caps.overview);
        assert!(caps.clusters);
        // PR13 ships the bare reporter — the showcase views flip on later.
        assert!(!caps.substitution_grid);
        assert!(!caps.d_slider);
        assert!(!caps.scope_banner);
    }

    #[test]
    fn envelope_new_leaves_mode_and_capabilities_none() {
        let env = Envelope::new(Report::empty_passed(), fixed_meta());
        assert!(env.mode.is_none());
        assert!(env.capabilities.is_none());
    }

    #[test]
    fn envelope_omits_mode_and_capabilities_when_none() {
        // Byte-identical-when-off contract: a constructor-path envelope
        // (every additive field None) must NOT emit `mode` / `capabilities`
        // keys, keeping the v0.1 snapshot stable for non-HTML runs.
        let env = Envelope::new(Report::empty_passed(), fixed_meta());
        let json = serde_json::to_string_pretty(&env).unwrap();
        assert!(!json.contains("\"mode\""), "mode must be omitted: {json}");
        assert!(
            !json.contains("\"capabilities\""),
            "capabilities must be omitted: {json}"
        );
    }

    #[test]
    fn with_presentation_emits_both_blocks() {
        let env = Envelope::new(Report::empty_passed(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::report());
        let json = serde_json::to_string_pretty(&env).unwrap();
        assert!(json.contains("\"mode\": \"report\""), "{json}");
        assert!(json.contains("\"capabilities\""), "{json}");
        assert!(json.contains("\"overview\": true"), "{json}");
        assert!(json.contains("\"substitution_grid\": false"), "{json}");
    }
}
