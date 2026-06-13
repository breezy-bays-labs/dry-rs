//! Unit tests for the first-order anti-unification (LGG) engine.
//!
//! The property-level invariants (P1 reconstruction, P2 determinism,
//! P4 divergence, P6 totality) live in
//! `crates/dry-core/tests/antiunify_proptest.rs`; these unit tests pin
//! the per-helper behavior + the build-plan worked 3-member example as
//! a known-good fixture.

use super::*;
use crate::domain::{LeafClass, LeafToken, LineColumn, NormalizedTree, Span};

/// A span helper — line `l`, columns `c0..=c1` on that line.
fn sp(l: u32, c0: u32, c1: u32) -> Span {
    Span::try_new(LineColumn::new(l, c0), LineColumn::new(l, c1)).expect("valid span")
}

/// A leaf node `Ident`/`Literal`/… carrying a lexeme + fp.
fn leaf(label: &str, fp: u64, lexeme: &str, span: Span) -> NormalizedTree {
    NormalizedTree::leaf(
        label.to_string(),
        fp,
        LeafToken::new(LeafClass::Ident, lexeme.to_string()),
        span,
    )
}

/// An internal node.
fn node(label: &str, fp: u64, children: Vec<NormalizedTree>, span: Span) -> NormalizedTree {
    NormalizedTree::new(label.to_string(), fp, children, span)
}

// ---- entry-point / totality (P6) ----

#[test]
fn empty_members_yields_empty_leaf_no_holes() {
    let t = antiunify(&[]);
    assert!(t.holes.is_empty());
    match t.root {
        TemplateNode::Fixed {
            label, children, ..
        } => {
            assert_eq!(label, "");
            assert!(children.is_empty());
        }
        TemplateNode::Hole(_) => panic!("empty input must not root a hole"),
    }
}

#[test]
fn singleton_yields_own_structure_no_holes() {
    let m = node(
        "Block",
        10,
        vec![leaf("Ident", 1, "x", sp(1, 0, 0))],
        sp(1, 0, 5),
    );
    let t = antiunify(std::slice::from_ref(&m));
    assert!(t.holes.is_empty(), "a tree is the LGG of itself: no holes");
    // The singleton template instantiates back to the member exactly.
    assert_eq!(instantiate(&t, std::slice::from_ref(&m), 0), m);
}

#[test]
fn identical_members_yield_hole_free_template() {
    let m = node(
        "Block",
        10,
        vec![leaf("Ident", 1, "x", sp(1, 0, 0))],
        sp(1, 0, 5),
    );
    let members = vec![m.clone(), m.clone(), m.clone()];
    let t = antiunify(&members);
    assert!(
        t.holes.is_empty(),
        "structurally identical members produce no holes"
    );
}

#[test]
fn mismatched_arity_does_not_panic() {
    // Same label, different child counts — exercises the LCS path.
    let a = node(
        "Call",
        1,
        vec![leaf("Ident", 10, "f", sp(1, 0, 0))],
        sp(1, 0, 3),
    );
    let b = node(
        "Call",
        2,
        vec![
            leaf("Ident", 10, "f", sp(2, 0, 0)),
            leaf("Ident", 20, "g", sp(2, 2, 2)),
        ],
        sp(2, 0, 5),
    );
    let t = antiunify(&[a, b]);
    // Common anchor: the shared `f` child (fp 10) aligns; `g` is a gap.
    assert!(!t.holes.is_empty(), "differing arity must produce a hole");
}

// ---- align_node decision ladder ----

#[test]
fn divergent_leaf_becomes_subexpr_hole() {
    // Same label, different lexeme + fp -> a rename -> a SubExpr hole.
    let a = leaf("Ident", 1, "x", sp(1, 0, 0));
    let b = leaf("Ident", 2, "y", sp(2, 0, 0));
    let t = antiunify(&[a, b]);
    assert_eq!(t.holes.len(), 1);
    assert_eq!(t.holes[0].kind, HoleKind::SubExpr);
    match t.root {
        TemplateNode::Hole(id) => assert_eq!(id.index, 0),
        TemplateNode::Fixed { .. } => panic!("divergent leaf must be a hole"),
    }
}

