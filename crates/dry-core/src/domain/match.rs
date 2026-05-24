//! The [`Match`] envelope — multi-score shape locked at v0.1.
//!
//! Per the nested-envelope ADR (`adr-nested-json-envelope.md`),
//! `Match` carries four score-related fields, **all four serialized
//! at v0.1**:
//!
//! - `score` (`f64`) — pure Jaccard similarity, populated at v0.1.
//!   This is the mathematical anchor inherited verbatim from dry4clj;
//!   its semantics never change across schema versions.
//! - `structural_score` (`Option<f64>`) — null at v0.1, populates at
//!   v0.2+ when rename signal lands.
//! - `rename_count` (`Option<u32>`) — null at v0.1, populates at v0.2+.
//! - `rename_density` (`Option<f64>`) — null at v0.1, populates at v0.2+.
//!
//! **Critical serde discipline**: the three reserved `Option<T>` slots
//! use `#[serde(default)]` **without** `skip_serializing_if = "Option::is_none"`.
//! The v0.1 wire contract requires them visible as `null`, not omitted.
//! Skipping them would contradict the locked shape. See the ADR's
//! "Note on serde attributes" section for the rationale.
//!
//! The struct does NOT carry `#[non_exhaustive]` — per the
//! enums-yes-structs-no rule, result structs evolve via constructors
//! and serde versioning, not the exhaustive-match attribute.

use serde::{Deserialize, Serialize};

use super::{FormRef, Tier};

/// A cluster of structurally-similar forms reported by the comparison
/// engine.
///
/// At v0.1, `score` carries pure Jaccard similarity and the three
/// reserved scoring slots emit `null`. At v0.2+, rename signal
/// populates `structural_score` / `rename_count` / `rename_density`
/// without bumping `schema_version`.
///
/// Construct via [`Match::new`] for the v0.1 path (only Jaccard) and
/// [`Match::with_scores`] when populating reserved slots in tests or
/// at v0.2+.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Match {
    /// The forms that participated in this cluster.
    pub forms: Vec<FormRef>,
    /// Jaccard similarity — pure mathematical truth. Stable across
    /// every schema version.
    pub score: f64,
    /// Structural similarity excluding rename effects. **Reserved at
    /// v0.1**; populates at v0.2+ as part of the rename-signal
    /// feature.
    ///
    /// The wire encoding is `null` until then; `#[serde(default)]`
    /// ensures backward-compat parsers tolerate the absence on
    /// deserialization.
    #[serde(default)]
    pub structural_score: Option<f64>,
    /// Number of placeholder renames between the two forms.
    /// **Reserved at v0.1**; populates at v0.2+.
    #[serde(default)]
    pub rename_count: Option<u32>,
    /// Per-token rename density (`rename_count / token_count`).
    /// **Reserved at v0.1**; populates at v0.2+.
    #[serde(default)]
    pub rename_density: Option<f64>,
    /// Agentic-quality routing tier.
    pub tier: Tier,
}

impl Match {
    /// Construct a [`Match`] for the v0.1 path: only `score` and `tier`
    /// are provided. The three reserved scoring slots default to
    /// `None` and serialize as `null`.
    #[must_use]
    pub const fn new(forms: Vec<FormRef>, score: f64, tier: Tier) -> Self {
        Self {
            forms,
            score,
            structural_score: None,
            rename_count: None,
            rename_density: None,
            tier,
        }
    }

