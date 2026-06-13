//! On-demand ordered-tree re-derivation — the [`dry_core::ports::TreeDeriverPort`]
//! implementation for Rust source (epic #107, build-plan PR 4).
//!
//! Detection stays bag-of-hashes; this module re-derives an ordered
//! [`NormalizedTree`] for ONE form, addressed by its source span, AFTER
//! detection has clustered. The anti-unification LGG pass (PR 5)
//! consumes the returned trees.
//!
//! ## One dispatch, two sinks — no drift by construction
//!
//! [`TreeBuilder`] is a [`SubformSink`] (`Out = NormalizedTree`) that
//! shares the SAME `walk_*` dispatch as the fingerprint fold
//! ([`super::walker::FormEmitter`]). Both drive the identical node
//! lifecycle over the identical syn enumeration
//! ([`super::walker::enumerate_forms`]), so the tree and the fingerprint
//! bag cannot disagree on which subforms exist, in what order, or with
//! which tags. What they EMIT differs: the fold collapses each node to a
//! `u64`; the tree builder materialises a node. Crucially, every
//! internal tree node's `fp` is computed with the SAME `Xxh3` primitive
//! sequence the fold uses (`tag`, then each child `fp`, then each leaf
//! token), so an internal node's `fp` is byte-identical to the fold's
//! seal for that node — and is therefore a member of the form's
//! `fingerprint_set`. This is the P3 anti-drift bridge:
//!
//! - the dedup'd SET of internal-node `fp`s in a re-derived tree is a
//!   subset of the form's `fingerprint_set` (every internal node's fold
//!   appears in the bag), AND
//! - the synthetic root's `fp` equals the form's top-level fold
//!   ([`super::visitor::fold_form_fp`] over the ordered `Attrs?`, `Sig`,
//!   `Block` subform fps) — the same fold both sinks compute from the
//!   same ordered top-level children.
//!
//! ## Drift notes replicated from #121's visitor
//!
//! - **`walk_expr` opens untagged then tags after dispatch.** The sink's
//!   default [`SubformSink::begin`] (open + tag together) is used by
//!   every node except expressions; `walk_expr` calls
//!   [`SubformSink::begin_node`] then [`SubformSink::tag`] once its
//!   category dispatch picks the discriminator. [`TreeBuilder`] honors
//!   this: `begin_node` opens an untagged builder (label `None`, hasher
//!   primed with nothing), `tag` sets the label AND hashes the tag — so
//!   an expr node's tag enters the fp in the same position as the fold's.
//! - **`hash_attrs` is a form-level prelude that conditionally seals.**
//!   Form enumeration drives the shared [`super::visitor::walk_attrs`],
//!   which seals the `Attrs` subform ONLY when a preserved attribute is
//!   seen. [`TreeBuilder`] inherits that rule verbatim (it is the same
//!   `walk_attrs` call), so an attribute-free form gains no phantom
//!   `Attrs` child in the tree, exactly as it gains no phantom fp.
//!
//! ## Span granularity seam
//!
//! The [`SubformSink`] lifecycle carries no per-node span (the
//! fingerprint fold never needed one). At v0.1 the tree builder stamps
//! every node with the FORM's span — coarse but sufficient for the LGG's
//! structural alignment (PR 5 derives per-hole substitution spans from
//! the member subtrees it consumes, not from these node spans). Threading
//! precise per-node spans through the sink is a documented follow-up; it
//! does not affect the fp bridge (spans are not hashed).

use dry_core::domain::{LeafClass, LeafToken, NormalizedTree, Span};
use dry_core::ports::{NormalizeError, TreeDeriverPort};
use xxhash_rust::xxh3::Xxh3;

use super::token::NormalizedToken;
use super::visitor::{self, SubformSink};
use super::walker::{FormParts, FormVisitor, enumerate_forms};
use crate::parser::SynNormalizer;

/// In-progress accumulator for one [`NormalizedTree`] node.
///
/// Held open between [`SubformSink::begin_node`] and
/// [`SubformSink::seal`]. The `fp` hasher reproduces the fingerprint
/// fold's per-node `Xxh3` exactly (tag, child fps, leaf tokens — in walk
/// order); `label`/`children`/`leaf` accumulate the tree shape.
pub(super) struct TreeNodeBuilder {
    /// Node fingerprint hasher — fed the SAME primitives, in the SAME
    /// order, as [`super::walker::FormEmitter`]'s per-node `Xxh3`.
    fp: Xxh3,
    /// Structural kind tag (`"Block"`, `"ExprBinary"`, …); `None` until
    /// [`SubformSink::tag`] runs (the untagged-then-tagged expr path).
    label: Option<&'static str>,
    /// Ordered child subtrees (folded children + recorded leaves).
    children: Vec<NormalizedTree>,
}