#[test]
fn differing_labels_become_hole() {
    let a = leaf("Ident", 1, "x", sp(1, 0, 0));
    let b = leaf("Literal", 2, "42", sp(2, 0, 1));
    let t = antiunify(&[a, b]);
    assert_eq!(t.holes.len(), 1);
}

#[test]
fn shared_internal_equal_arity_recurses() {
    // ExprBinary(+) over (x, <local>) where the right operand diverges.
    let a = node(
        "ExprBinary",
        100,
        vec![
            leaf("Ident", 1, "x", sp(1, 0, 0)),
            leaf("Ident", 11, "y", sp(1, 4, 4)),
        ],
        sp(1, 0, 4),
    );
    let b = node(
        "ExprBinary",
        200,
        vec![
            leaf("Ident", 1, "x", sp(2, 0, 0)),
            leaf("Ident", 22, "z", sp(2, 4, 4)),
        ],
        sp(2, 0, 4),
    );
    let t = antiunify(&[a, b]);
    // Left operand `x` (fp 1) is shared and fixed; right operand is a hole.
    match &t.root {
        TemplateNode::Fixed {
            label, children, ..
        } => {
            assert_eq!(label, "ExprBinary");
            assert_eq!(children.len(), 2);
            assert!(matches!(children[0], TemplateNode::Fixed { .. }));
            assert!(matches!(children[1], TemplateNode::Hole(_)));
        }
        TemplateNode::Hole(_) => panic!("shared label/arity must stay fixed"),
    }
    assert_eq!(t.holes.len(), 1);
    assert_eq!(t.holes[0].kind, HoleKind::SubExpr);
}

// ---- classify_hole_kind ----

#[test]
fn classify_optional_when_a_member_binds_zero() {
    let per_member = vec![vec![SubElement::new("a".into(), 1, sp(1, 0, 0))], vec![]];
    assert_eq!(classify_hole_kind(&per_member), HoleKind::Optional);
}

#[test]
fn classify_variadic_when_a_member_binds_many() {
    let per_member = vec![
        vec![SubElement::new("a".into(), 1, sp(1, 0, 0))],
        vec![
            SubElement::new("a".into(), 1, sp(2, 0, 0)),
            SubElement::new("b".into(), 2, sp(2, 2, 2)),
        ],
    ];
    assert_eq!(classify_hole_kind(&per_member), HoleKind::Variadic);
}

#[test]
fn classify_subexpr_when_all_bind_one() {
    let per_member = vec![
        vec![SubElement::new("a".into(), 1, sp(1, 0, 0))],
        vec![SubElement::new("b".into(), 2, sp(2, 0, 0))],
    ];
    assert_eq!(classify_hole_kind(&per_member), HoleKind::SubExpr);
}

// ---- build_divergence (P4-relations at unit scope) ----

#[test]
fn divergence_modal_baseline_counts_differing() {
    // Three members: two bind fp 111 ("y"), one binds fp 999 ("z").
    let subs = vec![
        Substitution::new(vec![SubElement::new("y".into(), 111, sp(1, 0, 0))]),
        Substitution::new(vec![SubElement::new("y".into(), 111, sp(2, 0, 0))]),
        Substitution::new(vec![SubElement::new("z".into(), 999, sp(3, 0, 0))]),
    ];
    let d = build_divergence(&subs);
    assert_eq!(d.members, 3);
    assert_eq!(d.distinct, 2);
    assert_eq!(d.differing, 1, "one member differs from the modal fp 111");
    // distinct_values: descending count then ascending fp.
    assert_eq!(d.distinct_values[0].fp, 111);
    assert_eq!(d.distinct_values[0].count, 2);
    assert_eq!(d.distinct_values[1].fp, 999);
    assert_eq!(d.distinct_values[1].count, 1);
}

