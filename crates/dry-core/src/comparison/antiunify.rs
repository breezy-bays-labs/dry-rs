//! First-order anti-unification (least general generalization, LGG)
//! over the member [`NormalizedTree`]s of a cluster (epic #107, PR 5).
//!
//! Where the comparison engine (`comparison/mod.rs`) decides *which*
//! forms cluster (Jaccard on bag-of-hashes), this module decides *what*
//! a cluster's shared shape is: it generalizes the N member trees into
//! a single [`Template`] — the common structure with named [`Hole`]s
//! where the members diverge — plus, per hole, the per-member
//! [`Substitution`] that fills it and a [`Divergence`] summary the
//! d-slider thresholds on.
//!
//! # First-order LGG
//!
//! Anti-unification computes the least general generalization of a set
//! of terms: the most specific template `T` such that every member is
//! an instance of `T` under some substitution. *First-order* means
//! holes stand for whole subtrees, not higher-order functions — a hole
//! binds one (scalar / sub-expr / block), zero (optional), or many
//! (variadic) subtrees per member.
//!
//! The generalization is computed over all N members **simultaneously**
//! (not by pairwise folding) so a hole's [`Substitution`] list is in
//! the SAME order as the input `members` slice — the index join the
//! wire contract relies on (`Hole.substitutions[k]` ⟷ `members[k]`).
//!
//! ## Alignment rules ([`align_node`])
//!
//! At each aligned position the N candidate nodes resolve to either a
//! [`TemplateNode::Fixed`] (shared structure, recursed) or a
//! [`TemplateNode::Hole`] (divergence):
//!
//! - **All `fp` equal** → the entire subtree is identical across every
//!   member; emit it verbatim as a fixed tree (the LGG short-circuit —
//!   equal fold fingerprint implies structurally-identical subtree).
//! - **Shared label, all internal, equal arity** → emit
//!   `Fixed { label, children }` and recurse child-wise positionally.
//! - **Shared label, all internal, differing arity** → emit
//!   `Fixed { label, children }` where [`lcs_children`] aligns the
//!   common children by an LCS over child `fp`s and the gaps become
//!   variadic / optional holes.
//! - **Otherwise** (labels differ, leaf-vs-internal mix, or shared-
//!   label leaves with differing lexeme) → a [`TemplateNode::Hole`],
//!   recording each member's subtree as a substitution element.
//!
//! ## Determinism
//!
//! [`HoleId`]s are allocated in stable left-to-right pre-order. The
//! [`lcs_children`] alignment tie-breaks on `(fp, within-member-child-
//! index)` — both intrinsic to the trees, never the member input order
//! — so the emitted [`Template`] is byte-identical across runs AND
//! across any permutation of the `members` slice (property P2). Every
//! ordering decision uses `f64`-free integer / `Ord` comparison; this
//! module touches no floats and no `HashSet` iteration order.
//!
//! ## Purity
//!
//! Per the hexagonal layering ADR this module imports no AST library
//! and performs no I/O. It consumes the POD [`NormalizedTree`] the
//! `TreeDeriverPort` (PR 4) produces and returns POD result types. The
//! wire is **lossy by design** (`fp` is one-way, `lexeme` is for
//! display): no consumer reconstruction is claimed. The in-engine
//! [`instantiate`] helper — checkable against the very trees the LGG
//! consumed — is the reconstruction contract (property P1).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::{NormalizedTree, Span};

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

/// Synthetic fingerprint for an "absent" binding in an [`HoleKind::Optional`]
/// substitution (a member that binds zero elements). Real subtree
/// fingerprints are xxh3 `u64`s; `0` is the documented absent sentinel
/// the divergence tally folds in (matching the worked-example JSON).
const ABSENT_FP: u64 = 0;

/// Display lexeme for an absent Optional binding.
const ABSENT_LEXEME: &str = "<absent>";

