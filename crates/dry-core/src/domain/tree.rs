//! Language-agnostic ordered tree IR — [`NormalizedTree`],
//! [`LeafToken`], [`LeafClass`].
//!
//! `NormalizedTree` is the structured intermediate representation the
//! anti-unification least-general-generalization (LGG) pass operates
//! over (epic #107). Where [`crate::domain::NormalizedForm`] flattens a
//! form into a `HashSet<u64>` of fingerprints for cheap Jaccard
//! similarity, `NormalizedTree` preserves the *ordered tree shape* —
//! the structure anti-unification needs to compute a generalized
//! template (the bag-of-hashes is insufficient for LGG; the tree is the
//! load-bearing IR). The `TreeDeriverPort` (PR 4) emits these from each
//! adapter; the LGG pass (PR 5) consumes them.
//!
//! Per the hexagonal layering ADR
//! (`ops/decisions/dry-rs/adr-hexagonal-layout.md`, filed in PR 2),
//! this module must not import external crates other than `serde`
//! derive. It is POD — every type is plain data with canonical
//! constructors and performs no I/O. No AST library
//! (`syn`, `swc_*`, `oxc_*`, `tree-sitter*`, `proc-macro2`, `quote`)
//! appears here; the tree is the language-agnostic shape that *replaces*
//! direct AST access in `dry-core`.
//!
//! # Known seam: the `label` vocabulary is Rust-shaped at v0.1
//!
//! [`NormalizedTree::label`] is a free-form `String` carrying the
//! adapter's node discriminator. At v0.1 only the syn (Rust) adapter
//! exists, so the label space is effectively the syn AST discriminator
//! strings (e.g. `"ExprBinary"`, `"ItemFn"`). A shared cross-language
//! node vocabulary is **deliberately deferred** to the dry4ts join
//! (v0.6+) — locking a unified label enum before a second adapter
//! validates it would risk a Rust-centric schema, mirroring the
//! `NormalizedForm.node_count` cross-language heuristic deferral (O8
//! ADR). The `String` shape is the documented seam: when dry4ts lands,
//! the label space is reconciled across adapters without reshaping the
//! tree. The `fp` fingerprint, by contrast, lives in the same `u64`
//! space as `NormalizedForm::fingerprint_set` and is cross-toolchain
//! stable today (xxh3 in every adapter).

use serde::{Deserialize, Serialize};

use super::Span;

/// Classification of a leaf token in a [`NormalizedTree`].
///
/// A starter taxonomy shared across adapters — the anti-unification LGG
/// distinguishes leaves by class when deciding whether two leaves
/// generalize to a placeholder (two `Ident`s with different lexemes
/// generalize; an `Ident` and a `Literal` do not). The set is
/// intentionally minimal at v0.1 and **extensible**: as adapters
/// surface finer distinctions (or dry4ts joins with TypeScript-specific
/// token kinds), new variants land additively. `#[non_exhaustive]`
/// keeps every addition non-breaking for downstream pattern matches —
/// external consumers must include a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum LeafClass {
    /// An identifier token (variable, function, type, or field name).
    Ident,
    /// A literal token (numeric, string, char, or boolean literal).
    Literal,
    /// A language keyword (`fn`, `let`, `if`, `return`, …).
    Keyword,
    /// A punctuation or operator token (`+`, `::`, `=>`, `{`, …).
    Punct,
    /// A lifetime token (`'a`, `'static`, …).
    Lifetime,
}

impl LeafClass {
    /// Stable label for this class — the single source of truth shared
    /// by every reporter surface.
    ///
    /// The returned `&'static str` is byte-identical to the serde wire
    /// rendering (`#[serde(rename_all = "snake_case")]`): `ident` /
    /// `literal` / `keyword` / `punct` / `lifetime`. Reporters that need
    /// a display label call this instead of re-spelling the mapping, so
    /// a new variant breaks exactly one match arm.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ident => "ident",
            Self::Literal => "literal",
            Self::Keyword => "keyword",
            Self::Punct => "punct",
            Self::Lifetime => "lifetime",
        }
    }
}

/// A leaf token carried by a leaf [`NormalizedTree`] node.
///
/// `class` drives anti-unification's generalize-or-not decision;
/// `lexeme` preserves the original surface text so the LGG can report a
/// concrete instantiation when two leaves match exactly. Internal
/// (non-leaf) tree nodes carry `leaf: None`; only leaves carry a
/// `LeafToken`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LeafToken {
    /// Token classification driving the generalize-or-not decision.
    pub class: LeafClass,
    /// Original surface text of the token (e.g. `"foo"`, `"42"`, `"fn"`).
    pub lexeme: String,
}

impl LeafToken {
    /// Construct a [`LeafToken`] from its class and lexeme.
    ///
    /// # Examples
    ///
    /// ```
    /// use dry_core::domain::{LeafClass, LeafToken};
    /// let tok = LeafToken::new(LeafClass::Ident, "foo".to_string());
    /// assert_eq!(tok.class, LeafClass::Ident);
    /// assert_eq!(tok.lexeme, "foo");
    /// ```
    #[must_use]
    pub const fn new(class: LeafClass, lexeme: String) -> Self {
        Self { class, lexeme }
    }
}

