//! Tree leaf + span fidelity (dry-rs#138 real Var lexemes, dry-rs#130
//! per-node spans on `NormalizedTree`).
//!
//! These two enrichments feed the substitution-table data the HTML
//! explorer renders, and they fire the rename signal on real Rust:
//!
//! - **#138** — an alpha-renamed local/param `Var` leaf carries its REAL
//!   identifier text on the tree (`y`, `total`, …) instead of a uniform
//!   `"_"`. The FINGERPRINT stays alpha-normalized (the fold hashes the
//!   token CLASS, not the lexeme), so a same-structure-different-name
//!   cluster now surfaces pure-rename holes (`distinct == 1` with `> 1`
//!   distinct lexeme) and `Match::with_template` derives `rename_count > 0`.
//! - **#130** — every `NormalizedTree` node carries the syn node's OWN
//!   span, not the enclosing form span. The per-binding `SubElement.span`
//!   in the substitution table therefore locates each binding precisely.
//!
//! The fingerprint-byte-identity guard lives in
//! `fingerprint_determinism.rs`; these tests pin the DISPLAY + span
//! enrichments that ride alongside it.

use std::path::PathBuf;

use dry_core::comparison::antiunify;
use dry_core::domain::{Match, NormalizedTree, Tier};
use dry4rs::domain::FilePath;
use dry4rs::parser::SynNormalizer;
use dry4rs::ports::{NormalizerPort, TreeDeriverPort};

fn path() -> FilePath {
    FilePath::from(PathBuf::from("fixture.rs"))
}

/// Derive the single form's `NormalizedTree` for a one-fn source.
fn tree_of(src: &str) -> NormalizedTree {
    let n = SynNormalizer::new();
    let forms = n.normalize(src, &path()).expect("normalize");
    let span = forms[0].span;
    n.derive_tree(src, span).expect("derive_tree")
}

/// Collect every leaf lexeme in pre-order.
fn leaf_lexemes(tree: &NormalizedTree, out: &mut Vec<String>) {
    if let Some(leaf) = &tree.leaf {
        out.push(leaf.lexeme.clone());
    }
    for c in &tree.children {
        leaf_lexemes(c, out);
    }
}

#[test]
fn var_leaf_carries_real_identifier_not_underscore() {
    // #138: a function with a named param `y` referenced in the body must
    // surface `y` as the Var leaf lexeme on the tree — NOT the uniform
    // `"_"` placeholder. The param-binding pattern AND the body reference
    // are both alpha-renameable `Var` leaves.
    let tree = tree_of("fn f(y: i32) -> i32 { y + 1 }");
    let mut lexemes = Vec::new();
    leaf_lexemes(&tree, &mut lexemes);
    assert!(
        lexemes.iter().any(|l| l == "y"),
        "expected the real identifier `y` as a Var leaf lexeme, got: {lexemes:?}"
    );
    assert!(
        !lexemes.iter().any(|l| l == "_"),
        "no Var leaf should still display the uniform `_` placeholder, got: {lexemes:?}"
    );
}

#[test]
fn alpha_rename_keeps_fingerprint_but_changes_var_lexeme() {
    // #138 contract: two structurally-identical bodies that differ ONLY in
    // a local name share the form fingerprint_set (alpha-equivalence) yet
    // expose DIFFERENT Var leaf lexemes on the tree.
    let n = SynNormalizer::new();
    let a = "fn f(y: i32) -> i32 { y + 1 }";
    let b = "fn f(z: i32) -> i32 { z + 1 }";
    let fa = n.normalize(a, &path()).expect("normalize a");
    let fb = n.normalize(b, &path()).expect("normalize b");
    // The fingerprint set is byte-identical (only the local name differs,
    // and the local name is NOT hashed — it collapses to the Var class).
    assert_eq!(
        fa[0].fingerprint_set, fb[0].fingerprint_set,
        "alpha-equivalent bodies must share the same fingerprint_set"
    );

    let ta = n.derive_tree(a, fa[0].span).expect("tree a");
    let tb = n.derive_tree(b, fb[0].span).expect("tree b");
    let mut la = Vec::new();
    let mut lb = Vec::new();
    leaf_lexemes(&ta, &mut la);
    leaf_lexemes(&tb, &mut lb);
    assert!(
        la.contains(&"y".to_string()),
        "tree a must show `y`: {la:?}"
    );
    assert!(
        lb.contains(&"z".to_string()),
        "tree b must show `z`: {lb:?}"
    );
    assert_ne!(la, lb, "the Var lexemes must differ between renamed bodies");
}