/// The tree-path [`SubformSink`]: materialises a [`NormalizedTree`] per
/// node while computing each node's `fp` byte-identically to the
/// fingerprint fold.
///
/// `span` is the enclosing form's span, stamped on every node (see the
/// module-level span-granularity seam).
pub(super) struct TreeBuilder {
    span: Span,
}

impl TreeBuilder {
    const fn new(span: Span) -> Self {
        Self { span }
    }
}

impl SubformSink for TreeBuilder {
    type Out = NormalizedTree;
    type Node = TreeNodeBuilder;

    fn begin_node(&mut self) -> Self::Node {
        TreeNodeBuilder {
            fp: Xxh3::new(),
            label: None,
            children: Vec::new(),
        }
    }

    fn tag(&mut self, node: &mut Self::Node, tag: &'static str) {
        // Hash the tag EXACTLY as the fold does (`&'static str: Hash`),
        // so the sealed fp matches the fingerprint bag.
        use std::hash::Hash;
        tag.hash(&mut node.fp);
        node.label = Some(tag);
    }

    fn fold(&mut self, node: &mut Self::Node, child: Self::Out) {
        // The fold hashes the child's `u64`; we hash the child node's
        // `fp` (the same `u64`) so the parent fp matches, then retain the
        // child subtree.
        use std::hash::Hash;
        child.fp.hash(&mut node.fp);
        node.children.push(child);
    }

    fn leaf(&mut self, node: &mut Self::Node, token: &NormalizedToken) {
        // Hash the token into the PARENT fp exactly as the fold does (the
        // fold never gives a leaf its own fp; leaves carry no bag entry).
        token.hash_into(&mut node.fp);
        // Materialise the leaf as a child subtree. Its own fp is the hash
        // of just this token — a defined, deterministic value used only
        // for tree identity/LGG short-circuit; it is intentionally NOT a
        // `fingerprint_set` member (the bridge scopes to INTERNAL nodes).
        let mut leaf_fp = Xxh3::new();
        token.hash_into(&mut leaf_fp);
        let (class, lexeme) = leaf_class_and_lexeme(token);
        node.children.push(NormalizedTree::leaf(
            leaf_label(class),
            std::hash::Hasher::finish(&leaf_fp),
            LeafToken::new(class, lexeme),
            self.span,
        ));
    }

    fn seal(&mut self, node: Self::Node) -> Self::Out {
        let fp = std::hash::Hasher::finish(&node.fp);
        // An untagged seal can only happen if a future dispatch path
        // forgets to tag; default to the same fallback the fold's
        // discriminator-less seal would carry an empty tag for. We label
        // it explicitly so the tree is never silently mislabelled.
        let label = node.label.unwrap_or("Untagged").to_string();
        NormalizedTree::new(label, fp, node.children, self.span)
    }

    fn record_identifier(&mut self, _id: String) {
        // The O11 identifier side-channel is a fingerprint-path concern
        // (rename signal). The tree path does not consume it.
    }
}

