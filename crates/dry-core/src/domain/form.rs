//! Cross-language form representation — [`NormalizedForm`] and
//! [`FormRef`].
//!
//! `NormalizedForm` is the language-agnostic IR the comparison engine
//! operates on. Each adapter's `NormalizerPort::normalize` (lands in
//! PR 4) emits a stream of `NormalizedForm` values; the comparison
//! engine clusters them by `fingerprint_set` and computes Jaccard
//! similarity over the sets.
//!
//! Per-language `FormContext` extensions (`visibility`, `attributes`,
//! `parent_module`, `parent_impl`) are deferred per roadmap — they are
//! heavily Rust-centric and would skew the schema if landed before
//! dry4ts validates cross-language abstraction.
//!
//! `node_count` semantics across languages will be pinned by the O8
//! ADR landing with PR 4 (`adr-normalized-form-schema.md`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{FilePath, FormKind, Span};

/// Cross-language normalized representation of a single form (function,
/// method, doctest body, …).
///
/// The comparison engine treats `NormalizedForm` as the unit of
/// similarity: two forms are compared by Jaccard intersection over
/// their `fingerprint_set`, with `node_count` driving the
/// sliding-window break that bounds the inner loop.
///
/// # Fields
///
/// - `kind` — production / test / doctest classification.
/// - `fingerprint_set` — `HashSet<u64>` of typed-placeholder
///   fingerprints; the comparison engine intersects these.
/// - `span` — source range, end-inclusive.
/// - `node_count` — total normalized-AST nodes; drives the
///   sliding-window break (Jaccard upper bound `min/max >= t`
///   ⟹ `max <= min/t`).
/// - `line_count` — source-line span (end-line minus start-line, plus
///   one); used by reporters and the size-bucketed parallelism in v0.5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedForm {
    /// What category of form this is (production, test, doctest).
    pub kind: FormKind,
    /// Hashed subform fingerprints; Jaccard similarity is computed
    /// over the intersection of two forms' sets.
    pub fingerprint_set: HashSet<u64>,
    /// Source range of the original form, end-inclusive.
    pub span: Span,
    /// Total normalized-AST nodes; load-bearing for the
    /// sliding-window break math.
    pub node_count: u32,
    /// Source line count covered by the form.
    pub line_count: u32,
}

impl NormalizedForm {
    /// Construct a [`NormalizedForm`] from its component parts.
    ///
    /// All inputs are accepted as-is — validation of `node_count` and
    /// `line_count` is the adapter's responsibility (an empty form is
    /// a well-formed but uninteresting input).
    #[must_use]
    pub fn new(
        kind: FormKind,
        fingerprint_set: HashSet<u64>,
        span: Span,
        node_count: u32,
        line_count: u32,
    ) -> Self {
        Self {
            kind,
            fingerprint_set,
            span,
            node_count,
            line_count,
        }
    }
}

/// A reference to a form embedded inside a `Match`.
///
/// `FormRef` carries only the file, span, and kind — it is the
/// reporter-friendly shape that names a form without dragging the
/// full `fingerprint_set` along. Each `Match.forms` element is a
/// `FormRef`; the original `NormalizedForm` instances do not flow
/// into the wire envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormRef {
    /// Workspace- or file-relative path of the source file.
    pub file: FilePath,
    /// Source range of the form, end-inclusive.
    pub span: Span,
    /// What category of form this is.
    pub kind: FormKind,
}

impl FormRef {
    /// Construct a [`FormRef`] from its component parts.
    #[must_use]
    pub const fn new(file: FilePath, span: Span, kind: FormKind) -> Self {
        Self { file, span, kind }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::LineColumn;

    fn make_span() -> Span {
        Span::try_new(LineColumn::new(1, 0), LineColumn::new(3, 12)).unwrap()
    }

    #[test]
    fn normalized_form_new_stores_all_fields() {
        let fps: HashSet<u64> = [1_u64, 2, 3, 4].into_iter().collect();
        let form = NormalizedForm::new(FormKind::Production, fps.clone(), make_span(), 17, 3);
        assert_eq!(form.kind, FormKind::Production);
        assert_eq!(form.fingerprint_set, fps);
        assert_eq!(form.node_count, 17);
        assert_eq!(form.line_count, 3);
    }

    #[test]
    fn normalized_form_with_empty_fingerprint_set_is_well_formed() {
        // The comparison engine will treat empty-set forms as
        // uninteresting (Jaccard against an empty set is 0.0), but
        // the domain type accepts them.
        let form = NormalizedForm::new(FormKind::Doctest, HashSet::new(), make_span(), 0, 0);
        assert!(form.fingerprint_set.is_empty());
    }

    #[test]
    fn normalized_form_serde_round_trips() {
        let fps: HashSet<u64> = [42_u64, 7, 99].into_iter().collect();
        let form = NormalizedForm::new(FormKind::Test, fps, make_span(), 8, 2);
        let json = serde_json::to_string(&form).unwrap();
        let back: NormalizedForm = serde_json::from_str(&json).unwrap();
        assert_eq!(back, form);
    }

    #[test]
    fn form_ref_new_stores_fields() {
        let file = FilePath::from(PathBuf::from("src/lib.rs"));
        let span = make_span();
        let r = FormRef::new(file.clone(), span, FormKind::Production);
        assert_eq!(r.file, file);
        assert_eq!(r.span, span);
        assert_eq!(r.kind, FormKind::Production);
    }

    #[test]
    fn form_ref_serde_round_trips() {
        let r = FormRef::new(
            FilePath::from(PathBuf::from("src/foo.rs")),
            make_span(),
            FormKind::Production,
        );
        let json = serde_json::to_string(&r).unwrap();
        let back: FormRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