/// Language-agnostic ordered tree node — the IR the anti-unification
/// LGG pass operates over.
///
/// A `NormalizedTree` is either an **internal node** (`leaf == None`,
/// with zero or more `children`) or a **leaf node** (`leaf == Some(_)`,
/// conventionally with no `children`). The shape is an *ordered* tree:
/// child order is significant, mirroring source order, because
/// anti-unification aligns children positionally.
///
/// # Fields
///
/// - `label` — the adapter's node discriminator string. Rust-shaped at
///   v0.1 (syn discriminators); cross-language vocabulary deferred to
///   the dry4ts join — see the module-level "Known seam" note.
/// - `fp` — the fold fingerprint of this subtree, in the same `u64`
///   space as [`crate::domain::NormalizedForm::fingerprint_set`]. Two
///   subtrees with equal `fp` are structurally identical under the
///   adapter's fold; the LGG short-circuits on `fp` equality.
/// - `children` — ordered child subtrees. Empty for leaves and for
///   childless internal nodes.
/// - `leaf` — `Some(LeafToken)` for leaf nodes, `None` for internal
///   nodes.
/// - `span` — source range of the subtree, end-inclusive (reuses the
///   canonical [`Span`] coordinate contract).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NormalizedTree {
    /// Adapter node-discriminator string (Rust-shaped at v0.1; see the
    /// module-level "Known seam" note on cross-language vocabulary).
    pub label: String,
    /// Fold fingerprint of this subtree, in the same `u64` space as
    /// [`crate::domain::NormalizedForm::fingerprint_set`].
    pub fp: u64,
    /// Ordered child subtrees; empty for leaves.
    pub children: Vec<NormalizedTree>,
    /// `Some` for leaf nodes, `None` for internal nodes.
    pub leaf: Option<LeafToken>,
    /// Source range of the subtree, end-inclusive.
    pub span: Span,
}

impl NormalizedTree {
    /// Construct an **internal** [`NormalizedTree`] node from its label,
    /// fingerprint, ordered children, and span. `leaf` is set to `None`.
    ///
    /// Use [`NormalizedTree::leaf`] to construct a leaf node instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use dry_core::domain::{LineColumn, NormalizedTree, Span};
    /// let span = Span::try_new(LineColumn::new(1, 0), LineColumn::new(1, 4)).unwrap();
    /// let node = NormalizedTree::new("ItemFn".to_string(), 0xABCD, Vec::new(), span);
    /// assert_eq!(node.label, "ItemFn");
    /// assert!(node.leaf.is_none());
    /// ```
    #[must_use]
    pub const fn new(label: String, fp: u64, children: Vec<NormalizedTree>, span: Span) -> Self {
        Self {
            label,
            fp,
            children,
            leaf: None,
            span,
        }
    }

    /// Construct a **leaf** [`NormalizedTree`] node from its label,
    /// fingerprint, leaf token, and span. `children` is set to empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use dry_core::domain::{LeafClass, LeafToken, LineColumn, NormalizedTree, Span};
    /// let span = Span::try_new(LineColumn::new(1, 0), LineColumn::new(1, 2)).unwrap();
    /// let tok = LeafToken::new(LeafClass::Ident, "foo".to_string());
    /// let node = NormalizedTree::leaf("Ident".to_string(), 0x1234, tok, span);
    /// assert!(node.children.is_empty());
    /// assert!(node.leaf.is_some());
    /// ```
    #[must_use]
    pub const fn leaf(label: String, fp: u64, leaf: LeafToken, span: Span) -> Self {
        Self {
            label,
            fp,
            children: Vec::new(),
            leaf: Some(leaf),
            span,
        }
    }

