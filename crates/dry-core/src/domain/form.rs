//! Cross-language form representation — [`NormalizedForm`] and
//! [`FormRef`].
//!
//! `NormalizedForm` is the language-agnostic IR the comparison engine
//! operates on. Each adapter's [`crate::ports::NormalizerPort::normalize`]
//! emits a stream of `NormalizedForm` values; the comparison engine
//! clusters them by `fingerprint_set` and computes Jaccard similarity
//! over the sets.
//!
//! Per-language `FormContext` extensions (`visibility`, `attributes`,
//! `parent_module`, `parent_impl`) are deferred per roadmap — they are
//! heavily Rust-centric and would skew the schema if landed before
//! dry4ts validates cross-language abstraction.
//!
//! `node_count` semantics across languages are pinned by the O8 ADR
//! (`ops/decisions/dry-rs/adr-normalized-form-schema.md`) — count is
//! over the leaf nodes that map to fingerprints after typed-placeholder
//! substitution, not raw punctuation or pure structural wrappers.
//! The `identifier_set` and `qualified_name` fields land with PR 4 per
//! the same ADR; v0.1 leaves them populated by adapters but unused by
//! the comparison engine until rename-signal scoring lands at v0.2+.

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
/// - `identifier_set` — original identifiers paralleling the
///   fingerprint stream. Populated by adapters; unused by the
///   comparison engine at v0.1, consumed by rename-signal scoring at
///   v0.2+. Wire shape: `Vec<String>` — see `adr-normalized-form-schema.md`.
/// - `qualified_name` — path components (`["foo", "bar", "baz"]`), not
///   a joined string. Reporters render with the per-language separator
///   (`::` for Rust, `.` for TS).
/// - `span` — source range, end-inclusive.
/// - `node_count` — count of normalized-AST nodes that map to
///   fingerprints (after typed-placeholder substitution). Drives the
///   sliding-window break: Jaccard upper bound `min/max >= t`
///   ⟹ `max <= min/t`. Heuristic pinned by the O8 ADR — all adapters
///   honor the same counting rule so threshold semantics stay unified
///   across languages.
/// - `line_count` — source-line span (end-line minus start-line, plus
///   one); used by reporters and the size-bucketed parallelism in v0.5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedForm {
    /// What category of form this is (production, test, doctest).
    pub kind: FormKind,
    /// Hashed subform fingerprints; Jaccard similarity is computed
    /// over the intersection of two forms' sets.
    pub fingerprint_set: HashSet<u64>,
    /// Original identifiers paralleling the fingerprint stream (O11).
    ///
    /// Populated by adapters; unused by the comparison engine at v0.1.
    /// At v0.2+ the rename-signal path consumes this to compute
    /// `rename_count` / `rename_density`. `Vec<String>` (not
    /// `HashSet`) so adapters preserve emission order and duplicates;
    /// the v0.2+ scoring path converts to a set at the comparison
    /// boundary. Defaults to an empty `Vec` for serde backward-compat
    /// with envelopes that omit the field.
    #[serde(default)]
    pub identifier_set: Vec<String>,
    /// Path components of the form's qualified name (O8).
    ///
    /// `["foo", "bar", "baz"]` rather than `"foo::bar::baz"` or
    /// `"foo.bar.baz"`: reporters render with the per-language
    /// separator. Defaults to an empty `Vec` for serde backward-compat.
    #[serde(default)]
    pub qualified_name: Vec<String>,
    /// Source range of the original form, end-inclusive.
    pub span: Span,
    /// Count of normalized-AST nodes mapped to fingerprints
    /// (post-substitution). Load-bearing for the sliding-window break
    /// math; the heuristic is the same across every adapter (see O8
    /// ADR).
    pub node_count: u32,
    /// Source line count covered by the form.
    pub line_count: u32,
}

