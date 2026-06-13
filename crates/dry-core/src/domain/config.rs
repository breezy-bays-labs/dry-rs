//! POD config types deserialized from `dry.toml` (or any other
//! adapter's `<tool>.toml`).
//!
//! Per the cross-tool config-file ADR (`ops/decisions/org/adr-config-
//! file-pattern.md`, D4 + D6 + D9), schema types live in
//! `dry-core::domain::config` — pure POD with `serde` derives, no I/O,
//! no logic. The loader (`discover_config`, `load_config`,
//! `ConfigError`) lives in `dry-core::adapters::config`. The hexagonal-
//! layout invariant lets downstream layers depend on these types
//! freely without dragging in `toml` parsing or filesystem walks.
//!
//! Wire-shape discipline (per [`crate::adapters::reporters`] +
//! `adr-config-file-pattern.md` D4 + D8 + D9):
//!
//! - Every struct in this module carries `#[serde(deny_unknown_fields)]`
//!   — typos surface at parse time with a clear TOML path + line + key
//!   message (ADR D4).
//! - Every public *enum* in this module carries `#[non_exhaustive]`
//!   (the AGENTS.md `#[non_exhaustive]` discipline + ADR D8 — enums
//!   YES, result structs NO).
//! - Every field uses `#[serde(default)]` + `#[serde(skip_serializing_
//!   if = "Option::is_none")]` (for `Option<T>` fields) so new fields
//!   land additively without breaking existing configs (ADR D9 +
//!   forward-compat).
//! - Map types (none yet at v0.1) MUST use `BTreeMap` (deterministic
//!   key order, byte-stable round-trip per ADR D9).

use documented::DocumentedFields;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cli::{Format, ThresholdMode};

/// Top-level config tree deserialized from `dry.toml`.
///
/// Forward-compat surface: new tables (e.g., reserved `[delta]` /
/// `[reporter]` from the per-tool ADR V2) land additively as
/// `Option<NewConfig>` fields with `#[serde(default)]`. Existing
/// configs continue to parse after the schema gains a field.
///
/// `#[serde(deny_unknown_fields)]` per ADR D4 — typos in TOML surface
/// at parse time with a clear `path:line:key` message. NO silent
/// default fallback.
///
/// ## Cascade model (dry-rs#78)
///
/// `[gate]`, `[output]`, `[walk]` carry SHARED knobs every adapter
/// honors. `[rust]` and `[typescript]` carry per-language overrides
/// that replace the shared value for one adapter only. The cascade
/// resolver ([`crate::cli::EffectiveConfig::resolve`]) collapses
/// per-language `Some(v)` over shared `Some(v)`; both `None` produces
/// resolved `None`.
#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, DocumentedFields, JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// `[gate]` table — threshold + threshold-mode preset.
    #[serde(skip_serializing_if = "GateConfig::is_default")]
    pub gate: GateConfig,
    /// `[output]` table — format selector + scorecard labels.
    #[serde(skip_serializing_if = "OutputConfig::is_default")]
    pub output: OutputConfig,
    /// `[walk]` table — file-walker tuning.
    #[serde(skip_serializing_if = "WalkConfig::is_default")]
    pub walk: WalkConfig,
    /// `[scope]` table — relatedness scoping by structural boundary.
    #[serde(skip_serializing_if = "ScopeConfig::is_default")]
    pub scope: ScopeConfig,
    /// `[rust]` table — per-language overrides for the `dry4rs` adapter.
    /// Every knob in this table cascades: when set, it replaces the
    /// corresponding shared `[gate]`/`[output]`/`[walk]` value for the
    /// rust adapter ONLY; when unset, the adapter falls back to the
    /// shared value, then the [`crate::cli::AdapterMeta`] default,
    /// then the compiled-in fallback. Other adapters (dry4ts) are
    /// unaffected by anything in this table.
    #[serde(skip_serializing_if = "LanguageConfig::is_default")]
    pub rust: LanguageConfig,
    /// `[typescript]` table — per-language overrides for the future
    /// `dry4ts` adapter (v0.6+). Reserved at v0.1 so the cross-tool
    /// schema stays stable across the dry4rs / dry4ts cadence. Every
    /// knob cascades on top of `[gate]` / `[output]` / `[walk]` the
    /// same way `[rust]` does, for the typescript adapter ONLY.
    #[serde(skip_serializing_if = "LanguageConfig::is_default")]
    pub typescript: LanguageConfig,
}

