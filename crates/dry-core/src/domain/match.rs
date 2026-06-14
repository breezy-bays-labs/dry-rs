//! The [`Match`] envelope — multi-score shape locked at v0.1.
//!
//! Per the nested-envelope ADR (`adr-nested-json-envelope.md`),
//! `Match` carries four score-related fields plus, additively, the
//! anti-unification [`Template`]:
//!
//! - `score` (`f64`) — pure Jaccard similarity, populated at v0.1.
//!   This is the mathematical anchor inherited verbatim from dry4clj;
//!   its semantics never change across schema versions.
//! - `structural_score` (`Option<f64>`) — **reserved-then-derived**:
//!   `null` until a [`Template`] is attached, then [`Match::with_template`]
//!   derives it from the holes (similarity with pure-rename divergence
//!   discounted; always `>= score`).
//! - `rename_count` (`Option<u32>`) — **reserved-then-derived**: `null`
//!   until a template is attached, then the count of pure-rename holes.
//! - `rename_density` (`Option<f64>`) — **reserved-then-derived**:
//!   `null` until a template is attached, then `rename_count / total_holes`.
//! - `template` (`Option<Template>`) — additive (epic #107): the
//!   anti-unification least-general-generalization over the cluster's
//!   member trees. `None` until the run loop attaches one.
//!
//! **Critical serde discipline — two DIFFERENT serde shapes by design**
//! (per the bot-suggestion-contract-stricting memory; AGENTS.md
//! "Locked wire shapes" calls this out so a code-review bot does not
//! "helpfully" normalize one to the other):
//!
//! - The three reserved score slots use **bare `#[serde(default)]`**,
//!   WITHOUT `skip_serializing_if`. They serialize as explicit `null`
//!   when unset (the locked v0.1 wire contract requires the keys
//!   visible). They are reserved-then-derived: still `null` when no
//!   template is attached, populated to numbers once one is.
//! - The new `template` field uses
//!   **`#[serde(default, skip_serializing_if = "Option::is_none")]`** —
//!   it is OMITTED entirely when `None`, so the v0.1 wire snapshot stays
//!   byte-identical when the feature is off. This omission (not `null`)
//!   is deliberate and is the OPPOSITE of the reserved-slot rule above.
//!
//! The struct does NOT carry `#[non_exhaustive]` — per the
//! enums-yes-structs-no rule, result structs evolve via constructors
//! and serde versioning, not the exhaustive-match attribute.

use serde::{Deserialize, Serialize};

use super::{FormRef, Hole, Template, Tier};

/// A cluster of structurally-similar forms reported by the comparison
/// engine.
///
/// At v0.1, `score` carries pure Jaccard similarity, the three reserved
/// scoring slots emit `null`, and `template` is omitted. Once the run
/// loop attaches an anti-unification [`Template`] via
/// [`Match::with_template`], the three reserved slots are DERIVED from
/// the template's holes — without bumping `schema_version`.
///
/// Construct via [`Match::new`] for the bare path (only Jaccard),
/// [`Match::with_scores`] when populating reserved slots directly in
/// tests, and [`Match::with_template`] to attach a template and derive
/// the rename signal from it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Match {
    /// The forms that participated in this cluster.
    pub forms: Vec<FormRef>,
    /// Jaccard similarity — pure mathematical truth. Stable across
    /// every schema version.
    pub score: f64,
    /// Structural similarity with pure-rename divergence discounted.
    /// **Reserved-then-derived**: `null` until a [`Template`] is
    /// attached, then derived by [`Match::with_template`] (always
    /// `>= score`).
    ///
    /// The wire encoding is `null` until then; bare `#[serde(default)]`
    /// (NO `skip_serializing_if`) keeps the key visible as `null` per
    /// the locked v0.1 wire shape.
    #[serde(default)]
    pub structural_score: Option<f64>,
    /// Number of pure-rename holes in the attached template (positions
    /// where every member binds a structurally-identical subtree —
    /// same fold `fp` — differing only by lexeme). **Reserved-then-
    /// derived**: `null` until a [`Template`] is attached.
    #[serde(default)]
    pub rename_count: Option<u32>,
    /// Pure-rename density (`rename_count / total_holes`).
    /// **Reserved-then-derived**: `null` until a [`Template`] is
    /// attached (and when the template has zero holes).
    #[serde(default)]
    pub rename_density: Option<f64>,
    /// Agentic-quality routing tier.
    pub tier: Tier,
    /// The anti-unification least-general-generalization (LGG) over the
    /// cluster's member trees (epic #107) — the shared structure with
    /// named holes where members diverge. **Additive**: `None` until the
    /// run loop attaches one.
    ///
    /// Serde shape DIFFERS from the reserved score slots by design:
    /// `skip_serializing_if = "Option::is_none"` OMITS the field when
    /// `None` (keeping the v0.1 wire snapshot byte-identical), where the
    /// reserved slots emit explicit `null`. See the module docs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,
}