#[test]
fn three_member_rename_cluster_fires_rename_count_with_real_lexemes() {
    // #138 + #130: three structurally-identical bodies differing only in
    // the param name (input / value / number) anti-unify to a template
    // with pure-rename holes. `Match::with_template` derives
    // `rename_count >= 1`, and the substitution lexemes are the REAL
    // names (not `_`).
    let n = SynNormalizer::new();
    let srcs = [
        "fn alpha(input: i32) -> i32 { let total = input + 1; total }",
        "fn beta(value: i32) -> i32 { let total = value + 1; total }",
        "fn gamma(number: i32) -> i32 { let total = number + 1; total }",
    ];
    let members: Vec<NormalizedTree> = srcs
        .iter()
        .map(|s| {
            let f = n.normalize(s, &path()).expect("normalize member");
            n.derive_tree(s, f[0].span).expect("tree member")
        })
        .collect();

    let template = antiunify(&members);
    assert!(
        !template.holes.is_empty(),
        "renamed members diverge — the template must carry at least one hole"
    );

    // At least one hole is a pure rename: distinct == 1 (same structural
    // fp) but the bound lexemes carry the real, differing names.
    let rename_hole = template.holes.iter().find(|h| {
        h.divergence.distinct == 1 && {
            let mut lx = h
                .substitutions
                .iter()
                .flat_map(|s| s.elements.iter().map(|e| e.lexeme.as_str()));
            lx.next().is_some_and(|first| lx.any(|l| l != first))
        }
    });
    let rename_hole = rename_hole.expect(
        "a pure-rename hole (same fp, differing real lexemes) must surface for input/value/number",
    );

    // The substitution lexemes are the REAL names — `input`, `value`,
    // `number` — never the `_` placeholder.
    let lexemes: Vec<&str> = rename_hole
        .substitutions
        .iter()
        .flat_map(|s| s.elements.iter().map(|e| e.lexeme.as_str()))
        .collect();
    assert!(
        ["input", "value", "number"]
            .iter()
            .all(|name| lexemes.contains(name)),
        "rename-hole substitutions must show the real param names, got: {lexemes:?}"
    );
    assert!(
        !lexemes.contains(&"_"),
        "no substitution lexeme should be the `_` placeholder, got: {lexemes:?}"
    );

    // `Match::with_template` derives rename_count >= 1.
    let m = Match::new(Vec::new(), 0.9, Tier::ReviewFirst).with_template(template);
    assert!(
        m.rename_count.is_some_and(|c| c >= 1),
        "rename_count must fire on a real rename cluster, got {:?}",
        m.rename_count
    );
}

#[test]
fn tree_nodes_carry_their_own_span_not_the_form_span() {
    // #130: per-node spans. The form spans the whole `fn .. }`; an inner
    // leaf (the `1` literal late in the body) must NOT carry that full
    // form span — its span is the literal's own narrow source range.
    let n = SynNormalizer::new();
    let src = "fn f(y: i32) -> i32 { y + 1 }";
    let forms = n.normalize(src, &path()).expect("normalize");
    let form_span = forms[0].span;
    let tree = n.derive_tree(src, form_span).expect("derive_tree");

    // Find the literal `1` leaf and assert its span is strictly inside the
    // form span (a per-node span, not the form span stamped everywhere).
    fn find_literal_one(tree: &NormalizedTree) -> Option<dry_core::domain::Span> {
        if let Some(leaf) = &tree.leaf {
            if leaf.lexeme == "1" {
                return Some(tree.span);
            }
        }
        tree.children.iter().find_map(find_literal_one)
    }
    let lit_span = find_literal_one(&tree).expect("a literal `1` leaf must exist");
    assert_ne!(
        lit_span, form_span,
        "the `1` literal leaf must carry its OWN span, not the form span"
    );
    // The literal starts strictly after the form's opening `fn` column,
    // and ends before the form's closing brace.
    assert!(
        lit_span.start > form_span.start,
        "leaf span {lit_span:?} should start after the form start {form_span:?}"
    );
    assert!(
        lit_span.end < form_span.end,
        "leaf span {lit_span:?} should end before the form end {form_span:?}"
    );
}

#[test]
fn substitution_spans_are_per_binding_not_form_span() {
    // #130: the substitution table's per-binding spans must be precise.
    // Two renamed bodies anti-unify; the rename hole's substitution
    // elements must carry each binding's OWN span (the identifier's
    // position), not the enclosing form span.
    let n = SynNormalizer::new();
    let a = "fn f(input: i32) -> i32 { input + 1 }";
    let b = "fn f(value: i32) -> i32 { value + 1 }";
    let fa = n.normalize(a, &path()).expect("normalize a");
    let fb = n.normalize(b, &path()).expect("normalize b");
    let form_a = fa[0].span;
    let form_b = fb[0].span;
    let ta = n.derive_tree(a, form_a).expect("tree a");
    let tb = n.derive_tree(b, form_b).expect("tree b");

    let template = antiunify(&[ta, tb]);
    let mut saw_precise_span = false;
    for hole in &template.holes {
        for (k, sub) in hole.substitutions.iter().enumerate() {
            let form_span = if k == 0 { form_a } else { form_b };
            for el in &sub.elements {
                // A real binding span is strictly narrower than the form
                // span (it is the bound subtree's own range).
                if el.span != form_span {
                    saw_precise_span = true;
                }
            }
        }
    }
    assert!(
        saw_precise_span,
        "at least one substitution element must carry a per-binding span \
         distinct from the enclosing form span"
    );
}
