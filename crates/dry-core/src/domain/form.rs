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

/// Structural address of a form within the source tree — which crate /
/// package it lives in and the module path leading to it.
///
/// `StructuralLocation` is an *additive* enrichment on
/// [`NormalizedForm`]: it carries no weight in Jaccard similarity (the
/// comparison engine never reads it) and exists purely so reporters and
/// downstream consumers can group / scope matches by structural origin
/// (e.g. "all duplication inside `crate::foo::bar`"). It is a result
/// struct, so — per the `#[non_exhaustive]` discipline in the
/// nested-envelope ADR — it does NOT carry `#[non_exhaustive]`; it
/// evolves via constructors and serde versioning.
///
/// Both fields are language-agnostic:
///
/// - `crate_id` — the crate (Rust) or package (TS) the form belongs to.
///   `None` when the adapter has no notion of a crate boundary for this
///   form, or has not yet populated it.
/// - `module_path` — path components of the enclosing module
///   (`["foo", "bar"]`), rendered by reporters with the per-language
///   separator. Empty when the form sits at the crate root or the
///   adapter does not populate it.
///
/// The default value (no crate, empty module path) is the "unknown
/// location" sentinel. [`StructuralLocation::is_default`] powers the
/// `skip_serializing_if` on [`NormalizedForm::location`] so a form with
/// no known location omits the `location` key entirely — keeping the
/// v0.1 wire snapshot byte-identical for adapters that do not yet emit
/// structural locations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralLocation {
    /// Crate (Rust) or package (TS) the form belongs to; `None` when
    /// the adapter has no crate notion for this form or has not
    /// populated it. Omitted from the wire when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crate_id: Option<String>,
    /// Path components of the enclosing module (`["foo", "bar"]`), not a
    /// separator-joined string — reporters render with `::` (Rust) or
    /// `.` (TS). Omitted from the wire when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub module_path: Vec<String>,
}