/// `[gate]` table — Jaccard threshold + threshold-mode preset.
///
/// Both fields are `Option<T>` so the precedence merger can
/// distinguish "user set this in TOML" from "user left it at default"
/// (ADR D3 — CLI > config > [`AdapterMeta`] default > compiled-in
/// fallback).
///
/// [`AdapterMeta`]: crate::cli::AdapterMeta
#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, DocumentedFields, JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct GateConfig {
    /// Jaccard similarity threshold. CLI `--threshold` overrides this
    /// when both are set; compiled-in default `0.85` applies when
    /// neither is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    /// Threshold-mode preset (`strict` / `default` / `lenient`).
    /// CLI `--threshold-mode` overrides this when both are set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold_mode: Option<ThresholdMode>,
}

impl GateConfig {
    /// True when every field is its serde default — used by the
    /// top-level `#[serde(skip_serializing_if = ...)]` to omit empty
    /// tables from re-serialized TOML output.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.threshold.is_none() && self.threshold_mode.is_none()
    }
}

/// `[output]` table — format selector + scorecard labels.
///
/// At v0.1 the table carries the format selector + the title /
/// subtitle pair external consumers (e.g., the `dry-scorecard` GitHub
/// Action's sticky PR-comment header) render verbatim. `[output]`
/// exists as a table for forward-compat with v0.2+ `--output <path>`
/// (file destination) + reporter-specific knobs.
#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, DocumentedFields, JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct OutputConfig {
    /// Output format (`text` / `json`). CLI `--format` overrides this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<Format>,
    /// Scorecard title displayed by external consumers (e.g., the
    /// dry-scorecard GitHub Action's sticky PR comment header).
    /// Replaces the consumer-side `comment-preamble` action input
    /// stopgap. When unset, consumers may default to the tool name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Scorecard subtitle (second header line). Same intent as title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
}

impl OutputConfig {
    /// True when every field is its serde default.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.format.is_none() && self.title.is_none() && self.subtitle.is_none()
    }
}

/// `[walk]` table — file-walker tuning.
///
/// `extensions` is `Option<Vec<String>>` so an unset value (no
/// `extensions = [...]` in TOML) falls back to the adapter-supplied
/// default (`AdapterMeta::extensions`). An explicit empty list
/// (`extensions = []`) is a user-supplied override that disables the
/// extension filter — the loader preserves that semantic difference.
#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, DocumentedFields, JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct WalkConfig {
    /// Walk files normally excluded by `.gitignore` / `.ignore`. CLI
    /// `--include-ignored` overrides this when both are set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_ignored: Option<bool>,
    /// File extensions to analyze (without the leading dot). When
    /// `None`, the adapter default applies; when `Some(vec)`, the
    /// supplied list overrides (an empty vec disables the filter).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
}

impl WalkConfig {
    /// True when every field is its serde default.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.include_ignored.is_none() && self.extensions.is_none()
    }
}

/// `[scope]` table — relatedness scoping by structural boundary
/// (dry-rs#123, `adr-relatedness-scoping-model.md`).
///
/// Four orthogonal `Option<bool>` knobs gate which pairs of forms the
/// comparison engine is allowed to cluster, by the structural
/// relationship between the two forms' [`crate::domain::StructuralLocation`]:
///
/// - `within_crate` — allow pairs whose two forms share a crate / package.
/// - `across_crate` — allow pairs whose two forms live in different crates.
/// - `within_module` — allow pairs whose two forms share a module path.
/// - `across_module` — allow pairs whose two forms live in different modules.
///
/// The four axes are orthogonal (crate × module, within × across) and
/// map 1:1 to the user's mental model — NOT a single enum forcing an
/// unnatural product. Each defaults to "allow" (the resolved
/// [`crate::cli::ResolvedScope`] is all-true) so an unconfigured run
/// clusters every pair exactly as it did before scoping landed.
///
/// Every field is `Option<bool>` so the cascade resolver
/// ([`crate::cli::EffectiveConfig::resolve`]) can distinguish "user
/// set this in TOML" from "user left it unset" — per-language
/// `Some(v)` shadows the shared `[scope]` value; both `None` resolves
/// to the all-true default.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, DocumentedFields, JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct ScopeConfig {
    /// Allow clustering pairs whose two forms share a crate / package.
    /// CLI `--[no-]within-crate` overrides this when both are set.
    /// Default (unset) resolves to `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within_crate: Option<bool>,
    /// Allow clustering pairs whose two forms live in different crates.
    /// CLI `--[no-]across-crate` overrides this when both are set.
    /// Default (unset) resolves to `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub across_crate: Option<bool>,
    /// Allow clustering pairs whose two forms share a module path.
    /// CLI `--[no-]within-module` overrides this when both are set.
    /// Default (unset) resolves to `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within_module: Option<bool>,
    /// Allow clustering pairs whose two forms live in different modules.
    /// CLI `--[no-]across-module` overrides this when both are set.
    /// Default (unset) resolves to `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub across_module: Option<bool>,
}

