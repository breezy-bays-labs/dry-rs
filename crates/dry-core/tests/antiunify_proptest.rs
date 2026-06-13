//! Property tests for the first-order anti-unification (LGG) engine
//! (`dry_core::comparison::antiunify`, epic #107 PR 5).
//!
//! Covers the four acceptance-criterion property families:
//!
//! - **P1 — in-engine reconstruction.** For every member `k`,
//!   `instantiate(template, members, k) == members[k]`, exercised across
//!   all four hole kinds (SubExpr, Block, Optional, Variadic). The
//!   template + per-member substitutions losslessly capture each member.
//! - **P2 — determinism + permutation stability.** The emitted
//!   `Template` is byte-identical across repeated runs AND across any
//!   permutation of the `members` slice (the LCS tie-break is intrinsic
//!   to the trees, never the input order).
//! - **P4 — divergence relationships.** `differing <= distinct <=
//!   members`; the d-slider set `{ holes : differing >= d }` is
//!   monotone non-increasing in `d` (P4d); and `distinct == members`
//!   iff every member binds a distinct value (the d = n endpoint).
//! - **P6 — totality / no-panic.** The engine never panics on a
//!   singleton, an empty slice, identical members, or mismatched arity.
//!
//! The regression seeds live in
//! `crates/dry-core/proptest-regressions/` (committed, never
//! gitignored — AGENTS.md working rules).

use dry_core::comparison::{Hole, Template, antiunify, instantiate};
use dry_core::domain::{LeafClass, LeafToken, LineColumn, NormalizedTree, Span};
use proptest::prelude::*;

/// Build a span on `line`, columns `c..=c` (single-position; the engine
/// ignores spans for structure but needs them well-formed).
fn sp(line: u32, c: u32) -> Span {
    Span::try_new(LineColumn::new(line, c), LineColumn::new(line, c)).expect("valid span")
}

/// A leaf node from a label index, fp, and lexeme.
fn leaf(label: u8, fp: u64, lexeme: &str, line: u32) -> NormalizedTree {
    NormalizedTree::leaf(
        format!("L{label}"),
        fp,
        LeafToken::new(LeafClass::Ident, lexeme.to_string()),
        sp(line, 0),
    )
}

/// An internal node.
fn node(label: u8, fp: u64, children: Vec<NormalizedTree>, line: u32) -> NormalizedTree {
    NormalizedTree::new(format!("N{label}"), fp, children, sp(line, 0))
}

// ---- tree generation ----

/// A small arbitrary `NormalizedTree`. Bounded depth/breadth keeps the
/// proptest fast; `span` varies by a `line` counter so distinct nodes
/// have distinct spans (needed by the hole-binding `(fp, span)` lookup
/// in `instantiate`).
fn arb_tree() -> impl Strategy<Value = NormalizedTree> {
    let leaf = (0u8..3, 0u64..8, "[a-z]{1,3}", 1u32..50)
        .prop_map(|(lbl, fp, lex, line)| leaf(lbl, fp, &lex, line));
    leaf.prop_recursive(3, 16, 3, |inner| {
        (
            0u8..3,
            0u64..8,
            prop::collection::vec(inner, 0..3),
            1u32..50,
        )
            .prop_map(|(lbl, fp, kids, line)| node(lbl, fp, kids, line))
    })
}

/// A cluster of 1..5 arbitrary member trees.
fn arb_members() -> impl Strategy<Value = Vec<NormalizedTree>> {
    prop::collection::vec(arb_tree(), 1..5)
}

/// Re-span a tree's every node onto a fresh, member-unique line base so
/// two members built from the same shape get DISTINCT spans (so the
/// `(fp, span)` hole-binding lookup in `instantiate` is unambiguous).
fn respan(tree: &NormalizedTree, base: u32, counter: &mut u32) -> NormalizedTree {
    let line = base + *counter;
    *counter += 1;
    let children: Vec<NormalizedTree> = tree
        .children
        .iter()
        .map(|c| respan(c, base, counter))
        .collect();
    match &tree.leaf {
        Some(tok) => NormalizedTree::leaf(tree.label.clone(), tree.fp, tok.clone(), sp(line, 0)),
        None => NormalizedTree::new(tree.label.clone(), tree.fp, children, sp(line, 0)),
    }
}

