//! Typed placeholder vocabulary for the syn normalizer.
//!
//! Per the O5 ADR (`ops/decisions/dry-rs/adr-rust-normalization-rules.md`),
//! the normalizer projects every syn AST node through a fixed
//! placeholder vocabulary before hashing. The projection layer is this
//! module: every leaf-token in the placeholder stream is a
//! [`NormalizedToken`] variant.
//!
//! **Never hash syn types directly.** syn's `extra-traits` feature
//! derives `Hash` on its types, but that `Hash` is over syn's enum
//! layout, which can change in a 2.0.x point release. Projecting
//! through `NormalizedToken` first decouples the fingerprint
//! vocabulary from syn's internal shape.
//!
//! The variant set is **closed at v0.1**. Adding a new placeholder
//! class requires an ADR amendment + a `PlaceholderPolicy`
//! `v0_2_default()` constructor.

use std::hash::{Hash, Hasher};

/// A leaf-token in the placeholder-substituted fingerprint stream.
///
/// Subtree structural shape is NOT carried by `NormalizedToken` — it's
/// represented by the recursive Merkle-style hashing in the walker
/// (each subform's `u64` folds in its children's `u64`s). This enum
/// is only the *leaf* vocabulary; the AST-node-kind tag at each
/// recursion boundary is a bare `&'static str` hashed alongside the
/// children's `u64`s.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NormalizedToken {
    /// Local variable identifier (collapsed — alpha-equivalent).
    Var,
    /// Function / method / referenced-name identifier (preserved).
    Ident(String),
    /// Generic type parameter (collapsed).
    TypeParam,
    /// Lifetime parameter (collapsed; `'static` uses `LifetimeStatic`).
    Lifetime,
    /// The `'static` lifetime (preserved).
    LifetimeStatic,
    /// Operator symbol (`+`, `==`, `?`, `.`, …). `&'static str` because
    /// the operator vocabulary is fixed.
    Op(&'static str),
    /// Control-flow keyword (`if`, `for`, `while`, `loop`, `match`,
    /// `return`, `break`, `continue`, `yield`, `await`).
    Kw(&'static str),
    /// Function-modifier keyword (`async`, `const`, `unsafe`).
    Modifier(&'static str),
    /// Integer literal value.
    LitInt(i128),
    /// Float literal — stored as `u64` bit pattern because `f64` is not
    /// `Hash` (NaN). The bit pattern preserves identity.
    LitFloat(u64),
    /// String literal value.
    LitStr(String),
    /// Boolean literal value.
    LitBool(bool),
    /// Char literal value.
    LitChar(char),
    /// Byte literal value (single byte, e.g., `b'x'`).
    LitByte(u8),
    /// Byte-string literal (raw bytes, e.g., `b"hello"`).
    LitByteStr(Vec<u8>),
    /// Macro invocation (opaque — arguments NOT walked at v0.1).
    MacroCall(String),
    /// Preserved attribute (`#[test]`, `#[inline]`, `#[must_use]`,
    /// etc.). The string carries the path token. Stripped attributes
    /// (`#[derive(...)]`, `#[doc(...)]`) produce no token.
    Attr(String),
    /// Module-path component (preserved as concrete signal).
    PathSeg(String),
    /// Form-boundary marker: an `ExprClosure` was constructed here. The
    /// closure emits its own form; the enclosing form sees only this
    /// opaque marker.
    Closure,
    /// Form-boundary marker: a nested `ItemFn` was declared here. The
    /// inner fn emits its own form; the enclosing form sees only this
    /// opaque marker.
    NestedFn,
}

impl NormalizedToken {
    /// Hash this token into the given hasher.
    ///
    /// Equivalent to `Hash::hash` but exposed as a method for use sites
    /// that build hashes incrementally without going through trait
    /// dispatch.
    pub fn hash_into<H: Hasher>(&self, hasher: &mut H) {
        Hash::hash(self, hasher);
    }
}

#[cfg(test)]
mod tests {
    use xxhash_rust::xxh3::Xxh3;

    use super::*;

    fn hash(t: &NormalizedToken) -> u64 {
        // Exercise the production hash path (cross-toolchain stable
        // xxh3 — same as `super::walker::FormEmitter`). `NormalizedToken`'s
        // `Hash` impl is hasher-agnostic, but testing with the
        // production hasher pins what the walker actually produces.
        let mut h = Xxh3::new();
        t.hash_into(&mut h);
        h.finish()
    }

    #[test]
    fn structurally_equal_tokens_hash_identically() {
        assert_eq!(hash(&NormalizedToken::Var), hash(&NormalizedToken::Var));
        assert_eq!(
            hash(&NormalizedToken::Ident("foo".into())),
            hash(&NormalizedToken::Ident("foo".into()))
        );
        assert_eq!(
            hash(&NormalizedToken::LitInt(42)),
            hash(&NormalizedToken::LitInt(42))
        );
    }

    #[test]
    fn different_idents_hash_differently() {
        assert_ne!(
            hash(&NormalizedToken::Ident("foo".into())),
            hash(&NormalizedToken::Ident("bar".into()))
        );
    }

    #[test]
    fn different_variants_hash_differently() {
        // The discriminator differs even when the payload coincides.
        // `Ident("x")` (function/method name space) vs `PathSeg("x")`
        // (module-path component) must hash distinctly.
        assert_ne!(
            hash(&NormalizedToken::Ident("x".into())),
            hash(&NormalizedToken::PathSeg("x".into()))
        );
    }

    #[test]
    fn var_and_typeparam_are_distinct() {
        // Both are collapsed-class markers but in different name
        // spaces (locals vs generics).
        assert_ne!(
            hash(&NormalizedToken::Var),
            hash(&NormalizedToken::TypeParam)
        );
    }

    #[test]
    fn lifetime_static_distinct_from_bound_lifetime() {
        // 'static is preserved as concrete; other lifetimes collapse.
        assert_ne!(
            hash(&NormalizedToken::Lifetime),
            hash(&NormalizedToken::LifetimeStatic)
        );
    }

    #[test]
    fn closure_and_nested_fn_markers_distinct() {
        // Both are form-boundary markers but they describe different
        // structural shapes (a closure expression vs a nested item).
        assert_ne!(
            hash(&NormalizedToken::Closure),
            hash(&NormalizedToken::NestedFn)
        );
    }

    #[test]
    fn float_lit_uses_bit_pattern_for_determinism() {
        // f64 is not Hash because NaN; LitFloat stores u64 bit pattern.
        let p1 = 1.5_f64.to_bits();
        let p2 = 1.5_f64.to_bits();
        assert_eq!(
            hash(&NormalizedToken::LitFloat(p1)),
            hash(&NormalizedToken::LitFloat(p2))
        );
        let nan_bits = f64::NAN.to_bits();
        // NaN hashes consistently because we use the bit pattern.
        assert_eq!(
            hash(&NormalizedToken::LitFloat(nan_bits)),
            hash(&NormalizedToken::LitFloat(nan_bits))
        );
    }
}