impl ScopeConfig {
    /// True when every field is `None` — used by the top-level
    /// `#[serde(skip_serializing_if = ...)]` to omit an empty `[scope]`
    /// table from re-serialized TOML output.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.within_crate.is_none()
            && self.across_crate.is_none()
            && self.within_module.is_none()
            && self.across_module.is_none()
    }
}

/// Per-language override table — `[rust]` for the `dry4rs` adapter,
/// `[typescript]` for the future `dry4ts` adapter (v0.6+).
///
/// Every knob mirrors one in `[gate]` / `[output]` / `[walk]` as
/// `Option<T>`. A `Some(v)` here REPLACES the corresponding shared
/// value when the adapter resolves its effective config via
/// [`crate::cli::EffectiveConfig::resolve`] (dry-rs#78); a `None`
/// here falls back to the shared value. When BOTH are unset, the
/// resolved field stays `None` and the next precedence tier
/// ([`crate::cli::AdapterMeta`] default → compiled-in fallback) applies.
///
/// Adding a knob requires extending BOTH this struct AND the shared
/// section struct that owns the matching shared knob; the cascade
/// resolver's exhaustive destructure is the compile-time guard.
#[derive(
    Debug, Clone, Default, PartialEq, Serialize, Deserialize, DocumentedFields, JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct LanguageConfig {
    /// Per-language override for `[gate].threshold`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    /// Per-language override for `[gate].threshold_mode`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold_mode: Option<ThresholdMode>,
    /// Per-language override for `[output].format`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<Format>,
    /// Per-language override for `[output].title`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Per-language override for `[output].subtitle`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Per-language override for `[walk].include_ignored`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_ignored: Option<bool>,
    /// Per-language override for `[walk].extensions`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    /// Per-language override for `[scope].within_crate`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within_crate: Option<bool>,
    /// Per-language override for `[scope].across_crate`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub across_crate: Option<bool>,
    /// Per-language override for `[scope].within_module`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within_module: Option<bool>,
    /// Per-language override for `[scope].across_module`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub across_module: Option<bool>,
}

impl LanguageConfig {
    /// True when every field is `None` — used by the top-level
    /// `#[serde(skip_serializing_if = ...)]` to omit empty tables
    /// from re-serialized TOML output.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.threshold.is_none()
            && self.threshold_mode.is_none()
            && self.format.is_none()
            && self.title.is_none()
            && self.subtitle.is_none()
            && self.include_ignored.is_none()
            && self.extensions.is_none()
            && self.within_crate.is_none()
            && self.across_crate.is_none()
            && self.within_module.is_none()
            && self.across_module.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_has_all_empty_sub_tables() {
        let c = Config::default();
        assert!(c.gate.is_default());
        assert!(c.output.is_default());
        assert!(c.walk.is_default());
        assert!(c.scope.is_default());
        assert!(c.rust.is_default());
        assert!(c.typescript.is_default());
    }

    #[test]
    fn scope_config_is_default_when_every_field_is_none() {
        let c = ScopeConfig::default();
        assert!(c.is_default());
        assert_eq!(c.within_crate, None);
        assert_eq!(c.across_crate, None);
        assert_eq!(c.within_module, None);
        assert_eq!(c.across_module, None);
    }