impl Match {
    /// Construct a [`Match`] for the bare path: only `score` and `tier`
    /// are provided. The three reserved scoring slots default to `None`
    /// and serialize as `null`; `template` defaults to `None` and is
    /// omitted from the wire.
    #[must_use]
    pub const fn new(forms: Vec<FormRef>, score: f64, tier: Tier) -> Self {
        Self {
            forms,
            score,
            structural_score: None,
            rename_count: None,
            rename_density: None,
            tier,
            template: None,
        }
    }

    /// Construct a [`Match`] with every score slot populated and no
    /// template.
    ///
    /// Intended for tests covering the populated-score envelope shape.
    /// The derive-from-template path uses [`Match::with_template`].
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
            template: None,
        }
    }

    /// Attach an anti-unification [`Template`] and DERIVE the three
    /// reserved score slots from its holes.
    ///
    /// # Derived signal
    ///
    /// Let `H` be `template.holes`:
    ///
    /// - **`rename_count`** = the number of *pure-rename holes* in `H`.
    ///   A pure-rename hole is one where every member binds a single,
    ///   structurally-identical subtree — the same fold `fp` across all
    ///   members ([`crate::domain::Divergence::distinct`] `== 1`) — but the bound
    ///   subtrees differ by lexeme (`> 1` distinct lexeme). The LGG
    ///   surfaces these as holes precisely because the alpha-equivalent
    ///   rename collapses to one structural `fp` while carrying a
    ///   different surface name (a hole is emitted only when the
    ///   subtrees are NOT lexically identical), so the rename is exactly
    ///   the "same `fp`, divergent lexeme" hole. (See
    ///   [`crate::domain::Divergence`] and the LGG short-circuit in
    ///   `crate::comparison::antiunify`.)
    /// - **`rename_density`** = `rename_count / total_holes` when the
    ///   template has at least one hole; `None` when it has none
    ///   (a hole-free template — every member identical — has no
    ///   divergence to attribute, so density is undefined, not `0.0`).
    /// - **`structural_score`** = the structural similarity with
    ///   pure-rename divergence discounted: `score` lifted toward `1.0`
    ///   in proportion to how much of the hole-divergence is pure
    ///   rename. Concretely
    ///   `score + (1 - score) * (rename_count / total_holes)` (and just
    ///   `score` for a hole-free template). This is monotone in
    ///   `rename_count`, lies in `[score, 1]` ⊆ `[0, 1]` for any
    ///   `score ∈ [0, 1]`, and equals `score` when no hole is a rename
    ///   (renames count as structurally matching, so discounting them
    ///   can only raise the score — never lower it).
    ///
    /// All three slots stay `Some` after this call (the rename-count and
    /// structural-score are always derivable; only `rename_density` is
    /// `None` for the degenerate hole-free template).
    #[must_use]
    pub fn with_template(mut self, template: Template) -> Self {
        let total_holes = template.holes.len();
        let renames = count_pure_rename_holes(&template.holes);
        self.rename_count = Some(u32::try_from(renames).unwrap_or(u32::MAX));
        self.rename_density = rename_density(renames, total_holes);
        self.structural_score = Some(structural_score(self.score, renames, total_holes));
        self.template = Some(template);
        self
    }
}

/// Count the *pure-rename holes* among a template's holes.
///
/// A pure-rename hole is one whose [`crate::domain::Divergence`] reports
/// a single distinct binding fingerprint across all members
/// (`distinct == 1` — every member binds the SAME structural subtree)
/// yet whose bound subtrees carry more than one distinct lexeme. That
/// "same `fp`, divergent lexeme" shape is exactly an alpha-equivalent
/// rename the LGG could not fix (it only fixes positions that are
/// lexically identical too).
fn count_pure_rename_holes(holes: &[Hole]) -> usize {
    holes.iter().filter(|h| is_pure_rename_hole(h)).count()
}

/// Whether one hole is a pure rename: a single distinct binding `fp`
/// (every member structurally identical) but more than one distinct
/// lexeme among the bound elements.
fn is_pure_rename_hole(hole: &Hole) -> bool {
    if hole.divergence.distinct != 1 {
        return false;
    }
    // More than one distinct bound lexeme across members — short-circuit
    // on the first differing element (no allocation, no sort).
    let mut elements = hole.substitutions.iter().flat_map(|s| s.elements.iter());
    elements
        .next()
        .is_some_and(|first| elements.any(|e| e.lexeme != first.lexeme))
}