#[test]
fn divergence_all_same_is_zero_differing() {
    let subs = vec![
        Substitution::new(vec![SubElement::new("y".into(), 111, sp(1, 0, 0))]),
        Substitution::new(vec![SubElement::new("y".into(), 111, sp(2, 0, 0))]),
        Substitution::new(vec![SubElement::new("y".into(), 111, sp(3, 0, 0))]),
    ];
    let d = build_divergence(&subs);
    assert_eq!(d.distinct, 1);
    assert_eq!(d.differing, 0);
}

#[test]
fn divergence_all_differ_distinct_equals_members() {
    // d = n endpoint: distinct == members.
    let subs = vec![
        Substitution::new(vec![SubElement::new("a".into(), 1, sp(1, 0, 0))]),
        Substitution::new(vec![SubElement::new("b".into(), 2, sp(2, 0, 0))]),
        Substitution::new(vec![SubElement::new("c".into(), 3, sp(3, 0, 0))]),
    ];
    let d = build_divergence(&subs);
    assert_eq!(d.distinct, d.members);
    // The modal is the smallest fp (1) on the all-tied counts; the
    // other two members differ from it.
    assert_eq!(d.differing, 2);
}

#[test]
fn divergence_absent_keys_on_zero_fp() {
    // Optional: one member absent -> ABSENT_FP (0) tally entry.
    let subs = vec![
        Substitution::new(vec![]),
        Substitution::new(vec![]),
        Substitution::new(vec![SubElement::new(
            "if x == 0 { return 0; }".into(),
            7_711_222,
            sp(6, 4, 27),
        )]),
    ];
    let d = build_divergence(&subs);
    assert_eq!(d.members, 3);
    assert_eq!(d.distinct, 2);
    assert_eq!(
        d.differing, 1,
        "the one present member differs from absent modal"
    );
    assert_eq!(d.distinct_values[0].fp, 0);
    assert_eq!(d.distinct_values[0].lexeme, "<absent>");
    assert_eq!(d.distinct_values[0].count, 2);
    assert_eq!(d.distinct_values[1].fp, 7_711_222);
    assert_eq!(d.distinct_values[1].count, 1);
}

// ---- LCS variadic alignment ----

#[test]
fn lcs_aligns_common_anchors_and_holes_the_gaps() {
    // Member 0 children: [A(1), B(2), C(3)]
    // Member 1 children: [A(1), C(3)]
    // Common anchors by fp: [1, 3]; B(2) is a gap present in member 0 only.
    let a = node(
        "Block",
        50,
        vec![
            leaf("Stmt", 1, "a", sp(1, 0, 0)),
            leaf("Stmt", 2, "b", sp(2, 0, 0)),
            leaf("Stmt", 3, "c", sp(3, 0, 0)),
        ],
        sp(1, 0, 9),
    );
    let b = node(
        "Block",
        60,
        vec![
            leaf("Stmt", 1, "a", sp(10, 0, 0)),
            leaf("Stmt", 3, "c", sp(12, 0, 0)),
        ],
        sp(10, 0, 6),
    );
    let t = antiunify(&[a, b]);
    // One Optional hole for the B gap (absent in member 1).
    assert_eq!(t.holes.len(), 1);
    assert_eq!(t.holes[0].kind, HoleKind::Optional);
    assert_eq!(
        t.holes[0].substitutions[0].elements.len(),
        1,
        "member 0 has B"
    );
    assert_eq!(
        t.holes[0].substitutions[1].elements.len(),
        0,
        "member 1 lacks B"
    );
}

#[test]
fn common_anchor_fps_intersects_all_members() {
    let a = node(
        "B",
        1,
        vec![
            leaf("S", 1, "a", sp(1, 0, 0)),
            leaf("S", 2, "b", sp(2, 0, 0)),
        ],
        sp(1, 0, 3),
    );
    let b = node(
        "B",
        2,
        vec![
            leaf("S", 2, "b", sp(3, 0, 0)),
            leaf("S", 3, "c", sp(4, 0, 0)),
        ],
        sp(3, 0, 3),
    );
    let refs = vec![&a, &b];
    // Only fp 2 is common to both ordered child sequences.
    assert_eq!(common_anchor_fps(&refs), vec![2]);
}

