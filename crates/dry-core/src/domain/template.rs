//! Anti-unification template POD types — [`Template`],
//! [`TemplateNode`], [`HoleId`], [`Hole`], [`HoleKind`],
//! [`Substitution`], [`SubElement`], [`Divergence`], [`DistinctValue`].
//!
//! These are the result types of the first-order anti-unification
//! (least general generalization, LGG) pass over a cluster's member
//! [`crate::domain::NormalizedTree`]s (epic #107). The LGG *algorithm*
//! lives in `crate::comparison::antiunify`; the *data* it produces lives
//! here in the domain so that [`crate::domain::Match`] (also domain) can
//! carry a [`Template`] without `domain` depending on `comparison` (the
//! dependency direction must stay strictly inward — domain knows nothing
//! of the engine that fills these types).
//!
//! A [`Template`] is the shared structure of a cluster — its
//! [`TemplateNode`] tree with named [`Hole`]s where the members diverge
//! — plus, per hole, the per-member [`Substitution`] that fills it and a
//! [`Divergence`] summary the d-slider thresholds on.
//!
//! Per the hexagonal layering ADR
//! (`ops/decisions/dry-rs/adr-hexagonal-layout.md`), this module imports
//! no AST library and performs no I/O. Every type is POD with canonical
//! constructors and serde derives only.
//!
//! # Wire & `#[non_exhaustive]` discipline
//!
//! The two enums ([`TemplateNode`], [`HoleKind`]) carry
//! `#[non_exhaustive]` (enums-YES — consumer pattern-match concern). The
//! result structs evolve via constructors, NOT `#[non_exhaustive]`
//! (structs-NO), per the AGENTS.md struct discipline. The wire is
//! **lossy by design**: a [`SubElement`]'s `fp` is one-way and its
//! `lexeme` is for display — no consumer reconstruction is claimed (the
//! in-engine `instantiate` reconstruction, checkable against the trees
//! the LGG consumed, is the only reconstruction contract).

use serde::{Deserialize, Serialize};

use super::Span;

/// A generalized template over a cluster's member trees: the shared
/// structure ([`TemplateNode`] tree) plus the per-hole divergence data.
///
/// `holes` is indexed by [`HoleId::index`] — `holes[h.index]` carries
/// the substitutions and divergence for the hole referenced by
/// `TemplateNode::Hole(h)` in `root`. Result struct: evolves via
/// constructors, NOT `#[non_exhaustive]` (AGENTS.md struct discipline).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    /// Root of the shared-structure tree with [`TemplateNode::Hole`]
    /// markers at the divergent positions.
    pub root: TemplateNode,
    /// Per-hole substitution + divergence data, indexed by
    /// [`HoleId::index`].
    pub holes: Vec<Hole>,
}

impl Template {
    /// Construct a [`Template`] from its root node and hole table.
    #[must_use]
    pub const fn new(root: TemplateNode, holes: Vec<Hole>) -> Self {
        Self { root, holes }
    }
}

/// A node in a [`Template`]'s shared-structure tree.
///
/// Either a [`Fixed`](TemplateNode::Fixed) node (structure shared by
/// every member, recursed) or a [`Hole`](TemplateNode::Hole) marking a
/// divergent position whose per-member bindings live in
/// [`Template::holes`].
///
/// Wire shape (`#[serde(tag = "node")]`): a fixed node renders as
/// `{"node":"fixed","label":…,"children":[…],"leaf_lexeme":…?}` and a
/// hole as `{"node":"hole","index":N}` (the [`HoleId`] flattens under
/// the internal tag). Tagged-union enum → `#[non_exhaustive]`
/// (AGENTS.md enum discipline).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum TemplateNode {
    /// Structure shared by every member at this position.
    Fixed {
        /// Adapter node-discriminator label (e.g. `"ExprBinary"`),
        /// shared by every member at this position.
        label: String,
        /// Ordered children; a leaf node has an empty `children`.
        children: Vec<TemplateNode>,
        /// `Some` when every member's node here is a leaf with the
        /// same lexeme (a fixed leaf); `None` for internal nodes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leaf_lexeme: Option<String>,
    },
    /// A divergent position; the per-member bindings live in
    /// [`Template::holes`]`[index]`.
    Hole(HoleId),
}