/// Compute the first-order least general generalization (LGG) of a
/// cluster's member trees.
///
/// Returns a [`Template`] whose [`Template::root`] captures the
/// structure shared by every member and whose [`Template::holes`]
/// records, per divergent position, the per-member [`Substitution`] (in
/// `members` order) and a [`Divergence`] summary.
///
/// Total and panic-free for every input (property P6): an empty slice
/// yields an empty-leaf root with no holes; a singleton yields its own
/// structure with no holes (a tree is the LGG of itself); identical
/// members yield a hole-free fixed template; mismatched arity / labels
/// resolve to holes.
///
/// Deterministic and permutation-stable (property P2): the same set of
/// members produces a byte-identical template regardless of slice
/// order, because every alignment decision is intrinsic to the trees.
#[must_use]
pub fn antiunify(members: &[NormalizedTree]) -> Template {
    let refs: Vec<&NormalizedTree> = members.iter().collect();
    let mut alloc = HoleAllocator::default();
    let root = align_node(&refs, &mut alloc);
    Template::new(root, alloc.into_holes())
}

/// Stable left-to-right pre-order [`HoleId`] allocator + hole-table
/// accumulator. `align_node` reserves an id BEFORE recursing into any
/// sibling so numbering follows pre-order traversal.
#[derive(Default)]
struct HoleAllocator {
    holes: Vec<Hole>,
}

impl HoleAllocator {
    /// Reserve the next [`HoleId`] (its index == current hole count).
    fn reserve(&mut self) -> HoleId {
        HoleId::new(u32::try_from(self.holes.len()).unwrap_or(u32::MAX))
    }

    /// Record a fully-built hole. Its `id.index` MUST equal the slot it
    /// lands in (guaranteed by `reserve` + push pairing in pre-order).
    fn push(&mut self, hole: Hole) {
        debug_assert_eq!(
            hole.id.index as usize,
            self.holes.len(),
            "hole id must match its slot index (pre-order allocation invariant)"
        );
        self.holes.push(hole);
    }

    fn into_holes(self) -> Vec<Hole> {
        self.holes
    }
}

/// Align N candidate nodes into one [`TemplateNode`], recording any
/// divergence as holes via `alloc`.
///
/// The decision ladder (see the module doc): all-`fp`-equal short-
/// circuits to a verbatim fixed subtree; shared-label internals recurse
/// (equal arity child-wise, differing arity via [`lcs_children`]);
/// everything else becomes a hole.
fn align_node(nodes: &[&NormalizedTree], alloc: &mut HoleAllocator) -> TemplateNode {
    match nodes {
        [] => empty_leaf_node(),
        [single] => fixed_from_tree(single),
        _ => align_many(nodes, alloc),
    }
}

/// A placeholder fixed leaf for the degenerate empty-members case.
fn empty_leaf_node() -> TemplateNode {
    TemplateNode::Fixed {
        label: String::new(),
        children: Vec::new(),
        leaf_lexeme: None,
    }
}

/// Align two-or-more candidate nodes (the general case).
///
/// The all-`fp`-equal short-circuit fires only when the subtrees are
/// also lexically identical (same lexemes, same shape ignoring span) —
/// a pure rename collapses to the same structural `fp` under the
/// normalizer's alpha-equivalence but carries a different lexeme, and
/// MUST surface as a hole (the rename signal), so equal `fp` alone is
/// not sufficient to fix a position.
fn align_many(nodes: &[&NormalizedTree], alloc: &mut HoleAllocator) -> TemplateNode {
    if all_structurally_identical(nodes) {
        // Identical structure AND lexemes ⟹ emit the first verbatim.
        return fixed_from_tree(nodes[0]);
    }
    if let Some(node) = try_align_shared_internal(nodes, alloc) {
        return node;
    }
    // Shared-label leaves with differing lexeme (a rename / literal
    // change), differing labels, or a leaf/internal mix: this position
    // is a scalar hole — Block when every bound node is block-like,
    // else SubExpr.
    make_hole(nodes, scalar_hole_kind(nodes), alloc)
}