#[test]
fn lcs_fp_sequence_classic() {
    assert_eq!(lcs_fp_sequence(&[1, 2, 3, 4], &[2, 4]), vec![2, 4]);
    assert_eq!(lcs_fp_sequence(&[1, 2, 3], &[3, 2, 1]).len(), 1);
    assert_eq!(lcs_fp_sequence(&[], &[1, 2]), Vec::<u64>::new());
}

// ---- worked 3-member example (build-plan fixture) ----

/// Build the build-plan worked example: a `Block` whose first child is
/// an optional guard (present in member 2 only) and whose second child
/// is `StmtExpr -> ExprBinary(+) -> (ExprLocal "x", <divergent local>)`.
/// The divergent local is an alpha-equivalent rename (same fp 111,
/// differing lexeme) across all three members.
fn worked_member(
    file_line: u32,
    guard: Option<NormalizedTree>,
    rhs_lexeme: &str,
) -> NormalizedTree {
    let x = node(
        "ExprLocal",
        500,
        vec![leaf("Ident", 9, "x", sp(file_line + 1, 8, 8))],
        sp(file_line + 1, 4, 4),
    );
    // The divergent local: SAME structural fp 111 across members (alpha-
    // equivalent), but a different lexeme each — the one pure rename.
    let rhs = node(
        "ExprLocal",
        111,
        vec![leaf("Ident", 9, rhs_lexeme, sp(file_line + 1, 8, 8))],
        sp(file_line + 1, 8, 8),
    );
    // The StmtExpr subtree is structurally SHARED across members (stable
    // fp): the only divergence inside it is the rename leaf, whose
    // structural fp (111) is itself shared. The Block-level guard is the
    // other divergence. Stable fps make StmtExpr the LCS anchor common
    // to all three members so the guard surfaces as the Optional gap.
    let binary = node("ExprBinary", 700, vec![x, rhs], sp(file_line + 1, 4, 9));
    let stmt = node("StmtExpr", 800, vec![binary], sp(file_line + 1, 4, 9));
    let fl = u64::from(file_line);
    let mut children = Vec::new();
    if let Some(g) = guard {
        children.push(g);
    }
    children.push(stmt);
    node("Block", 900 + fl, children, sp(file_line, 0, 1))
}

#[test]
fn worked_example_three_member_template_shape() {
    let guard = node(
        "ExprIf",
        7_711_222,
        vec![leaf("Ident", 70, "x", sp(6, 7, 7))],
        sp(6, 4, 27),
    );
    let m0 = worked_member(10, None, "y");
    let m1 = worked_member(22, None, "z");
    let m2 = worked_member(5, Some(guard), "total");
    let members = vec![m0, m1, m2];

    let t = antiunify(&members);

    // Two holes: hole 0 = optional guard, hole 1 = the rename sub-expr.
    assert_eq!(t.holes.len(), 2, "guard + rename = two holes: {t:#?}");

    // Pre-order allocation: the guard (first child) is hole 0.
    let guard_hole = &t.holes[0];
    assert_eq!(guard_hole.kind, HoleKind::Optional);
    assert_eq!(guard_hole.divergence.members, 3);
    assert_eq!(guard_hole.divergence.differing, 1);
    assert_eq!(guard_hole.divergence.distinct, 2);
    assert_eq!(guard_hole.substitutions[0].elements.len(), 0);
    assert_eq!(guard_hole.substitutions[1].elements.len(), 0);
    assert_eq!(guard_hole.substitutions[2].elements.len(), 1);

    // The rename hole: all three bind the SAME fp (111) with differing
    // lexemes -> distinct == 1, differing == 0.
    let rename_hole = &t.holes[1];
    assert_eq!(rename_hole.kind, HoleKind::SubExpr);
    assert_eq!(rename_hole.divergence.distinct, 1, "alpha-equiv: same fp");
    assert_eq!(rename_hole.divergence.differing, 0);
    assert_eq!(rename_hole.substitutions.len(), 3);
}