impl StructuralLocation {
    /// True when this is the "unknown location" sentinel — no crate and
    /// an empty module path. Used by the `skip_serializing_if` on
    /// [`NormalizedForm::location`] to omit the `location` key from the
    /// wire when the form carries no structural address.
    ///
    /// Not `const fn`: `Vec::is_empty` only became const-stable in Rust
    /// 1.87, but the workspace MSRV is pinned at 1.85.
    #[must_use]
    #[inline]
    pub fn is_default(&self) -> bool {
        self.crate_id.is_none() && self.module_path.is_empty()
    }
}

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
/// - `location` — additive [`StructuralLocation`] (crate + module path)
///   for reporter-side grouping / scoping. Never read by the comparison
///   engine; omitted from the wire when default ("unknown location").
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
    /// Structural address (crate + module path) of the form.
    ///
    /// Additive enrichment populated by adapters that can resolve a
    /// form's structural origin; defaults to the "unknown location"
    /// sentinel ([`StructuralLocation::default`]). The comparison
    /// engine never reads it. `skip_serializing_if` omits the
    /// `location` key from the wire when default, so envelopes from
    /// adapters that do not populate it stay byte-identical to the v0.1
    /// snapshot. Declared LAST so adding it leaves the prefix of the
    /// serialized object unchanged.
    #[serde(default, skip_serializing_if = "StructuralLocation::is_default")]
    pub location: StructuralLocation,
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
            location: StructuralLocation::default(),
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
            location: StructuralLocation::default(),
        }
    }

    /// Attach a [`StructuralLocation`] to this form, returning the
    /// updated value (builder-style).
    ///
    /// Adapters that resolve a form's structural origin call this after
    /// [`NormalizedForm::new`] / [`NormalizedForm::with_context`]. The
    /// location is additive — it never affects Jaccard similarity — so
    /// the comparison engine behaves identically whether or not it is
    /// set.
    #[must_use]
    #[inline]
    pub fn with_location(mut self, location: StructuralLocation) -> Self {
        self.location = location;
        self
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
    use crate::test_support::make_span;

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
        // PR 8 (#122): `location` defaults to the unknown-location sentinel.
        assert!(form.location.is_default());
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

    // ---- StructuralLocation (PR 8 / #122) ----

    fn populated_location() -> StructuralLocation {
        StructuralLocation {
            crate_id: Some("dry_core".to_string()),
            module_path: vec!["domain".to_string(), "form".to_string()],
        }
    }

    #[test]
    fn structural_location_default_is_unknown_sentinel() {
        let loc = StructuralLocation::default();
        assert_eq!(loc.crate_id, None);
        assert!(loc.module_path.is_empty());
        assert!(loc.is_default());
    }

    #[test]
    fn structural_location_is_default_truth_table() {
        // Default → true.
        assert!(StructuralLocation::default().is_default());
        // crate_id set → false.
        let crate_only = StructuralLocation {
            crate_id: Some("dry_core".to_string()),
            module_path: Vec::new(),
        };
        assert!(!crate_only.is_default());
        // module_path non-empty → false.
        let module_only = StructuralLocation {
            crate_id: None,
            module_path: vec!["domain".to_string()],
        };
        assert!(!module_only.is_default());
        // Both set → false.
        assert!(!populated_location().is_default());
    }

    #[test]
    fn structural_location_serde_round_trips_default() {
        let loc = StructuralLocation::default();
        let json = serde_json::to_string(&loc).unwrap();
        // Both fields omitted when default → empty JSON object.
        assert_eq!(json, "{}");
        let back: StructuralLocation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, loc);
    }

    #[test]
    fn structural_location_serde_round_trips_populated() {
        let loc = populated_location();
        let json = serde_json::to_string(&loc).unwrap();
        assert!(json.contains("\"crate_id\":\"dry_core\""));
        assert!(json.contains("\"module_path\":[\"domain\",\"form\"]"));
        let back: StructuralLocation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, loc);
    }

    #[test]
    fn structural_location_omits_empty_crate_and_module_individually() {
        // crate_id Some + module_path empty → only crate_id on the wire.
        let crate_only = StructuralLocation {
            crate_id: Some("dry_core".to_string()),
            module_path: Vec::new(),
        };
        let json = serde_json::to_string(&crate_only).unwrap();
        assert!(json.contains("crate_id"));
        assert!(!json.contains("module_path"));
        assert_eq!(
            serde_json::from_str::<StructuralLocation>(&json).unwrap(),
            crate_only,
            "crate-only location must round-trip"
        );
        // crate_id None + module_path non-empty → only module_path.
        let module_only = StructuralLocation {
            crate_id: None,
            module_path: vec!["domain".to_string()],
        };
        let json = serde_json::to_string(&module_only).unwrap();
        assert!(!json.contains("crate_id"));
        assert!(json.contains("module_path"));
        assert_eq!(
            serde_json::from_str::<StructuralLocation>(&json).unwrap(),
            module_only,
            "module-only location must round-trip"
        );
    }

    // ---- NormalizedForm.location wire gate (PR 8 / #122) ----

    #[test]
    fn normalized_form_with_default_location_omits_location_key() {
        // WIRE GATE: a form with the default location serializes WITHOUT
        // a `location` key (skip_serializing_if), keeping the v0.1
        // snapshot byte-identical for adapters that do not emit one.
        let form = NormalizedForm::new(FormKind::Production, HashSet::new(), make_span(), 4, 2);
        assert!(form.location.is_default());
        let json = serde_json::to_string(&form).unwrap();
        assert!(
            !json.contains("location"),
            "default location must be omitted from the wire, got: {json}"
        );
        // Round-trips back to the same value.
        let back: NormalizedForm = serde_json::from_str(&json).unwrap();
        assert_eq!(back, form);
    }

    #[test]
    fn normalized_form_with_location_serializes_the_field() {
        let form = NormalizedForm::new(FormKind::Production, HashSet::new(), make_span(), 4, 2)
            .with_location(populated_location());
        assert_eq!(form.location, populated_location());
        let json = serde_json::to_string(&form).unwrap();
        assert!(
            json.contains("location"),
            "populated location must serialize, got: {json}"
        );
        assert!(json.contains("\"crate_id\":\"dry_core\""));
        let back: NormalizedForm = serde_json::from_str(&json).unwrap();
        assert_eq!(back, form);
    }

    #[test]
    fn normalized_form_deserializes_envelope_missing_location_key() {
        // Backward-compat: a v0.1 envelope with no `location` key
        // deserializes with the default (unknown) location.
        let json = r#"{
            "kind": "production",
            "fingerprint_set": [1, 2, 3],
            "span": {"start": {"line": 1, "column": 0}, "end": {"line": 2, "column": 5}},
            "node_count": 4,
            "line_count": 2
        }"#;
        let form: NormalizedForm = serde_json::from_str(json)
            .expect("envelope missing location must deserialize to default");
        assert!(form.location.is_default());
    }

    #[test]
    fn normalized_form_with_location_is_additive_to_other_fields() {
        // with_location preserves every other field — it only swaps in
        // the structural location.
        let fps: HashSet<u64> = [1_u64, 2, 3].into_iter().collect();
        let base = NormalizedForm::with_context(
            FormKind::Test,
            fps.clone(),
            vec!["a".to_string()],
            vec!["mod_a".to_string()],
            make_span(),
            9,
            4,
        );
        let located = base.clone().with_location(populated_location());
        assert_eq!(located.kind, base.kind);
        assert_eq!(located.fingerprint_set, base.fingerprint_set);
        assert_eq!(located.identifier_set, base.identifier_set);
        assert_eq!(located.qualified_name, base.qualified_name);
        assert_eq!(located.span, base.span);
        assert_eq!(located.node_count, base.node_count);
        assert_eq!(located.line_count, base.line_count);
        assert_eq!(located.location, populated_location());
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