impl NormalizedForm {
    /// Construct a [`NormalizedForm`] from its component parts.
    ///
    /// `identifier_set` and `qualified_name` default to empty `Vec`s —
    /// PR 5's syn adapter and downstream call sites that don't yet
    /// emit identifiers or qualified-name paths can use this
    /// constructor unchanged. Use [`NormalizedForm::with_context`]
    /// when populating the rename-signal fields.
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
            identifier_set: Vec::new(),
            qualified_name: Vec::new(),
            span,
            node_count,
            line_count,
        }
    }

    /// Construct a [`NormalizedForm`] with all O8/O11 context fields
    /// supplied — `identifier_set` (the parallel-to-fingerprints
    /// identifier stream consumed by v0.2+ rename signal) and
    /// `qualified_name` (path components for reporter-side rendering).
    #[must_use]
    pub fn with_context(
        kind: FormKind,
        fingerprint_set: HashSet<u64>,
        identifier_set: Vec<String>,
        qualified_name: Vec<String>,
        span: Span,
        node_count: u32,
        line_count: u32,
    ) -> Self {
        Self {
            kind,
            fingerprint_set,
            identifier_set,
            qualified_name,
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
        // PR 4 (#5): O8/O11 context fields default to empty.
        assert!(form.identifier_set.is_empty());
        assert!(form.qualified_name.is_empty());
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
    fn normalized_form_with_context_stores_all_fields() {
        let fps: HashSet<u64> = [1_u64, 2, 3].into_iter().collect();
        let identifiers = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        let qname = vec![
            "my_module".to_string(),
            "MyType".to_string(),
            "foo".to_string(),
        ];
        let form = NormalizedForm::with_context(
            FormKind::Production,
            fps.clone(),
            identifiers.clone(),
            qname.clone(),
            make_span(),
            17,
            3,
        );
        assert_eq!(form.fingerprint_set, fps);
        assert_eq!(form.identifier_set, identifiers);
        assert_eq!(form.qualified_name, qname);
    }

    #[test]
    fn normalized_form_identifier_set_preserves_emission_order_and_duplicates() {
        // O11 contract: `Vec<String>` (not `HashSet`). Adapters emit
        // in token order; v0.2+ scoring converts to a set at the
        // boundary. Duplicates round-trip through serde.
        let identifiers = vec!["x".to_string(), "y".to_string(), "x".to_string()];
        let form = NormalizedForm::with_context(
            FormKind::Production,
            HashSet::new(),
            identifiers.clone(),
            Vec::new(),
            make_span(),
            0,
            0,
        );
        assert_eq!(form.identifier_set, identifiers);
        // The duplicate is preserved in slot 2.
        assert_eq!(form.identifier_set[0], form.identifier_set[2]);
    }

    #[test]
    fn normalized_form_qualified_name_is_path_components_not_joined() {
        // O8 decision: components, not a separator-joined string.
        // Reporters render with `::` (Rust) or `.` (TS) at display time.
        let form = NormalizedForm::with_context(
            FormKind::Test,
            HashSet::new(),
            Vec::new(),
            vec!["my_crate".into(), "tests".into(), "the_test_fn".into()],
            make_span(),
            0,
            0,
        );
        assert_eq!(form.qualified_name.len(), 3);
        assert_eq!(form.qualified_name[0], "my_crate");
        assert_eq!(form.qualified_name[2], "the_test_fn");
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
    fn normalized_form_serde_round_trips_with_context_populated() {
        // Forward-compat: an envelope with identifier_set + qualified_name
        // populated round-trips identically.
        let fps: HashSet<u64> = [1_u64, 2].into_iter().collect();
        let form = NormalizedForm::with_context(
            FormKind::Test,
            fps,
            vec!["a".into(), "b".into()],
            vec!["mod_a".into(), "fn_b".into()],
            make_span(),
            5,
            1,
        );
        let json = serde_json::to_string(&form).unwrap();
        let back: NormalizedForm = serde_json::from_str(&json).unwrap();
        assert_eq!(back, form);
    }

    #[test]
    fn normalized_form_deserializes_envelope_missing_context_fields() {
        // Backward-compat: a wire envelope omitting identifier_set
        // and qualified_name (e.g. a v0.1 producer that doesn't yet
        // populate them) must deserialize with both as empty Vecs.
        // `#[serde(default)]` powers this path.
        let json = r#"{
            "kind": "production",
            "fingerprint_set": [1, 2, 3],
            "span": {"start": {"line": 1, "column": 0}, "end": {"line": 2, "column": 5}},
            "node_count": 4,
            "line_count": 2
        }"#;
        let form: NormalizedForm = serde_json::from_str(json)
            .expect("envelope missing identifier_set and qualified_name must deserialize");
        assert!(form.identifier_set.is_empty());
        assert!(form.qualified_name.is_empty());
        assert_eq!(form.node_count, 4);
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