/// Build a member family that deterministically exercises all four hole
/// kinds: a shared `Block` whose children are
///   [ optional-guard?, shared-stmt(with-rename), variadic-tail* ].
///
/// - The guard is present only in members where `guard_present[k]` —
///   an Optional hole.
/// - The shared stmt contains a leaf with a SHARED structural fp but a
///   per-member lexeme — a SubExpr (rename) hole.
/// - The variadic tail holds `tail_counts[k]` repeated stmts — a
///   Variadic hole.
fn structured_family(guard_present: &[bool], tail_counts: &[usize]) -> Vec<NormalizedTree> {
    let n = guard_present.len();
    let mut members = Vec::with_capacity(n);
    for k in 0..n {
        let base = (k as u32 + 1) * 100;
        let mut children = Vec::new();
        if guard_present[k] {
            // Optional guard: shared fp 7000 so it anchors when present.
            children.push(node(8, 7000, vec![leaf(9, 91, "g", base + 1)], base + 2));
        }
        // Shared stmt with an embedded rename (shared structural fp 111,
        // per-member lexeme) — the SubExpr hole.
        let rename = leaf(7, 111, &format!("v{k}"), base + 10);
        let stmt = node(6, 800, vec![rename], base + 11);
        children.push(stmt);
        // Variadic tail: `tail_counts[k]` repeated stmts that SHARE one
        // structural fp (600) so they form a single contiguous gap run
        // relative to a member with fewer of them — a Variadic hole, not
        // a fragmented per-position split.
        for t in 0..tail_counts[k] {
            children.push(node(5, 600, vec![], base + 20 + t as u32));
        }
        members.push(node(1, 900, children, base));
    }
    members
}

// ---- P-helpers ----

/// All `(differing, distinct, members)` triples across a template's
/// holes.
fn divergence_triples(t: &Template) -> Vec<(u32, u32, u32)> {
    t.holes
        .iter()
        .map(|h| {
            (
                h.divergence.differing,
                h.divergence.distinct,
                h.divergence.members,
            )
        })
        .collect()
}

/// Count of holes whose `differing >= d` — the d-slider's revealed set
/// size at level `d`.
fn revealed_at(holes: &[Hole], d: u32) -> usize {
    holes.iter().filter(|h| h.divergence.differing >= d).count()
}