/// Classify a SCALAR hole (one bound subtree per member) as
/// [`HoleKind::Block`] when every bound node is block-like (its label
/// names a block / statement-group construct), else [`HoleKind::SubExpr`].
///
/// The block vocabulary is the syn-shaped v0.1 label space (a known
/// seam — see `domain::tree`): a label that is exactly `"Block"` or
/// starts with `"Block"` / `"ExprBlock"` / `"Stmt"`. When dry4ts joins,
/// the block label set is reconciled across adapters with the rest of
/// the cross-language vocabulary.
fn scalar_hole_kind(nodes: &[&NormalizedTree]) -> HoleKind {
    if nodes.iter().all(|n| is_block_label(&n.label)) {
        HoleKind::Block
    } else {
        HoleKind::SubExpr
    }
}

/// Whether a node label names a block / statement-group construct.
fn is_block_label(label: &str) -> bool {
    label.starts_with("ExprBlock") || label.starts_with("Block") || label.starts_with("Stmt")
}

/// If every node shares one label and is internal, emit a `Fixed` node
/// and recurse (equal arity → child-wise; differing arity →
/// [`lcs_children`]). Returns `None` when the nodes are not all
/// shared-label internals (the caller then makes a hole).
fn try_align_shared_internal(
    nodes: &[&NormalizedTree],
    alloc: &mut HoleAllocator,
) -> Option<TemplateNode> {
    if !all_same_label(nodes) || !all_internal(nodes) {
        return None;
    }
    let label = nodes[0].label.clone();
    let children = if all_same_arity(nodes) {
        align_children_positional(nodes, alloc)
    } else {
        lcs_children(nodes, alloc)
    };
    Some(TemplateNode::Fixed {
        label,
        children,
        leaf_lexeme: None,
    })
}

/// Recurse position-by-position over equal-arity internal nodes.
fn align_children_positional(
    nodes: &[&NormalizedTree],
    alloc: &mut HoleAllocator,
) -> Vec<TemplateNode> {
    let arity = nodes[0].children.len();
    let mut children = Vec::with_capacity(arity);
    for pos in 0..arity {
        let at_pos: Vec<&NormalizedTree> = nodes.iter().map(|n| &n.children[pos]).collect();
        children.push(align_node(&at_pos, alloc));
    }
    children
}

/// Project a [`NormalizedTree`] verbatim into a fixed [`TemplateNode`]
/// (no holes) — used for identical subtrees and the singleton case.
fn fixed_from_tree(tree: &NormalizedTree) -> TemplateNode {
    let children = tree.children.iter().map(fixed_from_tree).collect();
    let leaf_lexeme = tree.leaf.as_ref().map(|l| l.lexeme.clone());
    TemplateNode::Fixed {
        label: tree.label.clone(),
        children,
        leaf_lexeme,
    }
}

/// Build a scalar / block hole: each member binds exactly its subtree
/// at this position. Reserves the id in pre-order BEFORE building the
/// substitutions so sibling holes number left-to-right.
fn make_hole(nodes: &[&NormalizedTree], kind: HoleKind, alloc: &mut HoleAllocator) -> TemplateNode {
    let id = alloc.reserve();
    let substitutions: Vec<Substitution> = nodes
        .iter()
        .map(|n| Substitution::new(vec![sub_element_of(n)]))
        .collect();
    let divergence = build_divergence(&substitutions);
    alloc.push(Hole::new(id, kind, substitutions, divergence));
    TemplateNode::Hole(id)
}

/// Build a hole from already-grouped per-member element lists (the
/// LCS-gap path, where a member may bind zero or many elements).
/// `kind` is classified from the element-count distribution.
fn make_grouped_hole(per_member: Vec<Vec<SubElement>>, alloc: &mut HoleAllocator) -> TemplateNode {
    let id = alloc.reserve();
    let kind = classify_hole_kind(&per_member);
    let substitutions: Vec<Substitution> = per_member.into_iter().map(Substitution::new).collect();
    let divergence = build_divergence(&substitutions);
    alloc.push(Hole::new(id, kind, substitutions, divergence));
    TemplateNode::Hole(id)
}