/// Stable identifier for a [`Hole`] — its index into
/// [`Template::holes`]. Allocated in left-to-right pre-order so the
/// numbering is deterministic and permutation-stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HoleId {
    /// Index into [`Template::holes`].
    pub index: u32,
}

impl HoleId {
    /// Construct a [`HoleId`] from its index.
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self { index }
    }
}

/// Per-member bindings + divergence for one [`Hole`].
///
/// `substitutions` is in the SAME order as the LGG's input `members`
/// slice (index join): `substitutions[k]` is what member `k` binds at
/// this hole. Result struct, NOT `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hole {
    /// This hole's identifier (its index into [`Template::holes`]).
    pub id: HoleId,
    /// What kind of position this hole generalizes.
    pub kind: HoleKind,
    /// Per-member bindings, in `members` order (index join).
    pub substitutions: Vec<Substitution>,
    /// Divergence summary across the member bindings.
    pub divergence: Divergence,
}

impl Hole {
    /// Construct a [`Hole`] from its parts.
    #[must_use]
    pub const fn new(
        id: HoleId,
        kind: HoleKind,
        substitutions: Vec<Substitution>,
        divergence: Divergence,
    ) -> Self {
        Self {
            id,
            kind,
            substitutions,
            divergence,
        }
    }
}

/// What a [`Hole`] generalizes. Enum → `#[non_exhaustive]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum HoleKind {
    /// A scalar sub-expression position — each member binds exactly one
    /// subtree.
    SubExpr,
    /// A block / statement-group position — each member binds exactly
    /// one (block) subtree.
    Block,
    /// A position absent in at least one member — that member binds zero
    /// elements.
    Optional,
    /// A repetition group — at least one member binds more than one
    /// element.
    Variadic,
}

/// One member's binding at a [`Hole`].
///
/// `elements.len()` carries the binding shape: `0` = absent (Optional),
/// `1` = scalar (`SubExpr` / `Block`), `N > 1` = a variadic repetition
/// group. Result struct, NOT `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Substitution {
    /// The subtree(s) this member binds at the hole.
    pub elements: Vec<SubElement>,
}

impl Substitution {
    /// Construct a [`Substitution`] from its element list.
    #[must_use]
    pub const fn new(elements: Vec<SubElement>) -> Self {
        Self { elements }
    }
}

/// A single bound subtree inside a [`Substitution`].
///
/// `lexeme` is display-only (the wire is lossy by design); `fp` is the
/// subtree fold fingerprint that drives divergence; `span` locates the
/// binding in source. Result struct, NOT `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubElement {
    /// Display label for the bound subtree (leaf lexeme, or the node
    /// label for an internal subtree). One-way — not reconstructable.
    pub lexeme: String,
    /// Fold fingerprint of the bound subtree (the divergence key).
    pub fp: u64,
    /// Source range of the bound subtree.
    pub span: Span,
}

impl SubElement {
    /// Construct a [`SubElement`] from a bound subtree's display
    /// lexeme, fingerprint, and span.
    #[must_use]
    pub const fn new(lexeme: String, fp: u64, span: Span) -> Self {
        Self { lexeme, fp, span }
    }
}

/// Divergence summary for one [`Hole`] — the d-slider's backend
/// contract.
///
/// The baseline is the **modal** element fingerprint (the binding `fp`
/// most members agree on; ties break by `u64` order). `differing`
/// counts the members whose binding `fp` differs from the modal value;
/// `distinct` counts the distinct binding `fp`s; `members` is N. The
/// d-slider thresholds on `differing` at its low end (holes with
/// `differing >= d` are non-increasing in d — property P4d) and uses
/// `distinct == members` for the d = n (all-differ) endpoint. Result
/// struct, NOT `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Divergence {
    /// Number of distinct binding fingerprints across the members.
    pub distinct: u32,
    /// Number of members whose binding `fp` differs from the modal
    /// baseline.
    pub differing: u32,
    /// Number of members (N).
    pub members: u32,
    /// Per-distinct-value tallies, ordered by descending `count` then
    /// ascending `fp`.
    pub distinct_values: Vec<DistinctValue>,
}