proptest! {
    // ---- P6: totality / no-panic ----

    /// The engine never panics for any cluster of 1..5 arbitrary trees.
    #[test]
    fn p6_never_panics(members in arb_members()) {
        let _ = antiunify(&members);
    }

    /// A singleton is the LGG of itself: no holes, reconstructs exactly.
    #[test]
    fn p6_singleton_no_holes(tree in arb_tree()) {
        let members = vec![tree];
        let t = antiunify(&members);
        prop_assert!(t.holes.is_empty(), "singleton must have no holes");
        prop_assert_eq!(instantiate(&t, &members, 0), members[0].clone());
    }

    /// Identical members produce a hole-free template.
    #[test]
    fn p6_identical_members_no_holes(tree in arb_tree(), n in 2usize..5) {
        // Re-span each copy so they are distinct in source position but
        // structurally + lexically identical.
        let members: Vec<NormalizedTree> = (0..n)
            .map(|k| {
                let mut counter = 0;
                respan(&tree, (k as u32 + 1) * 1000, &mut counter)
            })
            .collect();
        let t = antiunify(&members);
        prop_assert!(
            t.holes.is_empty(),
            "structurally identical members must yield no holes: {:#?}",
            t
        );
    }

    // ---- P1: in-engine reconstruction (arbitrary members) ----

    /// For every member, `instantiate(template, members, k) ==
    /// members[k]` — the template + substitutions losslessly capture
    /// each member.
    #[test]
    fn p1_reconstructs_each_member(members in arb_members()) {
        let t = antiunify(&members);
        for (k, m) in members.iter().enumerate() {
            prop_assert_eq!(
                &instantiate(&t, &members, k),
                m,
                "member {} failed to reconstruct from template {:#?}",
                k,
                t
            );
        }
    }

    // ---- P1: reconstruction across ALL FOUR hole kinds ----

    /// The structured family exercises Optional + SubExpr + Variadic
    /// (and Block via the shared internal nodes) simultaneously, and
    /// every member still reconstructs exactly.
    #[test]
    fn p1_reconstructs_all_hole_kinds(
        guard in prop::collection::vec(any::<bool>(), 2..5),
        tails in prop::collection::vec(0usize..3, 2..5),
    ) {
        // Equal-length guard/tail vectors (zip to the shorter).
        let n = guard.len().min(tails.len());
        let members = structured_family(&guard[..n], &tails[..n]);
        let t = antiunify(&members);
        for (k, m) in members.iter().enumerate() {
            prop_assert_eq!(
                &instantiate(&t, &members, k),
                m,
                "structured member {} failed to reconstruct: {:#?}",
                k,
                t
            );
        }
    }

    // ---- P2: determinism + permutation stability ----

    /// Repeated runs over the same members produce a byte-identical
    /// template.
    #[test]
    fn p2_deterministic_across_runs(members in arb_members()) {
        let a = serde_json::to_string(&antiunify(&members)).unwrap();
        let b = serde_json::to_string(&antiunify(&members)).unwrap();
        prop_assert_eq!(a, b);
    }

    /// Permuting the member input order leaves the template structurally
    /// invariant. The hole `substitutions` are an index join with the
    /// members, so they DO permute with the members; the template's
    /// ROOT skeleton + hole kinds + divergence multiset are what stay
    /// invariant. We assert the root skeleton + per-hole kind/divergence
    /// are identical under a reversal permutation.
    #[test]
    fn p2_permutation_stable_skeleton(members in arb_members()) {
        let forward = antiunify(&members);
        let mut reversed = members.clone();
        reversed.reverse();
        let back = antiunify(&reversed);

        // Root skeleton (the TemplateNode tree, which carries no
        // member-order data) must be byte-identical.
        prop_assert_eq!(
            serde_json::to_string(&forward.root).unwrap(),
            serde_json::to_string(&back.root).unwrap(),
            "root skeleton must be permutation-invariant"
        );
        // Hole count + per-hole kind must match.
        prop_assert_eq!(forward.holes.len(), back.holes.len());
        for (hf, hb) in forward.holes.iter().zip(&back.holes) {
            prop_assert_eq!(hf.kind, hb.kind, "hole kind must be permutation-invariant");
            // The divergence summary is order-independent (counts +
            // sorted distinct_values), so it must match exactly.
            prop_assert_eq!(
                &hf.divergence,
                &hb.divergence,
                "divergence must be permutation-invariant"
            );
        }
    }

    // ---- P4: divergence relationships ----

    /// `differing <= distinct <= members` for every hole.
    #[test]
    fn p4_differing_le_distinct_le_members(members in arb_members()) {
        let t = antiunify(&members);
        for (differing, distinct, n) in divergence_triples(&t) {
            prop_assert!(differing <= distinct, "differing {} > distinct {}", differing, distinct);
            prop_assert!(distinct <= n, "distinct {} > members {}", distinct, n);
        }
    }

    /// P4d — the revealed-hole set `{ h : differing >= d }` is monotone
    /// non-increasing as `d` rises.
    #[test]
    fn p4d_revealed_set_monotone_in_d(members in arb_members()) {
        let t = antiunify(&members);
        let max_d = members.len() as u32;
        for d in 0..max_d {
            let here = revealed_at(&t.holes, d);
            let next = revealed_at(&t.holes, d + 1);
            prop_assert!(
                next <= here,
                "revealed set grew from d={} ({}) to d={} ({})",
                d, here, d + 1, next
            );
        }
    }

    /// `distinct == members` exactly when every member binds a distinct
    /// value (the d = n all-differ endpoint).
    #[test]
    fn p4_distinct_eq_members_iff_all_differ(members in arb_members()) {
        let t = antiunify(&members);
        for h in &t.holes {
            let n = h.divergence.members;
            let all_distinct_values_singleton =
                h.divergence.distinct_values.iter().all(|v| v.count == 1);
            let all_differ = h.divergence.distinct == n && all_distinct_values_singleton;
            // distinct == members <=> every distinct value has count 1.
            prop_assert_eq!(
                h.divergence.distinct == n,
                all_differ,
                "distinct==members must coincide with all-singleton tallies: {:?}",
                h.divergence
            );
        }
    }

    // ---- determinism of the hole index join ----

    /// Each hole's `substitutions` length equals the member count (the
    /// index-join contract).
    #[test]
    fn substitutions_index_join_with_members(members in arb_members()) {
        let t = antiunify(&members);
        for h in &t.holes {
            prop_assert_eq!(
                h.substitutions.len(),
                members.len(),
                "hole {} substitutions must be 1:1 with members",
                h.id.index
            );
            prop_assert_eq!(h.divergence.members as usize, members.len());
        }
    }
}

