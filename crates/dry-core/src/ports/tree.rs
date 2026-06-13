//! [`TreeDeriverPort`] — on-demand ordered-tree re-derivation for
//! anti-unification (LGG).
//!
//! The detection path stays bag-of-hashes
//! ([`crate::domain::NormalizedForm::fingerprint_set`], unchanged). This
//! is the SECOND port trait (epic #107): it re-derives an ordered,
//! language-agnostic [`NormalizedTree`] for a single form, addressed by
//! its source span, AFTER detection has already clustered the bag-of-
//! hashes. The anti-unification pass (`comparison::antiunify`, PR 5)
//! consumes the returned trees to compute a generalized template for a
//! cluster's members.
//!
//! ## Why a second port (not a method on [`NormalizerPort`])
//!
//! [`TreeDeriverPort`] earns trait status as a genuine SECOND
//! polymorphism axis — syn (Rust) and swc/oxc (TypeScript) produce
//! structurally different trees from the same source, exactly the litmus
//! that earned [`NormalizerPort`]. Each adapter implements BOTH ports
//! (the Rust adapter `SynNormalizer` impls `NormalizerPort` AND
//! `TreeDeriverPort`); the run loop's generic bound widens additively to
//! `N: NormalizerPort + TreeDeriverPort + Default` (PR 7) — source-
//! compatible for every existing caller, runtime CLI surface unchanged.
//!
//! ## The re-derive contract
//!
//! [`TreeDeriverPort::derive_tree`] RE-PARSES `source`, drives the
//! shared per-adapter visitor in tree-building mode to enumerate every
//! form's `(form_span, NormalizedTree)`, and returns the tree whose
//! `form_span == span`. Form-identity resolution stays in ONE place (the
//! shared visitor that also drives the fingerprint fold), never a
//! parallel span-matcher — this is the construction that keeps the tree
//! path from drifting off the fingerprint path. No exact-span match
//! yields [`NormalizeError::Parse`] (template degrades to `None`
//! downstream); the method never panics.
//!
//! ## Hexagonal layering
//!
//! This port lives in `dry-core` and references only
//! [`NormalizedTree`] (POD, `dry-core::domain`) and the [`Span`]
//! address — NO AST library appears here. The syn → tree mapping lives
//! entirely in the adapter (`dry4rs::parser::tree`). The `dry-core
//! AST-library purity` CI gate covers this module.

use crate::domain::{NormalizedTree, Span};
use crate::ports::NormalizeError;

/// Re-derive the ordered [`NormalizedTree`] for a single form addressed
/// by its source span.
///
/// Each adapter crate implements this trait alongside
/// [`NormalizerPort`](crate::ports::NormalizerPort) for its target
/// source language. The anti-unification pass calls `derive_tree`
/// per cluster member AFTER detection, re-reading the member's source
/// file (cluster members only — re-parse is cheap because clusters are
/// tiny). The bag-of-hashes detection path never invokes this trait.
///
/// # Object safety
///
/// The trait is object-safe — `derive_tree` takes `&self`, no
/// `Self: Sized` requirement, no generic methods — so
/// `Box<dyn TreeDeriverPort>` / `&dyn TreeDeriverPort` are usable
/// shapes. Mirrors [`NormalizerPort`](crate::ports::NormalizerPort)'s
/// object-safe contract; no `Send + Sync` bound on the trait itself
/// (parallelism bounds belong at the comparison-engine call site).
pub trait TreeDeriverPort {
    /// Re-derive the [`NormalizedTree`] for the form at `span`.
    ///
    /// `source` is the form's source FILE contents (the run loop re-reads
    /// the file for cluster-member files only — the source is not
    /// retained from the detection pass). `span` is the form's
    /// identifying span, exactly as it appears on the form's
    /// [`FormRef`](crate::domain::FormRef) / `NormalizedForm`. The
    /// adapter re-parses `source`, drives its shared visitor to
    /// enumerate every form's `(form_span, NormalizedTree)`, and returns
    /// the tree whose `form_span == span`.
    ///
    /// # Errors
    ///
    /// Returns [`NormalizeError::Parse`] when:
    /// - `source` does not parse (whole-file syntax error), or
    /// - no form in the re-parsed source has `form_span == span` (the
    ///   source changed between detection and re-derive, or the span
    ///   does not address an emitted form).
    ///
    /// The method never panics — a missing span is an `Err`, not an
    /// `unwrap` failure, so the downstream template degrades gracefully
    /// to `None`.
    fn derive_tree(&self, source: &str, span: Span) -> Result<NormalizedTree, NormalizeError>;
}

// Compile-time invariant: the port is object-safe, so a future edit
// that introduces a non-object-safe method (`Self: Sized`, a generic
// method, an `&mut self` requirement) fails to compile here. Mirrors
// the `NormalizerPort` assertion in `super::normalizer`.
#[cfg(test)]
static_assertions::assert_obj_safe!(TreeDeriverPort);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LeafClass, LeafToken, LineColumn};

    /// A trivial in-crate implementor that proves the port is usable
    /// both as a generic bound and behind a `&dyn` / `Box<dyn>`
    /// coercion (the runtime-adapter-selection shape).
    struct StubDeriver;

    impl TreeDeriverPort for StubDeriver {
        fn derive_tree(&self, _source: &str, span: Span) -> Result<NormalizedTree, NormalizeError> {
            // Return a one-leaf tree whose span echoes the address so
            // the coercion test can assert the value flowed through.
            Ok(NormalizedTree::leaf(
                "Stub".to_string(),
                0,
                LeafToken::new(LeafClass::Ident, "stub".to_string()),
                span,
            ))
        }
    }

    fn span() -> Span {
        Span::try_new(LineColumn::new(1, 0), LineColumn::new(1, 4)).unwrap()
    }

    #[test]
    fn port_is_usable_behind_dyn_coercion() {
        let stub = StubDeriver;
        let dynamic: &dyn TreeDeriverPort = &stub;
        let tree = dynamic.derive_tree("fn _x() {}", span()).unwrap();
        assert_eq!(tree.span, span());
        assert!(tree.is_leaf());
    }

    #[test]
    fn port_is_usable_as_boxed_trait_object() {
        let boxed: Box<dyn TreeDeriverPort> = Box::new(StubDeriver);
        let tree = boxed.derive_tree("fn _x() {}", span()).unwrap();
        assert_eq!(tree.label, "Stub");
    }
}