/// Map a [`NormalizedToken`] to its [`LeafClass`] + display lexeme.
///
/// The class drives the LGG's generalize-or-not decision; the lexeme
/// preserves surface text for the substitution display. This is the
/// syn→leaf mapping that stays in the adapter (dry4rs), mirroring the
/// fold's token vocabulary so the same tokens classify consistently.
fn leaf_class_and_lexeme(token: &NormalizedToken) -> (LeafClass, String) {
    match token {
        NormalizedToken::Var => (LeafClass::Ident, "_".to_string()),
        NormalizedToken::Ident(name) | NormalizedToken::PathSeg(name) => {
            (LeafClass::Ident, name.clone())
        }
        NormalizedToken::MacroCall(name) => (LeafClass::Ident, format!("{name}!")),
        NormalizedToken::Attr(name) => (LeafClass::Ident, format!("#[{name}]")),
        NormalizedToken::TypeParam => (LeafClass::Ident, "T".to_string()),
        NormalizedToken::Lifetime => (LeafClass::Lifetime, "'_".to_string()),
        NormalizedToken::LifetimeStatic => (LeafClass::Lifetime, "'static".to_string()),
        NormalizedToken::Op(sym) => (LeafClass::Punct, (*sym).to_string()),
        NormalizedToken::Kw(kw) | NormalizedToken::Modifier(kw) => {
            (LeafClass::Keyword, (*kw).to_string())
        }
        NormalizedToken::LitInt(v) => (LeafClass::Literal, v.to_string()),
        NormalizedToken::LitFloat(bits) => (LeafClass::Literal, f64::from_bits(*bits).to_string()),
        NormalizedToken::LitStr(s) => (LeafClass::Literal, format!("{s:?}")),
        NormalizedToken::LitBool(b) => (LeafClass::Literal, b.to_string()),
        NormalizedToken::LitChar(c) => (LeafClass::Literal, format!("{c:?}")),
        NormalizedToken::LitByte(b) => {
            let escaped: String = std::ascii::escape_default(*b).map(|c| c as char).collect();
            (LeafClass::Literal, format!("b'{escaped}'"))
        }
        NormalizedToken::LitByteStr(bytes) => {
            (LeafClass::Literal, format!("b\"{} bytes\"", bytes.len()))
        }
        NormalizedToken::Closure => (LeafClass::Keyword, "closure".to_string()),
        NormalizedToken::NestedFn => (LeafClass::Keyword, "fn".to_string()),
    }
}

/// Leaf node label from its class — the leaf's `NormalizedTree.label`.
fn leaf_label(class: LeafClass) -> String {
    class.as_str().to_string()
}

/// The tree-path [`FormVisitor`]: builds one `(form_span, NormalizedTree)`
/// per enumerated form by driving a [`TreeBuilder`] over the same
/// `(attrs, sig, block)` the fingerprint path uses, then folding the
/// ordered top-level subforms into a synthetic `"Form"` root.
struct TreeCollector {
    trees: Vec<(Span, NormalizedTree)>,
}

impl FormVisitor for TreeCollector {
    fn visit_form(&mut self, parts: FormParts<'_>) {
        let mut builder = TreeBuilder::new(parts.span);

        // Top-level subforms in emission order, exactly mirroring the
        // fingerprint path (Attrs prelude — conditional — then Sig, then
        // Block). `walk_attrs` is the SHARED prelude: it yields `Some`
        // only when a preserved attribute sealed an `Attrs` subform.
        let mut children = Vec::new();
        if let Some(attrs_tree) = visitor::walk_attrs(&mut builder, parts.attrs) {
            children.push(attrs_tree);
        }
        children.push(visitor::walk_sig(&mut builder, parts.sig));
        children.push(visitor::walk_block(&mut builder, parts.block));

        // Synthetic root: fold the ordered top-level subform fps into the
        // form-level fingerprint (the SAME `fold_form_fp` the bridge
        // checks against). The root's children ARE the top-level subforms.
        let child_fps: Vec<u64> = children.iter().map(|t| t.fp).collect();
        let root_fp = visitor::fold_form_fp(&child_fps);
        let root = NormalizedTree::new("Form".to_string(), root_fp, children, parts.span);
        self.trees.push((parts.span, root));
    }
}

/// Re-derive every form's `(form_span, NormalizedTree)` from `source`.
///
/// Re-parses with syn and drives the SHARED enumeration in
/// [`TreeBuilder`] mode. Returns the per-form trees in source order.
///
/// # Errors
///
/// Returns [`NormalizeError::Parse`] when `source` does not parse.
fn derive_all_trees(source: &str) -> Result<Vec<(Span, NormalizedTree)>, NormalizeError> {
    let file = syn::parse_file(source).map_err(|err| NormalizeError::Parse {
        message: err.to_string(),
        span: None,
    })?;
    // `in_test_file = false`: FormKind does not affect tree shape or
    // form identity (the span), so the test-context seed is irrelevant
    // here — the tree path is span-addressed, not kind-addressed.
    let mut collector = TreeCollector { trees: Vec::new() };
    enumerate_forms(&file.items, false, &mut collector);
    Ok(collector.trees)
}