// ---- non-proptest pin: the four hole kinds actually appear ----

/// Sanity: the structured family produces Optional + SubExpr + Variadic
/// (the gap / repetition / rename kinds), so the P1-all-kinds proptest
/// is not vacuous. (The fourth kind, Block, is a scalar-hole
/// classification covered by the engine unit tests — a block-labelled
/// scalar divergence; it does not arise from this gap-based family.)
#[test]
fn structured_family_covers_optional_subexpr_variadic() {
    use dry_core::comparison::HoleKind;
    // member 0: no guard, 0 tail; member 1: guard, 2 tail; member 2: no
    // guard, 1 tail. Optional (guard differs), Variadic (tail counts
    // differ), SubExpr (the rename).
    let members = structured_family(&[false, true, false], &[0, 2, 1]);
    let t = antiunify(&members);
    let kinds: Vec<HoleKind> = t.holes.iter().map(|h| h.kind).collect();
    assert!(
        kinds.contains(&HoleKind::Optional),
        "missing Optional: {kinds:?}"
    );
    assert!(
        kinds.contains(&HoleKind::Variadic),
        "missing Variadic: {kinds:?}"
    );
    assert!(
        kinds.contains(&HoleKind::SubExpr),
        "missing SubExpr: {kinds:?}"
    );
    // Every member still reconstructs (P1 across these kinds).
    for (k, m) in members.iter().enumerate() {
        assert_eq!(
            &instantiate(&t, &members, k),
            m,
            "member {k} reconstruction"
        );
    }
}

/// A scalar Block hole: two members whose aligned position holds a
/// block-labelled subtree with DIFFERING block labels (so the position
/// generalizes to a single hole, not a recursed shared-label node)
/// yields a `Block` hole — every bound node is block-like — and both
/// members reconstruct.
#[test]
fn block_labelled_scalar_divergence_is_block_hole() {
    use dry_core::comparison::HoleKind;
    // Root "N1" with one child each: differently-labelled blocks
    // ("Block" vs "StmtBlock") — both block-like, so the scalar hole is
    // classified Block, not SubExpr.
    let a = node(
        1,
        10,
        vec![NormalizedTree::new(
            "Block".to_string(),
            300,
            vec![leaf(4, 41, "p", 5)],
            sp(6, 0),
        )],
        1,
    );
    let b = node(
        1,
        20,
        vec![NormalizedTree::new(
            "StmtBlock".to_string(),
            400,
            vec![leaf(4, 42, "q", 15)],
            sp(16, 0),
        )],
        11,
    );
    let members = vec![a, b];
    let t = antiunify(&members);
    let kinds: Vec<HoleKind> = t.holes.iter().map(|h| h.kind).collect();
    assert!(
        kinds.contains(&HoleKind::Block),
        "expected a Block hole: {kinds:?}"
    );
    for (k, m) in members.iter().enumerate() {
        assert_eq!(
            &instantiate(&t, &members, k),
            m,
            "member {k} reconstruction"
        );
    }
}
