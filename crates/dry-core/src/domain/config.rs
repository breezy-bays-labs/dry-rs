//! POD config types deserialized from `dry4rs.toml` (or any other
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

use serde::{Deserialize, Serialize};

use crate::cli::{Format, ThresholdMode};

/// Top-level config tree deserialized from `dry4rs.toml`.
///
/// Forward-compat surface: new tables (e.g., reserved `[delta]` /
/// `[reporter]` from the per-tool ADR V2) land additively as
/// `Option<NewConfig>` fields with `#[serde(default)]`. Existing
/// configs continue to parse after the schema gains a field.
///
/// `#[serde(deny_unknown_fields)]` per ADR D4 — typos in TOML surface
/// at parse time with a clear `path:line:key` message. NO silent
/// default fallback.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// `[gate]` table — threshold + threshold-mode preset.
    #[serde(skip_serializing_if = "GateConfig::is_default")]
    pub gate: GateConfig,
    /// `[output]` table — format selector.
    #[serde(skip_serializing_if = "OutputConfig::is_default")]
    pub output: OutputConfig,
    /// `[walk]` table — file-walker tuning.
    #[serde(skip_serializing_if = "WalkConfig::is_default")]
    pub walk: WalkConfig,
}

/// `[gate]` table — Jaccard threshold + threshold-mode preset.
///
/// Both fields are `Option<T>` so the precedence merger can
/// distinguish "user set this in TOML" from "user left it at default"
/// (ADR D3 — CLI > config > [`AdapterMeta`] default > compiled-in
/// fallback).
///
/// [`AdapterMeta`]: crate::cli::AdapterMeta
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

/// `[output]` table — format selector.
///
/// At v0.1 the `format` field is the only knob; `[output]` exists as
/// a table for forward-compat with v0.2+ `--output <path>` (file
/// destination) + reporter-specific knobs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct OutputConfig {
    /// Output format (`text` / `json`). CLI `--format` overrides this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<Format>,
}

impl OutputConfig {
    /// True when every field is its serde default.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.format.is_none()
    }
}

/// `[walk]` table — file-walker tuning.
///
/// `extensions` is `Option<Vec<String>>` so an unset value (no
/// `extensions = [...]` in TOML) falls back to the adapter-supplied
/// default (`AdapterMeta::extensions`). An explicit empty list
/// (`extensions = []`) is a user-supplied override that disables the
/// extension filter — the loader preserves that semantic difference.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_has_all_empty_sub_tables() {
        let c = Config::default();
        assert!(c.gate.is_default());
        assert!(c.output.is_default());
        assert!(c.walk.is_default());
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
    fn output_config_is_default_when_format_is_none() {
        let c = OutputConfig::default();
        assert!(c.is_default());
        let c2 = OutputConfig {
            format: Some(Format::Json),
        };
        assert!(!c2.is_default());
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
