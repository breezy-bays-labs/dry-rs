//! Closed enums for form classification and routing — [`FormKind`],
//! [`Tier`], [`Severity`].
//!
//! Every public enum in `dry-core::domain` carries `#[non_exhaustive]`
//! per the nested-envelope ADR — external consumers pattern-match on
//! these variants, and the attribute keeps additions non-breaking.
//!
//! Wire-format vocabulary uses `snake_case` for compound variants
//! (`auto_refactor`, `review_first`) to match the JSON shape locked in
//! the envelope ADR; serde's `rename_all = "snake_case"` ensures the
//! rendering is mechanical, not a per-variant hand-spelling.

use serde::{Deserialize, Serialize};

/// What kind of form was normalized.
///
/// `Production` covers ordinary functions, methods, and definitions.
/// `Test` covers `#[test]`-annotated bodies and other test-harness
/// functions. `Doctest` covers documentation-test bodies extracted
/// from `///` blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum FormKind {
    /// Production-source forms (functions, methods, free-standing items).
    Production,
    /// Test-harness forms (`#[test]` / `#[cfg(test)]` / similar).
    Test,
    /// Documentation-test forms extracted from `///` blocks.
    Doctest,
}

impl FormKind {
    /// Stable label for this kind — the single source of truth shared
    /// by every reporter surface.
    ///
    /// The returned `&'static str` is byte-identical to the serde wire
    /// rendering (`#[serde(rename_all = "snake_case")]`): `production`
    /// / `test` / `doctest`. Reporters that need a display label call
    /// this instead of re-spelling the mapping, so a new variant breaks
    /// exactly one match arm.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Test => "test",
            Self::Doctest => "doctest",
        }
    }
}

/// Agentic-quality routing tier derived from a `Match`'s score.
///
/// The thresholds are fixed at v0.1:
///
/// - `AutoRefactor` — score ≥ 0.95. Agents may refactor without
///   human review when the structural match is exact (`structural_score == 1.0`,
///   populated at v0.2+).
/// - `ReviewFirst` — score ≥ 0.85. Agents propose a refactor; a human
///   confirms before merge.
/// - `Advisory` — score ≥ threshold but below 0.85. Surface as
///   information; no refactor proposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Score ≥ 0.95; safe-to-refactor when paired with v0.2+ rename signal.
    AutoRefactor,
    /// Score ≥ 0.85; agent proposes, human confirms.
    ReviewFirst,
    /// Score above threshold but below 0.85; surface as advisory.
    Advisory,
}

impl Tier {
    /// Stable label for this tier — the single source of truth shared
    /// by every reporter surface.
    ///
    /// The returned `&'static str` is byte-identical to the serde wire
    /// rendering (`#[serde(rename_all = "snake_case")]`): `auto_refactor`
    /// / `review_first` / `advisory`. Reporters that need a heading or
    /// message label call this instead of re-spelling the mapping, so a
    /// new variant breaks exactly one match arm.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutoRefactor => "auto_refactor",
            Self::ReviewFirst => "review_first",
            Self::Advisory => "advisory",
        }
    }
}

/// Display severity derived from [`Tier`] (cosmetic at v0.1; the
/// derivation rule populates at v0.3+ when the markdown / HTML
/// reporters land).
///
/// Mirrors the cross-tool severity vocabulary used by crap4rs and
/// scrap-rs (high / medium / low) so a unified dashboard renders all
/// three sensors with a single severity axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Highest visual weight.
    High,
    /// Mid weight.
    Medium,
    /// Lowest visual weight; advisory-style display.
    Low,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&FormKind::Production).unwrap(),
            "\"production\""
        );
        assert_eq!(serde_json::to_string(&FormKind::Test).unwrap(), "\"test\"");
        assert_eq!(
            serde_json::to_string(&FormKind::Doctest).unwrap(),
            "\"doctest\""
        );
    }

    #[test]
    fn form_kind_round_trips_through_json() {
        for kind in [FormKind::Production, FormKind::Test, FormKind::Doctest] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: FormKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn tier_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&Tier::AutoRefactor).unwrap(),
            "\"auto_refactor\""
        );
        assert_eq!(
            serde_json::to_string(&Tier::ReviewFirst).unwrap(),
            "\"review_first\""
        );
        assert_eq!(
            serde_json::to_string(&Tier::Advisory).unwrap(),
            "\"advisory\""
        );
    }

    #[test]
    fn tier_round_trips_through_json() {
        for tier in [Tier::AutoRefactor, Tier::ReviewFirst, Tier::Advisory] {
            let json = serde_json::to_string(&tier).unwrap();
            let back: Tier = serde_json::from_str(&json).unwrap();
            assert_eq!(back, tier);
        }
    }

    #[test]
    fn severity_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Severity::High).unwrap(), "\"high\"");
        assert_eq!(
            serde_json::to_string(&Severity::Medium).unwrap(),
            "\"medium\""
        );
        assert_eq!(serde_json::to_string(&Severity::Low).unwrap(), "\"low\"");
    }

    #[test]
    fn severity_round_trips_through_json() {
        for sev in [Severity::High, Severity::Medium, Severity::Low] {
            let json = serde_json::to_string(&sev).unwrap();
            let back: Severity = serde_json::from_str(&json).unwrap();
            assert_eq!(back, sev);
        }
    }

    #[test]
    fn form_kind_as_str_matches_serde_label() {
        // `as_str` is the single source of truth shared by reporters;
        // it MUST stay byte-identical to the serde wire rendering so the
        // text/markdown/github-annotations surfaces never drift from the
        // JSON envelope.
        for kind in [FormKind::Production, FormKind::Test, FormKind::Doctest] {
            let json = serde_json::to_string(&kind).unwrap();
            let unquoted = json.trim_matches('"');
            assert_eq!(kind.as_str(), unquoted, "kind label drifted from serde");
        }
    }

    #[test]
    fn tier_as_str_matches_serde_label() {
        for tier in [Tier::AutoRefactor, Tier::ReviewFirst, Tier::Advisory] {
            let json = serde_json::to_string(&tier).unwrap();
            let unquoted = json.trim_matches('"');
            assert_eq!(tier.as_str(), unquoted, "tier label drifted from serde");
        }
    }

    #[test]
    fn tier_ordering_matches_threshold_progression() {
        // Derived `PartialOrd` orders by declaration: AutoRefactor < ReviewFirst < Advisory.
        // The progression reflects "highest confidence" to "lowest"; downstream
        // sort sites should NOT rely on raw enum ordering for display sort
        // (display uses score directly).
        assert!(Tier::AutoRefactor < Tier::ReviewFirst);
        assert!(Tier::ReviewFirst < Tier::Advisory);
    }
}