    /// Construct a [`Match`] with every score slot populated.
    ///
    /// Intended for tests covering the v0.2+ envelope shape and for
    /// the comparison engine's rename-signal path once it lands. The
    /// v0.1 emit site uses [`Match::new`] exclusively.
    #[must_use]
    pub const fn with_scores(
        forms: Vec<FormRef>,
        score: f64,
        structural_score: Option<f64>,
        rename_count: Option<u32>,
        rename_density: Option<f64>,
        tier: Tier,
    ) -> Self {
        Self {
            forms,
            score,
            structural_score,
            rename_count,
            rename_density,
            tier,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::{FilePath, FormKind, LineColumn, Span};

    fn make_form_ref() -> FormRef {
        FormRef::new(
            FilePath::from(PathBuf::from("src/foo.rs")),
            Span::try_new(LineColumn::new(1, 0), LineColumn::new(3, 12)).unwrap(),
            FormKind::Production,
        )
    }

    #[test]
    fn new_defaults_three_reserved_slots_to_none() {
        let m = Match::new(vec![make_form_ref()], 0.92, Tier::ReviewFirst);
        assert!((m.score - 0.92).abs() < f64::EPSILON);
        assert_eq!(m.structural_score, None);
        assert_eq!(m.rename_count, None);
        assert_eq!(m.rename_density, None);
        assert_eq!(m.tier, Tier::ReviewFirst);
    }

    #[test]
    fn with_scores_stores_all_provided_values() {
        let m = Match::with_scores(
            vec![make_form_ref()],
            0.95,
            Some(1.0),
            Some(0),
            Some(0.0),
            Tier::AutoRefactor,
        );
        assert!((m.score - 0.95).abs() < f64::EPSILON);
        assert_eq!(m.structural_score, Some(1.0));
        assert_eq!(m.rename_count, Some(0));
        assert_eq!(m.rename_density, Some(0.0));
    }

    #[test]
    fn serializes_three_reserved_slots_as_null_at_v0_1() {
        // Load-bearing wire-contract assertion: the three reserved
        // fields MUST appear as explicit `null` in the v0.1 output,
        // not be omitted. This is the locked shape the envelope ADR
        // commits to.
        let m = Match::new(vec![], 0.92, Tier::ReviewFirst);
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            json.contains("\"structural_score\":null"),
            "structural_score should serialize as null, got: {json}"
        );
        assert!(
            json.contains("\"rename_count\":null"),
            "rename_count should serialize as null, got: {json}"
        );
        assert!(
            json.contains("\"rename_density\":null"),
            "rename_density should serialize as null, got: {json}"
        );
    }

    #[test]
    fn serializes_populated_scores_as_numbers() {
        let m = Match::with_scores(
            vec![],
            0.95,
            Some(1.0),
            Some(0),
            Some(0.0),
            Tier::AutoRefactor,
        );
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"structural_score\":1.0"), "json: {json}");
        assert!(json.contains("\"rename_count\":0"), "json: {json}");
        assert!(json.contains("\"rename_density\":0.0"), "json: {json}");
    }

    #[test]
    fn deserializes_explicit_null_slots() {
        // Forward-compat: a v0.1 envelope hits a v0.1 parser; nulls
        // must round-trip into `None`.
        let json = r#"{
            "forms": [],
            "score": 0.92,
            "structural_score": null,
            "rename_count": null,
            "rename_density": null,
            "tier": "review_first"
        }"#;
        let m: Match = serde_json::from_str(json).expect("must deserialize");
        assert_eq!(m.structural_score, None);
        assert_eq!(m.rename_count, None);
        assert_eq!(m.rename_density, None);
    }

    #[test]
    fn deserializes_missing_reserved_slots_via_serde_default() {
        // Backward-compat over inputs that omit the slots entirely:
        // `#[serde(default)]` produces `None`. This is the path a
        // future producer-agnostic parser uses if upstream omits a
        // null instead of emitting one.
        let json = r#"{
            "forms": [],
            "score": 0.92,
            "tier": "review_first"
        }"#;
        let m: Match = serde_json::from_str(json).expect("must deserialize without reserved slots");
        assert_eq!(m.structural_score, None);
        assert_eq!(m.rename_count, None);
        assert_eq!(m.rename_density, None);
    }

    #[test]
    fn round_trips_with_populated_scores() {
        let original = Match::with_scores(
            vec![make_form_ref()],
            0.95,
            Some(0.97),
            Some(2),
            Some(0.04),
            Tier::AutoRefactor,
        );
        let json = serde_json::to_string(&original).unwrap();
        let back: Match = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn round_trips_with_reserved_slots_null() {
        let original = Match::new(vec![make_form_ref()], 0.88, Tier::ReviewFirst);
        let json = serde_json::to_string(&original).unwrap();
        let back: Match = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }
}