/// Derive `rename_density` = `rename_count / total_holes`, or `None`
/// when the template is hole-free (density is undefined, not `0.0`).
fn rename_density(renames: usize, total_holes: usize) -> Option<f64> {
    if total_holes == 0 {
        None
    } else {
        #[expect(
            clippy::cast_precision_loss,
            reason = "hole counts are tiny; f64 represents them exactly"
        )]
        Some(renames as f64 / total_holes as f64)
    }
}

/// Derive `structural_score`: lift `score` toward `1.0` in proportion to
/// the fraction of hole-divergence that is pure rename. Equals `score`
/// for a hole-free template (no divergence to discount). Always in
/// `[score, 1]` for `score ∈ [0, 1]`.
fn structural_score(score: f64, renames: usize, total_holes: usize) -> f64 {
    if total_holes == 0 {
        return score;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "hole counts are tiny; f64 represents them exactly"
    )]
    let rename_fraction = renames as f64 / total_holes as f64;
    score + (1.0 - score) * rename_fraction
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        DistinctValue, Divergence, HoleId, HoleKind, LineColumn, Span, SubElement, Substitution,
        TemplateNode,
    };
    use crate::test_support::make_form_ref_default as make_form_ref;

    /// A span helper for template fixtures.
    fn tsp() -> Span {
        Span::try_new(LineColumn::new(1, 0), LineColumn::new(1, 4)).unwrap()
    }

    /// Build a hole whose members all bind the SAME structural `fp`
    /// (`distinct == 1`) but with the given lexemes — a pure rename when
    /// the lexemes differ.
    fn rename_hole(index: u32, fp: u64, lexemes: &[&str]) -> Hole {
        let subs: Vec<Substitution> = lexemes
            .iter()
            .map(|lx| Substitution::new(vec![SubElement::new((*lx).to_string(), fp, tsp())]))
            .collect();
        let members = u32::try_from(lexemes.len()).unwrap();
        // distinct == 1: every member binds the same fp.
        let dv = vec![DistinctValue::new(fp, lexemes[0].to_string(), members)];
        Hole::new(
            HoleId::new(index),
            HoleKind::SubExpr,
            subs,
            Divergence::new(1, 0, members, dv),
        )
    }

    /// Build a hole where members bind DIFFERENT structural `fp`s
    /// (`distinct > 1`) — a genuine structural divergence, NOT a rename.
    fn divergent_hole(index: u32) -> Hole {
        let subs = vec![
            Substitution::new(vec![SubElement::new("a".into(), 1, tsp())]),
            Substitution::new(vec![SubElement::new("b".into(), 2, tsp())]),
        ];
        let dv = vec![
            DistinctValue::new(1, "a".into(), 1),
            DistinctValue::new(2, "b".into(), 1),
        ];
        Hole::new(
            HoleId::new(index),
            HoleKind::SubExpr,
            subs,
            Divergence::new(2, 1, 2, dv),
        )
    }

    /// A minimal fixed root for a template carrying `holes`.
    fn template_with(holes: Vec<Hole>) -> Template {
        let children = holes
            .iter()
            .map(|h| TemplateNode::Hole(h.id))
            .collect::<Vec<_>>();
        Template::new(
            TemplateNode::Fixed {
                label: "Block".into(),
                children,
                leaf_lexeme: None,
            },
            holes,
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

    // ---- P5: rename-signal derivation from the template (epic #107) ----

    #[test]
    fn new_defaults_template_to_none() {
        let m = Match::new(vec![make_form_ref()], 0.92, Tier::ReviewFirst);
        assert_eq!(m.template, None);
    }

    #[test]
    fn with_template_derives_one_pure_rename_hole() {
        // Worked-example shape: one optional/divergent hole + one pure-
        // rename hole (same fp 111, lexemes y/z/total). rename_count = 1,
        // total_holes = 2, density = 1/2 = 0.5.
        let holes = vec![divergent_hole(0), rename_hole(1, 111, &["y", "z", "total"])];
        let template = template_with(holes);
        let m = Match::new(vec![make_form_ref()], 0.88, Tier::ReviewFirst).with_template(template);

        assert_eq!(m.rename_count, Some(1), "exactly one hole is a pure rename");
        assert_eq!(
            m.rename_density,
            Some(0.5),
            "rename_density = rename_count / total_holes = 1/2"
        );
        // structural_score = 0.88 + (1 - 0.88) * (1/2) = 0.94, >= score.
        let ss = m.structural_score.unwrap();
        assert!((ss - 0.94).abs() < 1e-9, "structural_score derived: {ss}");
        assert!(ss >= m.score, "structural_score must be >= score");
        assert!(m.template.is_some(), "template attached");
    }

    #[test]
    fn with_template_counts_only_renames_not_structural_divergence() {
        // A hole with distinct fps (genuine structural divergence) is NOT
        // a rename; only the same-fp-different-lexeme hole counts.
        let holes = vec![
            divergent_hole(0),
            divergent_hole(1),
            rename_hole(2, 500, &["count", "total"]),
        ];
        let m = Match::new(vec![make_form_ref()], 0.85, Tier::ReviewFirst)
            .with_template(template_with(holes));
        assert_eq!(m.rename_count, Some(1));
        // density = 1/3.
        assert!((m.rename_density.unwrap() - (1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn with_template_same_fp_same_lexeme_is_not_a_rename() {
        // distinct == 1 but only ONE lexeme across all members: this is a
        // fixed value the LGG would not even hole, but if it surfaces it
        // is NOT a rename (no lexeme divergence).
        let holes = vec![rename_hole(0, 111, &["x", "x", "x"])];
        let m = Match::new(vec![make_form_ref()], 0.9, Tier::AutoRefactor)
            .with_template(template_with(holes));
        assert_eq!(m.rename_count, Some(0), "single lexeme is not a rename");
        assert_eq!(m.rename_density, Some(0.0));
        // No renames -> structural_score == score.
        assert!((m.structural_score.unwrap() - 0.9).abs() < 1e-9);
    }

    #[test]
    fn with_template_hole_free_yields_zero_count_none_density() {
        // A hole-free template (every member identical): no divergence to
        // attribute. rename_count = 0, density = None (undefined, not 0).
        let template = template_with(vec![]);
        let m = Match::new(vec![make_form_ref()], 1.0, Tier::AutoRefactor).with_template(template);
        assert_eq!(m.rename_count, Some(0));
        assert_eq!(m.rename_density, None, "hole-free density is undefined");
        assert_eq!(m.structural_score, Some(1.0));
    }

    #[test]
    fn with_template_structural_score_in_unit_interval_and_ge_score() {
        // P5 invariant: structural_score in [score, 1] for any score in
        // [0, 1], any rename/total ratio.
        for &score in &[0.0_f64, 0.5, 0.85, 0.95, 1.0] {
            for (renames, total) in [(0_u32, 1_u32), (1, 1), (1, 2), (2, 3), (3, 3)] {
                let mut holes: Vec<Hole> = Vec::new();
                for i in 0..renames {
                    holes.push(rename_hole(i, 100 + u64::from(i), &["a", "b"]));
                }
                for i in renames..total {
                    holes.push(divergent_hole(i));
                }
                let m =
                    Match::new(vec![], score, Tier::Advisory).with_template(template_with(holes));
                let ss = m.structural_score.unwrap();
                assert!(ss >= score - 1e-12, "ss {ss} >= score {score}");
                assert!((0.0..=1.0).contains(&ss), "ss {ss} in [0,1]");
                assert_eq!(m.rename_count, Some(renames));
            }
        }
    }

    // ---- WIRE GATE: template None byte-identical; populated round-trip ----

    #[test]
    fn template_none_omits_field_from_wire() {
        // The wire-gate guarantee: with template None, the serialized
        // Match is byte-identical to the pre-template v0.1 shape — the
        // `template` key is ABSENT (skip_serializing_if), while the three
        // reserved score slots STILL emit explicit `null`.
        let m = Match::new(vec![], 0.92, Tier::ReviewFirst);
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            !json.contains("\"template\""),
            "template must be omitted when None, got: {json}"
        );
        assert!(json.contains("\"structural_score\":null"), "json: {json}");
        assert!(json.contains("\"rename_count\":null"), "json: {json}");
        assert!(json.contains("\"rename_density\":null"), "json: {json}");
        // Exact byte shape of the v0.1 Match with empty forms.
        assert_eq!(
            json,
            r#"{"forms":[],"score":0.92,"structural_score":null,"rename_count":null,"rename_density":null,"tier":"review_first"}"#
        );
    }

    #[test]
    fn populated_template_round_trips() {
        let holes = vec![divergent_hole(0), rename_hole(1, 111, &["y", "z", "total"])];
        let original = Match::new(vec![make_form_ref()], 0.88, Tier::ReviewFirst)
            .with_template(template_with(holes));
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("\"template\""), "template emitted when Some");
        // Reserved slots are now NUMBERS, not null (reserved-then-derived).
        assert!(json.contains("\"rename_count\":1"), "json: {json}");
        let back: Match = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }
}