#[test]
fn worked_example_reconstructs_every_member() {
    // P1 at unit scope on the worked fixture (the proptest covers the
    // general case).
    let guard = node(
        "ExprIf",
        7_711_222,
        vec![leaf("Ident", 70, "x", sp(6, 7, 7))],
        sp(6, 4, 27),
    );
    let members = vec![
        worked_member(10, None, "y"),
        worked_member(22, None, "z"),
        worked_member(5, Some(guard), "total"),
    ];
    let t = antiunify(&members);
    for (k, m) in members.iter().enumerate() {
        assert_eq!(
            &instantiate(&t, &members, k),
            m,
            "member {k} must reconstruct exactly"
        );
    }
}

#[test]
fn worked_example_serializes_to_expected_wire_shape() {
    // Byte-shape pin: a hole serializes as {"node":"hole","index":N};
    // a fixed leaf carries leaf_lexeme.
    let a = leaf("Ident", 1, "x", sp(1, 0, 0));
    let b = leaf("Ident", 2, "y", sp(2, 0, 0));
    let t = antiunify(&[a, b]);
    let json = serde_json::to_string(&t.root).expect("serialize root");
    assert_eq!(json, r#"{"node":"hole","index":0}"#);

    // A fixed leaf node.
    let fixed = TemplateNode::Fixed {
        label: "ExprLocal".into(),
        children: vec![],
        leaf_lexeme: Some("x".into()),
    };
    assert_eq!(
        serde_json::to_string(&fixed).unwrap(),
        r#"{"node":"fixed","label":"ExprLocal","children":[],"leaf_lexeme":"x"}"#
    );

    // leaf_lexeme is skipped when None.
    let internal = TemplateNode::Fixed {
        label: "Block".into(),
        children: vec![],
        leaf_lexeme: None,
    };
    assert_eq!(
        serde_json::to_string(&internal).unwrap(),
        r#"{"node":"fixed","label":"Block","children":[]}"#
    );
}

#[test]
fn template_round_trips_through_json() {
    let a = node(
        "ExprBinary",
        1,
        vec![
            leaf("Ident", 9, "x", sp(1, 0, 0)),
            leaf("Ident", 11, "y", sp(1, 4, 4)),
        ],
        sp(1, 0, 4),
    );
    let b = node(
        "ExprBinary",
        2,
        vec![
            leaf("Ident", 9, "x", sp(2, 0, 0)),
            leaf("Ident", 22, "z", sp(2, 4, 4)),
        ],
        sp(2, 0, 4),
    );
    let t = antiunify(&[a, b]);
    let json = serde_json::to_string(&t).unwrap();
    let back: Template = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
}

// ---- HoleId pre-order allocation ----

#[test]
fn hole_ids_allocate_in_preorder() {
    // Two divergent positions; the left (first child) must be hole 0.
    let a = node(
        "Pair",
        1,
        vec![
            leaf("Ident", 10, "a", sp(1, 0, 0)),
            leaf("Ident", 20, "b", sp(1, 2, 2)),
        ],
        sp(1, 0, 2),
    );
    let b = node(
        "Pair",
        2,
        vec![
            leaf("Ident", 11, "c", sp(2, 0, 0)),
            leaf("Ident", 21, "d", sp(2, 2, 2)),
        ],
        sp(2, 0, 2),
    );
    let t = antiunify(&[a, b]);
    assert_eq!(t.holes.len(), 2);
    assert_eq!(t.holes[0].id.index, 0);
    assert_eq!(t.holes[1].id.index, 1);
    // The root's children reference holes 0 then 1, left-to-right.
    if let TemplateNode::Fixed { children, .. } = &t.root {
        assert!(matches!(
            children[0],
            TemplateNode::Hole(HoleId { index: 0 })
        ));
        assert!(matches!(
            children[1],
            TemplateNode::Hole(HoleId { index: 1 })
        ));
    } else {
        panic!("expected fixed root");
    }
}