impl TreeDeriverPort for SynNormalizer {
    /// Re-derive the [`NormalizedTree`] for the form at `span`.
    ///
    /// Re-parses `source`, enumerates every `(form_span, NormalizedTree)`
    /// via the shared visitor, and returns the tree whose
    /// `form_span == span`.
    ///
    /// ## Two forms at the same span
    ///
    /// Form identity is `(file, span)`; within a single file the
    /// enumeration cannot produce two distinct forms at the byte-
    /// identical `(fn_token .. close-brace)` span (each fn occupies a
    /// disjoint source range). Should a future construct ever collide,
    /// this method returns the FIRST match in source order — a stable,
    /// documented tie-break (there is no `NodeIdent` input-index tie-break
    /// at this layer). The contract is "first form whose span equals the
    /// address", never a panic.
    ///
    /// # Errors
    ///
    /// Returns [`NormalizeError::Parse`] when `source` does not parse, or
    /// when no enumerated form has `form_span == span` (source changed
    /// between detection and re-derive, or the span addresses no form).
    fn derive_tree(&self, source: &str, span: Span) -> Result<NormalizedTree, NormalizeError> {
        let trees = derive_all_trees(source)?;
        trees
            .into_iter()
            .find(|(form_span, _)| *form_span == span)
            .map(|(_, tree)| tree)
            .ok_or_else(|| NormalizeError::Parse {
                message: format!(
                    "no form found at span {span:?} (source may have changed since detection)"
                ),
                span: Some(span),
            })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use dry_core::domain::FilePath;
    use dry_core::ports::NormalizerPort;

    use super::*;

    fn path() -> FilePath {
        FilePath::from(PathBuf::from("fixture.rs"))
    }

    /// Collect every INTERNAL (non-leaf) node's fp into a dedup'd set —
    /// the left-hand side of the P3 bridge.
    fn internal_fps(tree: &NormalizedTree, out: &mut HashSet<u64>) {
        if !tree.is_leaf() {
            out.insert(tree.fp);
            for child in &tree.children {
                internal_fps(child, out);
            }
        }
    }

    /// Recursively assert leaf/internal node invariants and report
    /// whether a literal leaf with the given lexeme was seen.
    fn contains_literal_leaf(tree: &NormalizedTree, lexeme: &str) -> bool {
        if let Some(leaf) = &tree.leaf {
            assert!(tree.children.is_empty(), "leaf must have no children");
            return leaf.class == LeafClass::Literal && leaf.lexeme == lexeme;
        }
        assert!(tree.leaf.is_none(), "internal node carries no leaf token");
        tree.children
            .iter()
            .any(|c| contains_literal_leaf(c, lexeme))
    }

    #[test]
    fn derive_tree_returns_tree_for_known_span() {
        let n = SynNormalizer::new();
        let src = "fn add(x: i32, y: i32) -> i32 { x + y }";
        // Resolve the form's span from the normalizer (same identity).
        let forms = n.normalize(src, &path()).unwrap();
        let span = forms[0].span;
        let tree = n.derive_tree(src, span).unwrap();
        assert_eq!(tree.label, "Form");
        assert_eq!(tree.span, span);
        assert!(!tree.is_leaf());
        // Root has Sig + Block top-level children (no preserved attr).
        assert_eq!(tree.children.len(), 2);
    }

    #[test]
    fn p3_bridge_internal_fps_subset_and_root_fp_matches() {
        // THE merge-gate contract. For a corpus of forms: every internal
        // node fp of the re-derived tree is in the form's fingerprint_set,
        // AND the root.fp equals the form-level top-level fold computed
        // independently from the form's emitted subforms.
        let n = SynNormalizer::new();
        let corpus = [
            "fn empty() {}",
            "fn add(x: i32, y: i32) -> i32 { x + y }",
            "#[inline] fn host() { let _x = 0; }",
            "#[test] fn t() { assert_eq!(1, 1); }",
            "fn cond(n: i32) -> bool { if n > 0 { true } else { false } }",
            "fn mch(n: i32) -> i32 { match n { 0 => 0, _ => 1 } }",
            "struct S; impl S { fn new() -> Self { S } }",
            "trait T { fn dm(&self) -> i32 { 0 } }",
            "fn loops() { for i in 0..3 { let _ = i; } }",
            "async fn a() -> i32 { 1 }",
            "fn cmp<T: Ord>(a: T, b: T) -> bool { a < b }",
        ];
        for src in corpus {
            let forms = n.normalize(src, &path()).unwrap();
            for form in &forms {
                let tree = n.derive_tree(src, form.span).unwrap();
                // (1) internal-node fps subset of fingerprint_set.
                let mut fps = HashSet::new();
                internal_fps(&tree, &mut fps);
                // The root "Form" fp is a SYNTHETIC top-level fold, not a
                // bag member by itself — exclude it from the subset check
                // (the bag holds the per-subform seals, not the form
                // fold). Every OTHER internal node is a real seal.
                let non_root: HashSet<u64> =
                    fps.iter().copied().filter(|fp| *fp != tree.fp).collect();
                assert!(
                    non_root.is_subset(&form.fingerprint_set),
                    "internal-node fps drifted from fingerprint_set for source {src:?}: \
                     extra = {:?}",
                    non_root
                        .difference(&form.fingerprint_set)
                        .collect::<Vec<_>>()
                );
                // (2) root.fp == fold over the ordered top-level subforms.
                let child_fps: Vec<u64> = tree.children.iter().map(|t| t.fp).collect();
                assert_eq!(
                    tree.fp,
                    visitor::fold_form_fp(&child_fps),
                    "root fp must equal the top-level fold for source {src:?}"
                );
                // The top-level subform fps are themselves bag members
                // (Attrs?/Sig/Block seals), reinforcing the bridge.
                for child in &tree.children {
                    assert!(
                        form.fingerprint_set.contains(&child.fp),
                        "top-level subform fp {} missing from bag for {src:?}",
                        child.fp
                    );
                }
            }
        }
    }

    #[test]
    fn derive_tree_is_deterministic() {
        let n = SynNormalizer::new();
        let src = "fn f(n: i32) -> i32 { n + 1 }";
        let span = n.normalize(src, &path()).unwrap()[0].span;
        let a = n.derive_tree(src, span).unwrap();
        let b = n.derive_tree(src, span).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn parse_error_returns_err_without_panic() {
        let n = SynNormalizer::new();
        let span = Span::try_new(
            dry_core::domain::LineColumn::new(1, 0),
            dry_core::domain::LineColumn::new(1, 1),
        )
        .unwrap();
        let err = n
            .derive_tree("fn this is not valid rust { ;;; }", span)
            .expect_err("invalid source must Err, never panic");
        let NormalizeError::Parse { message, .. } = err else {
            panic!("expected Parse variant");
        };
        assert!(!message.is_empty());
    }

    #[test]
    fn span_not_present_returns_err() {
        // A well-formed source, but an address that no form occupies.
        let n = SynNormalizer::new();
        let src = "fn only() { let _ = 1; }";
        let bogus = Span::try_new(
            dry_core::domain::LineColumn::new(999, 0),
            dry_core::domain::LineColumn::new(999, 1),
        )
        .unwrap();
        let err = n.derive_tree(src, bogus).expect_err("absent span must Err");
        assert!(matches!(err, NormalizeError::Parse { .. }));
    }

    #[test]
    fn two_distinct_forms_resolve_to_their_own_trees() {
        // Two forms in one file: each span addresses its own tree; the
        // first-match tie-break is exercised implicitly (spans differ, so
        // each resolves uniquely).
        let n = SynNormalizer::new();
        let src = "fn one() -> i32 { 1 }\nfn two() -> i32 { 2 }";
        let forms = n.normalize(src, &path()).unwrap();
        assert_eq!(forms.len(), 2);
        let t0 = n.derive_tree(src, forms[0].span).unwrap();
        let t1 = n.derive_tree(src, forms[1].span).unwrap();
        assert_eq!(t0.span, forms[0].span);
        assert_eq!(t1.span, forms[1].span);
        // The two literals differ, so the trees differ.
        assert_ne!(t0, t1);
    }

    #[test]
    fn leaf_nodes_carry_tokens_internal_nodes_do_not() {
        let n = SynNormalizer::new();
        let src = "fn lit() -> i32 { 42 }";
        let span = n.normalize(src, &path()).unwrap()[0].span;
        let tree = n.derive_tree(src, span).unwrap();
        // A literal leaf for "42" must exist; the helper also asserts the
        // leaf/internal-node structural invariants as it walks.
        assert!(
            contains_literal_leaf(&tree, "42"),
            "expected a literal leaf with lexeme 42"
        );
    }
}