    /// Whether this node is a leaf (`leaf.is_some()`).
    #[must_use]
    pub const fn is_leaf(&self) -> bool {
        self.leaf.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::LineColumn;

    fn make_span() -> Span {
        Span::try_new(LineColumn::new(1, 0), LineColumn::new(3, 12)).unwrap()
    }

    #[test]
    fn leaf_class_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&LeafClass::Ident).unwrap(),
            "\"ident\""
        );
        assert_eq!(
            serde_json::to_string(&LeafClass::Literal).unwrap(),
            "\"literal\""
        );
        assert_eq!(
            serde_json::to_string(&LeafClass::Keyword).unwrap(),
            "\"keyword\""
        );
        assert_eq!(
            serde_json::to_string(&LeafClass::Punct).unwrap(),
            "\"punct\""
        );
        assert_eq!(
            serde_json::to_string(&LeafClass::Lifetime).unwrap(),
            "\"lifetime\""
        );
    }

    #[test]
    fn leaf_class_round_trips_each_variant_through_json() {
        for class in [
            LeafClass::Ident,
            LeafClass::Literal,
            LeafClass::Keyword,
            LeafClass::Punct,
            LeafClass::Lifetime,
        ] {
            let json = serde_json::to_string(&class).unwrap();
            let back: LeafClass = serde_json::from_str(&json).unwrap();
            assert_eq!(back, class);
        }
    }

    #[test]
    fn leaf_class_as_str_matches_serde_label() {
        // `as_str` is the single source of truth shared by reporters; it
        // MUST stay byte-identical to the serde wire rendering so display
        // surfaces never drift from the JSON envelope.
        for class in [
            LeafClass::Ident,
            LeafClass::Literal,
            LeafClass::Keyword,
            LeafClass::Punct,
            LeafClass::Lifetime,
        ] {
            let json = serde_json::to_string(&class).unwrap();
            let unquoted = json.trim_matches('"');
            assert_eq!(class.as_str(), unquoted, "class label drifted from serde");
        }
    }

    // `#[non_exhaustive]` discipline: `LeafClass` is an enum, so it
    // carries `#[non_exhaustive]` (enums-YES). External crates that
    // match on it MUST include a wildcard arm — adding a variant is a
    // non-breaking change. The result structs `NormalizedTree` and
    // `LeafToken` deliberately do NOT carry the attribute (structs-NO);
    // they evolve via the `new` / `leaf` constructors. Within this
    // crate, matches may stay exhaustive (the attribute is a no-op for
    // same-crate code), as `leaf_class_as_str_matches_serde_label`
    // exercises.

    #[test]
    fn leaf_token_new_stores_fields() {
        let tok = LeafToken::new(LeafClass::Ident, "foo".to_string());
        assert_eq!(tok.class, LeafClass::Ident);
        assert_eq!(tok.lexeme, "foo");
    }

    #[test]
    fn leaf_token_serde_round_trips() {
        let tok = LeafToken::new(LeafClass::Literal, "42".to_string());
        let json = serde_json::to_string(&tok).unwrap();
        let back: LeafToken = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tok);
    }

    #[test]
    fn normalized_tree_new_builds_internal_node() {
        let node = NormalizedTree::new("ItemFn".to_string(), 0xABCD, Vec::new(), make_span());
        assert_eq!(node.label, "ItemFn");
        assert_eq!(node.fp, 0xABCD);
        assert!(node.children.is_empty());
        assert!(node.leaf.is_none());
        assert!(!node.is_leaf());
    }

    #[test]
    fn normalized_tree_leaf_builds_leaf_node() {
        let tok = LeafToken::new(LeafClass::Ident, "foo".to_string());
        let node = NormalizedTree::leaf("Ident".to_string(), 0x1234, tok.clone(), make_span());
        assert_eq!(node.label, "Ident");
        assert_eq!(node.fp, 0x1234);
        assert!(node.children.is_empty());
        assert_eq!(node.leaf, Some(tok));
        assert!(node.is_leaf());
    }

    #[test]
    fn normalized_tree_serde_round_trips_nested_tree() {
        // Internal node with two children, one of which is a leaf:
        //
        //   ExprBinary (internal)
        //   ├── Ident "x"        (leaf)
        //   └── ExprLit (internal)
        let leaf_child = NormalizedTree::leaf(
            "Ident".to_string(),
            0x0001,
            LeafToken::new(LeafClass::Ident, "x".to_string()),
            make_span(),
        );
        let internal_child =
            NormalizedTree::new("ExprLit".to_string(), 0x0002, Vec::new(), make_span());
        let root = NormalizedTree::new(
            "ExprBinary".to_string(),
            0x0003,
            vec![leaf_child, internal_child],
            make_span(),
        );

        let json = serde_json::to_string(&root).unwrap();
        let back: NormalizedTree = serde_json::from_str(&json).unwrap();
        assert_eq!(back, root);
    }

    #[test]
    fn normalized_tree_preserves_child_order() {
        // The tree is ordered — anti-unification aligns children
        // positionally, so child order must round-trip exactly.
        let a = NormalizedTree::leaf(
            "Ident".to_string(),
            1,
            LeafToken::new(LeafClass::Ident, "a".to_string()),
            make_span(),
        );
        let b = NormalizedTree::leaf(
            "Ident".to_string(),
            2,
            LeafToken::new(LeafClass::Ident, "b".to_string()),
            make_span(),
        );
        let root = NormalizedTree::new("Tuple".to_string(), 3, vec![a, b], make_span());
        let back: NormalizedTree =
            serde_json::from_str(&serde_json::to_string(&root).unwrap()).unwrap();
        assert_eq!(back.children[0].leaf.as_ref().unwrap().lexeme, "a");
        assert_eq!(back.children[1].leaf.as_ref().unwrap().lexeme, "b");
    }

    #[test]
    fn normalized_tree_leaf_node_serde_round_trips() {
        let node = NormalizedTree::leaf(
            "Lit".to_string(),
            0xDEAD_BEEF,
            LeafToken::new(LeafClass::Literal, "1.0".to_string()),
            make_span(),
        );
        let back: NormalizedTree =
            serde_json::from_str(&serde_json::to_string(&node).unwrap()).unwrap();
        assert_eq!(back, node);
        assert!(back.is_leaf());
    }
}