    #[test]
    fn scope_config_any_field_some_unsets_default() {
        let with_within_crate = ScopeConfig {
            within_crate: Some(false),
            ..ScopeConfig::default()
        };
        assert!(!with_within_crate.is_default());

        let with_across_crate = ScopeConfig {
            across_crate: Some(false),
            ..ScopeConfig::default()
        };
        assert!(!with_across_crate.is_default());

        let with_within_module = ScopeConfig {
            within_module: Some(false),
            ..ScopeConfig::default()
        };
        assert!(!with_within_module.is_default());

        let with_across_module = ScopeConfig {
            across_module: Some(false),
            ..ScopeConfig::default()
        };
        assert!(!with_across_module.is_default());
    }

    #[test]
    fn scope_config_round_trips_through_toml() {
        let scoped = ScopeConfig {
            within_crate: Some(true),
            across_crate: Some(false),
            within_module: Some(true),
            across_module: Some(false),
        };
        let cfg = Config {
            scope: scoped.clone(),
            ..Config::default()
        };
        let serialized = toml::to_string_pretty(&cfg).expect("Config serializes to TOML");
        let parsed: Config = toml::from_str(&serialized).expect("Config round-trips through TOML");
        assert_eq!(parsed.scope, scoped);
    }

    #[test]
    fn empty_scope_table_omitted_from_serialized_toml() {
        let cfg = Config::default();
        let serialized = toml::to_string_pretty(&cfg).expect("Config serializes to TOML");
        assert!(
            !serialized.contains("[scope]"),
            "empty [scope] table must be omitted from re-serialized TOML"
        );
    }

    #[test]
    fn language_config_scope_knobs_participate_in_is_default() {
        for ctor in [
            |c: &mut LanguageConfig| c.within_crate = Some(false),
            |c: &mut LanguageConfig| c.across_crate = Some(false),
            |c: &mut LanguageConfig| c.within_module = Some(false),
            |c: &mut LanguageConfig| c.across_module = Some(false),
        ] {
            let mut lang = LanguageConfig::default();
            assert!(lang.is_default());
            ctor(&mut lang);
            assert!(
                !lang.is_default(),
                "a Some() scope knob must unset LanguageConfig::is_default"
            );
        }
    }

    #[test]
    fn gate_config_is_default_when_every_field_is_none() {
        let c = GateConfig::default();
        assert!(c.is_default());
        let c2 = GateConfig {
            threshold: Some(0.9),
            threshold_mode: None,
        };
        assert!(!c2.is_default());
    }

    #[test]
    fn output_config_is_default_when_every_field_is_none() {
        let c = OutputConfig::default();
        assert!(c.is_default());
        let with_format = OutputConfig {
            format: Some(Format::Json),
            title: None,
            subtitle: None,
        };
        assert!(!with_format.is_default());
        let with_title = OutputConfig {
            format: None,
            title: Some("hello".to_string()),
            subtitle: None,
        };
        assert!(!with_title.is_default());
        let with_subtitle = OutputConfig {
            format: None,
            title: None,
            subtitle: Some("world".to_string()),
        };
        assert!(!with_subtitle.is_default());
    }

    #[test]
    fn language_config_default_is_all_none() {
        let c = LanguageConfig::default();
        assert!(c.is_default());
    }

    #[test]
    fn language_config_any_field_some_unsets_default() {
        let with_threshold = LanguageConfig {
            threshold: Some(0.9),
            ..LanguageConfig::default()
        };
        assert!(!with_threshold.is_default());

        let with_title = LanguageConfig {
            title: Some("rust override".to_string()),
            ..LanguageConfig::default()
        };
        assert!(!with_title.is_default());

        let with_extensions = LanguageConfig {
            extensions: Some(vec!["rs".to_string()]),
            ..LanguageConfig::default()
        };
        assert!(!with_extensions.is_default());
    }

    #[test]
    fn walk_config_distinguishes_unset_from_empty_extensions() {
        let unset = WalkConfig::default();
        assert!(unset.extensions.is_none());
        assert!(unset.is_default());

        let empty = WalkConfig {
            include_ignored: None,
            extensions: Some(Vec::new()),
        };
        assert_eq!(empty.extensions, Some(Vec::new()));
        assert!(!empty.is_default());
    }
}
