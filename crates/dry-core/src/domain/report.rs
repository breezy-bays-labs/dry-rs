//! Top-level analysis result — [`Report`].
//!
//! `Report` is the truthful-gate domain shape that lives at `result.*`
//! in the wire envelope (per the nested-envelope ADR). It carries the
//! full match set, the aggregated summary, and the gate verdict.
//!
//! CLI flags (`--top`, `--only-failing`) reshape the **view**
//! projection that lives at `view.*` in the envelope; they cannot
//! mutate a `Report`. The truthful-gate guarantee is structural —
//! `Report` is the source of truth.

use serde::{Deserialize, Serialize};

use super::{Match, Summary};

/// Top-level analysis result reported by the comparison engine.
///
/// `passed` is the gate verdict: `true` when no match exceeds the
/// configured threshold, `false` otherwise. CI consumers reading
/// `result.passed` are immune to view-side reshaping (`--top N`,
/// `--only-failing`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// All matches surfaced by the comparison engine, unfiltered.
    pub matches: Vec<Match>,
    /// Aggregated counters across `matches`.
    pub summary: Summary,
    /// `true` when the gate verdict is "no match exceeds threshold".
    pub passed: bool,
}

impl Report {
    /// Construct a [`Report`] from its component parts.
    #[must_use]
    pub const fn new(matches: Vec<Match>, summary: Summary, passed: bool) -> Self {
        Self {
            matches,
            summary,
            passed,
        }
    }

    /// Construct an empty passing [`Report`] — zero matches, empty
    /// summary, `passed == true`.
    #[must_use]
    pub fn empty_passed() -> Self {
        Self {
            matches: Vec::new(),
            summary: Summary::new(),
            passed: true,
        }
    }
}

impl Default for Report {
    fn default() -> Self {
        Self::empty_passed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Tier;
    use crate::test_support::make_form_ref_default as make_form_ref;

    #[test]
    fn empty_passed_is_passing_with_no_matches() {
        let r = Report::empty_passed();
        assert!(r.passed);
        assert!(r.matches.is_empty());
        assert_eq!(r.summary, Summary::new());
    }

    #[test]
    fn default_matches_empty_passed() {
        assert_eq!(Report::default(), Report::empty_passed());
    }

    #[test]
    fn new_stores_all_fields() {
        let m = Match::new(vec![make_form_ref()], 0.92, Tier::ReviewFirst);
        let r = Report::new(vec![m.clone()], Summary::new(), false);
        assert_eq!(r.matches, vec![m]);
        assert!(!r.passed);
    }

    #[test]
    fn serde_round_trips_with_matches() {
        let m = Match::new(vec![make_form_ref()], 0.92, Tier::ReviewFirst);
        let original = Report::new(vec![m], Summary::new(), false);
        let json = serde_json::to_string(&original).unwrap();
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn serde_round_trips_empty_passed() {
        let original = Report::empty_passed();
        let json = serde_json::to_string(&original).unwrap();
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }
}