/// Classify a hole's [`HoleKind`] from its per-member element lists.
///
/// Precedence: any member binding MORE THAN ONE element ⟹ `Variadic`
/// (a repetition group is the strongest signal — it dominates even when
/// another member binds zero, since the hole must represent a
/// repetition); else any member binding ZERO ⟹ `Optional`; else every
/// member binds exactly one ⟹ `SubExpr`.
fn classify_hole_kind(per_member: &[Vec<SubElement>]) -> HoleKind {
    if per_member.iter().any(|e| e.len() > 1) {
        HoleKind::Variadic
    } else if per_member.iter().any(Vec::is_empty) {
        HoleKind::Optional
    } else {
        HoleKind::SubExpr
    }
}

/// Project a single node into a [`SubElement`]: a leaf binds its
/// lexeme; an internal node binds its label as the display lexeme (the
/// wire is lossy by design — `fp` carries the identity, `lexeme` is for
/// display only).
fn sub_element_of(node: &NormalizedTree) -> SubElement {
    let lexeme = node
        .leaf
        .as_ref()
        .map_or_else(|| node.label.clone(), |l| l.lexeme.clone());
    SubElement::new(lexeme, node.fp, node.span)
}

/// Align children of differing-arity internal nodes via a deterministic
/// LCS over child `fp`s, then turn the gaps into holes.
///
/// The LCS is computed pairwise between the first member's children and
/// each other member's children, intersected into the positions every
/// member agrees on (a conservative common subsequence — the anchors
/// shared by ALL members). Anchor positions become aligned children
/// (recursed); the children between consecutive anchors become a single
/// grouped hole whose per-member element list is that member's gap
/// run (0 elements ⟹ Optional, >1 ⟹ Variadic).
fn lcs_children(nodes: &[&NormalizedTree], alloc: &mut HoleAllocator) -> Vec<TemplateNode> {
    let anchors = common_anchor_fps(nodes);
    let mut out: Vec<TemplateNode> = Vec::new();
    // Per-member cursor into that member's children.
    let mut cursors = vec![0usize; nodes.len()];

    for &anchor_fp in &anchors {
        emit_gap_before_anchor(nodes, &mut cursors, anchor_fp, &mut out, alloc);
        emit_anchor(nodes, &mut cursors, &mut out, alloc);
    }
    // Trailing gap after the last anchor (or the whole thing if there
    // were no anchors at all).
    emit_trailing_gap(nodes, &cursors, &mut out, alloc);
    out
}

/// The ordered list of child fingerprints present (in order) in EVERY
/// member — the alignment anchors. Computed by folding the first
/// member's child-fp sequence against each other member via a longest-
/// common-subsequence intersection, so the result is a subsequence of
/// every member's child order (deterministic, position-stable).
fn common_anchor_fps(nodes: &[&NormalizedTree]) -> Vec<u64> {
    let mut anchors: Vec<u64> = nodes[0].children.iter().map(|c| c.fp).collect();
    for node in &nodes[1..] {
        let other: Vec<u64> = node.children.iter().map(|c| c.fp).collect();
        anchors = lcs_fp_sequence(&anchors, &other);
        if anchors.is_empty() {
            break;
        }
    }
    anchors
}

/// Longest common subsequence of two `fp` sequences (classic DP).
/// Tie-break is the standard "prefer the upper-left predecessor"
/// reconstruction, which is deterministic and depends only on the two
/// sequences — never on member input order.
fn lcs_fp_sequence(a: &[u64], b: &[u64]) -> Vec<u64> {
    let (n, m) = (a.len(), b.len());
    // dp[i][j] = LCS length of a[i..] and b[j..].
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    reconstruct_lcs(a, b, &dp)
}