impl Divergence {
    /// Construct a [`Divergence`] from its parts.
    #[must_use]
    pub const fn new(
        distinct: u32,
        differing: u32,
        members: u32,
        distinct_values: Vec<DistinctValue>,
    ) -> Self {
        Self {
            distinct,
            differing,
            members,
            distinct_values,
        }
    }
}

/// One distinct binding value in a [`Divergence`] tally.
///
/// Result struct, NOT `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistinctValue {
    /// The binding fingerprint (`0` is the synthetic "absent" key for
    /// Optional holes).
    pub fp: u64,
    /// A representative display lexeme for this value (`"<absent>"` for
    /// the absent key).
    pub lexeme: String,
    /// How many members bind this value.
    pub count: u32,
}

impl DistinctValue {
    /// Construct a [`DistinctValue`] from its parts.
    #[must_use]
    pub const fn new(fp: u64, lexeme: String, count: u32) -> Self {
        Self { fp, lexeme, count }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::LineColumn;

    fn sp() -> Span {
        Span::try_new(LineColumn::new(1, 0), LineColumn::new(1, 4)).expect("valid span")
    }

    #[test]
    fn template_node_hole_serializes_with_internal_tag() {
        let node = TemplateNode::Hole(HoleId::new(0));
        assert_eq!(
            serde_json::to_string(&node).unwrap(),
            r#"{"node":"hole","index":0}"#
        );
    }

    #[test]
    fn template_node_fixed_leaf_carries_lexeme() {
        let node = TemplateNode::Fixed {
            label: "ExprLocal".into(),
            children: vec![],
            leaf_lexeme: Some("x".into()),
        };
        assert_eq!(
            serde_json::to_string(&node).unwrap(),
            r#"{"node":"fixed","label":"ExprLocal","children":[],"leaf_lexeme":"x"}"#
        );
    }

    #[test]
    fn template_node_fixed_internal_skips_none_lexeme() {
        let node = TemplateNode::Fixed {
            label: "Block".into(),
            children: vec![],
            leaf_lexeme: None,
        };
        assert_eq!(
            serde_json::to_string(&node).unwrap(),
            r#"{"node":"fixed","label":"Block","children":[]}"#
        );
    }

    #[test]
    fn hole_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&HoleKind::SubExpr).unwrap(),
            "\"sub_expr\""
        );
        assert_eq!(
            serde_json::to_string(&HoleKind::Variadic).unwrap(),
            "\"variadic\""
        );
    }

    #[test]
    fn template_round_trips_through_json() {
        let template = Template::new(
            TemplateNode::Fixed {
                label: "Block".into(),
                children: vec![TemplateNode::Hole(HoleId::new(0))],
                leaf_lexeme: None,
            },
            vec![Hole::new(
                HoleId::new(0),
                HoleKind::SubExpr,
                vec![
                    Substitution::new(vec![SubElement::new("y".into(), 111, sp())]),
                    Substitution::new(vec![SubElement::new("z".into(), 111, sp())]),
                ],
                Divergence::new(1, 0, 2, vec![DistinctValue::new(111, "y".into(), 2)]),
            )],
        );
        let json = serde_json::to_string(&template).unwrap();
        let back: Template = serde_json::from_str(&json).unwrap();
        assert_eq!(back, template);
    }

    #[test]
    fn constructors_store_fields() {
        let sub = Substitution::new(vec![SubElement::new("a".into(), 7, sp())]);
        assert_eq!(sub.elements.len(), 1);
        assert_eq!(sub.elements[0].fp, 7);
        let dv = DistinctValue::new(0, "<absent>".into(), 2);
        assert_eq!(dv.count, 2);
        let d = Divergence::new(2, 1, 3, vec![dv]);
        assert_eq!(d.distinct, 2);
        assert_eq!(d.differing, 1);
        assert_eq!(d.members, 3);
    }
}
