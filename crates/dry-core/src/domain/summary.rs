//! Aggregated counters across all matches — [`Summary`].
//!
//! `Summary` lives at the truthful-gate boundary of the wire envelope
//! (`result.summary`) and at the shapeable-display boundary
//! (`view.summary`). Both projections share the same struct; the
//! reporter constructs each independently.
//!
//! `BTreeMap` (not `HashMap`) is used for the `by_*` counters so the
//! JSON key ordering is deterministic across runs and the
//! `wire_envelope_snapshot` insta test (lands with PR 7) stays
//! reproducible.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{FormKind, Tier};

/// Aggregated counters across the matches reported by the comparison
/// engine.
///
/// Field order matters for wire output (serde emits in declaration
/// order on structs). The deterministic `BTreeMap` ordering combined
/// with the fixed field order gives a byte-stable JSON projection for
/// the insta snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Total number of normalized forms surveyed (before similarity
    /// filtering).
    pub total_forms: u32,
    /// Count of matches grouped by routing tier.
    pub by_tier: BTreeMap<Tier, u32>,
    /// Count of matches grouped by form kind.
    pub by_kind: BTreeMap<FormKind, u32>,
}

impl Summary {
    /// Construct an empty [`Summary`] — zero forms, empty maps.
    #[must_use]
    pub fn new() -> Self {
        Self {
            total_forms: 0,
            by_tier: BTreeMap::new(),
            by_kind: BTreeMap::new(),
        }
    }
}

impl Default for Summary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_empty() {
        let s = Summary::new();
        assert_eq!(s.total_forms, 0);
        assert!(s.by_tier.is_empty());
        assert!(s.by_kind.is_empty());
    }

    #[test]
    fn default_matches_new() {
        assert_eq!(Summary::default(), Summary::new());
    }

    #[test]
    fn by_tier_ordering_is_deterministic() {
        // BTreeMap orders by key; Tier derives Ord by declaration:
        // AutoRefactor < ReviewFirst < Advisory. The wire output must
        // therefore emit `auto_refactor` before `review_first` etc.
        let mut by_tier = BTreeMap::new();
        by_tier.insert(Tier::Advisory, 4);
        by_tier.insert(Tier::AutoRefactor, 1);
        by_tier.insert(Tier::ReviewFirst, 12);

        let s = Summary {
            total_forms: 17,
            by_tier,
            by_kind: BTreeMap::new(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let auto_idx = json.find("auto_refactor").expect("auto_refactor present");
        let review_idx = json.find("review_first").expect("review_first present");
        let advisory_idx = json.find("advisory").expect("advisory present");
        assert!(
            auto_idx < review_idx,
            "auto_refactor must precede review_first: {json}"
        );
        assert!(
            review_idx < advisory_idx,
            "review_first must precede advisory: {json}"
        );
    }

    #[test]
    fn by_kind_ordering_is_deterministic() {
        // FormKind ord: Production < Test < Doctest.
        let mut by_kind = BTreeMap::new();
        by_kind.insert(FormKind::Doctest, 5);
        by_kind.insert(FormKind::Production, 380);
        by_kind.insert(FormKind::Test, 32);

        let s = Summary {
            total_forms: 417,
            by_tier: BTreeMap::new(),
            by_kind,
        };
        let json = serde_json::to_string(&s).unwrap();
        let prod_idx = json.find("production").expect("production present");
        let test_idx = json.find("\"test\"").expect("test present");
        let doctest_idx = json.find("doctest").expect("doctest present");
        assert!(prod_idx < test_idx, "production must precede test: {json}");
        assert!(test_idx < doctest_idx, "test must precede doctest: {json}");
    }

    #[test]
    fn serde_round_trips() {
        let mut by_tier = BTreeMap::new();
        by_tier.insert(Tier::Advisory, 4);
        by_tier.insert(Tier::ReviewFirst, 12);
        let mut by_kind = BTreeMap::new();
        by_kind.insert(FormKind::Production, 380);

        let original = Summary {
            total_forms: 412,
            by_tier,
            by_kind,
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: Summary = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }
}