/// Walk the LCS DP table to recover the subsequence. On a mismatch
/// prefer advancing in `a` when `dp[i+1][j] >= dp[i][j+1]` — a fixed,
/// deterministic tie-break.
fn reconstruct_lcs(a: &[u64], b: &[u64], dp: &[Vec<u32>]) -> Vec<u64> {
    let (mut i, mut j) = (0usize, 0usize);
    let mut out = Vec::new();
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            out.push(a[i]);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Emit a grouped hole for the children each member holds BEFORE the
/// next `anchor_fp`, advancing each member's cursor to (but not past)
/// the anchor. Only emits a hole if at least one member has a non-empty
/// gap; a clean alignment (every member already at the anchor) emits
/// nothing.
fn emit_gap_before_anchor(
    nodes: &[&NormalizedTree],
    cursors: &mut [usize],
    anchor_fp: u64,
    out: &mut Vec<TemplateNode>,
    alloc: &mut HoleAllocator,
) {
    let per_member: Vec<Vec<SubElement>> = nodes
        .iter()
        .enumerate()
        .map(|(k, node)| collect_gap_until(node, &mut cursors[k], anchor_fp))
        .collect();
    if per_member.iter().any(|g| !g.is_empty()) {
        out.push(make_grouped_hole(per_member, alloc));
    }
}

/// Collect (as [`SubElement`]s) one member's children from `*cursor`
/// up to — but not including — the first child whose `fp == anchor_fp`,
/// advancing `*cursor` to that anchor child.
fn collect_gap_until(node: &NormalizedTree, cursor: &mut usize, anchor_fp: u64) -> Vec<SubElement> {
    let mut gap = Vec::new();
    while *cursor < node.children.len() && node.children[*cursor].fp != anchor_fp {
        gap.push(sub_element_of(&node.children[*cursor]));
        *cursor += 1;
    }
    gap
}

/// Emit the aligned anchor child shared by every member, recursing into
/// it, and advance every member's cursor past its anchor child.
fn emit_anchor(
    nodes: &[&NormalizedTree],
    cursors: &mut [usize],
    out: &mut Vec<TemplateNode>,
    alloc: &mut HoleAllocator,
) {
    let anchor_nodes: Vec<&NormalizedTree> = nodes
        .iter()
        .enumerate()
        .map(|(k, node)| {
            let child = &node.children[cursors[k]];
            cursors[k] += 1;
            child
        })
        .collect();
    out.push(align_node(&anchor_nodes, alloc));
}

/// Emit a grouped hole for the children every member holds AFTER its
/// last consumed anchor (the trailing gap). Emits nothing if every
/// member is already exhausted.
fn emit_trailing_gap(
    nodes: &[&NormalizedTree],
    cursors: &[usize],
    out: &mut Vec<TemplateNode>,
    alloc: &mut HoleAllocator,
) {
    let per_member: Vec<Vec<SubElement>> = nodes
        .iter()
        .enumerate()
        .map(|(k, node)| {
            node.children[cursors[k]..]
                .iter()
                .map(sub_element_of)
                .collect()
        })
        .collect();
    if per_member.iter().any(|g| !g.is_empty()) {
        out.push(make_grouped_hole(per_member, alloc));
    }
}

/// Compute a hole's [`Divergence`] from its per-member substitutions.
///
/// The binding fingerprint of a member is the XOR-fold of its element
/// fingerprints (so a multi-element variadic binding collapses to a
/// single order-independent key, and a zero-element Optional binding
/// keys on [`ABSENT_FP`]). The **modal** fingerprint (highest count;
/// ties broken by ascending `u64`) is the baseline; `differing` counts
/// the members whose binding fp differs from it.
fn build_divergence(substitutions: &[Substitution]) -> Divergence {
    let members = u32::try_from(substitutions.len()).unwrap_or(u32::MAX);
    let keyed: Vec<(u64, String)> = substitutions.iter().map(binding_key).collect();
    let tally = tally_bindings(&keyed);
    let distinct = u32::try_from(tally.len()).unwrap_or(u32::MAX);
    let modal_fp = modal_fingerprint(&tally);
    let differing = keyed.iter().filter(|(fp, _)| *fp != modal_fp).count();
    let distinct_values = distinct_values_sorted(&tally);
    Divergence::new(
        distinct,
        u32::try_from(differing).unwrap_or(u32::MAX),
        members,
        distinct_values,
    )
}

/// The (fingerprint, display-lexeme) key for one member's binding: the
/// XOR-fold of its element fps with a representative lexeme. A
/// zero-element (absent) binding keys on [`ABSENT_FP`] /
/// [`ABSENT_LEXEME`].
fn binding_key(sub: &Substitution) -> (u64, String) {
    if sub.elements.is_empty() {
        return (ABSENT_FP, ABSENT_LEXEME.to_string());
    }
    let fp = sub.elements.iter().fold(0u64, |acc, e| acc ^ e.fp);
    let lexeme = joined_lexeme(&sub.elements);
    (fp, lexeme)
}

/// Join a binding's element lexemes for display (single element →
/// itself; multiple → space-joined, mirroring a repetition group).
fn joined_lexeme(elements: &[SubElement]) -> String {
    if elements.len() == 1 {
        elements[0].lexeme.clone()
    } else {
        elements
            .iter()
            .map(|e| e.lexeme.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Count binding occurrences keyed by fingerprint, retaining the
/// lexicographically SMALLEST lexeme per fingerprint as the
/// representative. `BTreeMap` keeps the tally deterministic and ordered
/// by `fp` for the downstream modal / sort steps.
///
/// The representative lexeme is the min (not first-seen) so it does not
/// depend on member input order — two members sharing one `fp` but
/// carrying different lexemes (a pure rename) must yield the SAME
/// representative regardless of which member appears first (property
/// P2: permutation stability).
fn tally_bindings(keyed: &[(u64, String)]) -> BTreeMap<u64, (String, u32)> {
    let mut tally: BTreeMap<u64, (String, u32)> = BTreeMap::new();
    for (fp, lexeme) in keyed {
        let entry = tally.entry(*fp).or_insert_with(|| (lexeme.clone(), 0));
        entry.1 += 1;
        if lexeme < &entry.0 {
            entry.0.clone_from(lexeme);
        }
    }
    tally
}

/// The modal binding fingerprint: the highest-count entry, ties broken
/// by ascending `u64` (the `BTreeMap` iteration order makes the first
/// max deterministic).
fn modal_fingerprint(tally: &BTreeMap<u64, (String, u32)>) -> u64 {
    tally
        .iter()
        .max_by(|(fp_a, (_, ca)), (fp_b, (_, cb))| {
            // Higher count wins; on a tie the SMALLER fp wins, so invert
            // the fp comparison (we take the `max`).
            ca.cmp(cb).then_with(|| fp_b.cmp(fp_a))
        })
        .map_or(ABSENT_FP, |(fp, _)| *fp)
}

/// Build the sorted `distinct_values` list: descending `count`, then
/// ascending `fp`. Matches the worked-example ordering (absent key
/// `fp = 0` with the higher count sorts first).
fn distinct_values_sorted(tally: &BTreeMap<u64, (String, u32)>) -> Vec<DistinctValue> {
    let mut values: Vec<DistinctValue> = tally
        .iter()
        .map(|(fp, (lexeme, count))| DistinctValue::new(*fp, lexeme.clone(), *count))
        .collect();
    values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.fp.cmp(&b.fp)));
    values
}

// ---- in-engine reconstruction (property P1) ----

/// Reconstruct member `k`'s full [`NormalizedTree`] from a [`Template`]
/// and the original `members` slice the LGG consumed (property P1).
///
/// The reconstruction walks the template's [`TemplateNode::Fixed`]
/// skeleton, and at each [`TemplateNode::Hole`] splices in member `k`'s
/// actual bound subtree(s) — located in the original member tree by the
/// substitution element's `(fp, span)` (the durable subtree identity).
/// A fixed node's `fp` / `span` (dropped by the lossy wire) are
/// recovered from the same member tree by addressing the position the
/// reconstruction is building. For a faithful LGG this reproduces
/// `members[k]` exactly — `fp`, `span`, `leaf`, children, and all — so
/// `instantiate(t, members, k) == members[k]` for every `k`.
///
/// Checkable IN-ENGINE only (it needs the original trees); the wire is
/// lossy by design and claims no consumer reconstruction.
///
/// # Panics
///
/// Panics if `k` is out of range — an LGG-internal invariant, not user
/// input.
#[must_use]
pub fn instantiate(template: &Template, members: &[NormalizedTree], k: usize) -> NormalizedTree {
    rebuild_against_member(&template.root, &template.holes, k, &members[k])
}

/// Reconstruct the single subtree a [`TemplateNode::Fixed`] yields,
/// addressing the aligned `member` node for the identity the lossy
/// template drops and recursing children with hole-run alignment.
///
/// `member` is the original member subtree this template node aligns
/// with; its `label` / `fp` / `leaf` / `span` are copied verbatim and
/// its children feed the child reconstruction.
fn rebuild_against_member(
    node: &TemplateNode,
    holes: &[Hole],
    k: usize,
    member: &NormalizedTree,
) -> NormalizedTree {
    match node {
        TemplateNode::Fixed { children, .. } => {
            let kids = rebuild_children(children, holes, k, member);
            NormalizedTree {
                label: member.label.clone(),
                fp: member.fp,
                children: kids,
                leaf: member.leaf.clone(),
                span: member.span,
            }
        }
        // A hole at the root degenerates to the member itself (the LGG
        // never roots a single tree at a hole, but be total).
        TemplateNode::Hole(_) => member.clone(),
    }
}

/// Reconstruct a fixed node's children by walking the template children
/// against `member`'s children: a fixed template child consumes one
/// member child (recursed); a hole consumes its per-member element run
/// (spliced verbatim from `member`'s children at the cursor).
fn rebuild_children(
    tmpl_children: &[TemplateNode],
    holes: &[Hole],
    k: usize,
    member: &NormalizedTree,
) -> Vec<NormalizedTree> {
    let mut kids: Vec<NormalizedTree> = Vec::new();
    let mut cursor = 0usize;
    for tc in tmpl_children {
        match tc {
            TemplateNode::Fixed { .. } => {
                let mc = &member.children[cursor];
                kids.push(rebuild_against_member(tc, holes, k, mc));
                cursor += 1;
            }
            TemplateNode::Hole(id) => {
                let count = holes[id.index as usize].substitutions[k].elements.len();
                for offset in 0..count {
                    kids.push(member.children[cursor + offset].clone());
                }
                cursor += count;
            }
        }
    }
    kids
}

// ---- shared-shape predicates over the candidate node slice ----

/// Every node is structurally AND lexically identical to the first,
/// ignoring `span` (members at different source positions share
/// structure but never spans).
///
/// This — not bare `fp` equality — is the fixed-position short-circuit:
/// a pure rename collapses to the same structural `fp` but differs in
/// lexeme, and must NOT be fixed (it is the rename hole).
fn all_structurally_identical(nodes: &[&NormalizedTree]) -> bool {
    let first = nodes[0];
    nodes[1..]
        .iter()
        .all(|n| same_shape_ignoring_span(first, n))
}

/// Structural + lexical equality of two trees ignoring `span`: same
/// `fp`, label, leaf lexeme, arity, and recursively-identical children.
fn same_shape_ignoring_span(a: &NormalizedTree, b: &NormalizedTree) -> bool {
    a.fp == b.fp
        && a.label == b.label
        && leaf_lexeme(a) == leaf_lexeme(b)
        && a.children.len() == b.children.len()
        && a.children
            .iter()
            .zip(&b.children)
            .all(|(x, y)| same_shape_ignoring_span(x, y))
}

/// The leaf lexeme of a node, or `None` for an internal node.
fn leaf_lexeme(node: &NormalizedTree) -> Option<&str> {
    node.leaf.as_ref().map(|l| l.lexeme.as_str())
}

/// Every node shares the first node's label.
fn all_same_label(nodes: &[&NormalizedTree]) -> bool {
    let first = &nodes[0].label;
    nodes.iter().all(|n| &n.label == first)
}

/// Every node is internal (not a leaf).
fn all_internal(nodes: &[&NormalizedTree]) -> bool {
    nodes.iter().all(|n| !n.is_leaf())
}

/// Every node has the first node's child count.
fn all_same_arity(nodes: &[&NormalizedTree]) -> bool {
    let arity = nodes[0].children.len();
    nodes.iter().all(|n| n.children.len() == arity)
}

#[cfg(test)]
mod tests;
