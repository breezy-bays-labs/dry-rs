//! Serde round-trip tests for every public domain type.
//!
//! These tests live outside the per-module `#[cfg(test)] mod tests`
//! blocks so they exercise the types through the **public** API
//! surface (the `dry_core::domain` re-exports) — the same path
//! external consumers and adapter binaries hit.
//!
//! The `wire_envelope_snapshot.rs` insta test that locks the full
//! nested envelope shape lands with PR 7 alongside the JSON reporter.
//! This file covers the narrower contract: every public domain type
//! round-trips through `serde_json` cleanly, and `Match` specifically
//! emits the three reserved score slots as explicit `null` rather
//! than omitting them.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use dry_core::domain::{
    FilePath, Fingerprint, FormKind, FormRef, LineColumn, Match, NormalizedForm, Report, Score,
    Severity, Span, Summary, Tier,
};

fn make_form_ref() -> FormRef {
    FormRef::new(
        FilePath::from(PathBuf::from("crates/dry-core/src/lib.rs")),
        Span::try_new(LineColumn::new(1, 0), LineColumn::new(3, 12)).unwrap(),
        FormKind::Production,
    )
}

#[test]
fn line_column_round_trips() {
    let original = LineColumn::new(7, 4);
    let json = serde_json::to_string(&original).unwrap();
    let back: LineColumn = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

#[test]
fn span_round_trips() {
    let original = Span::try_new(LineColumn::new(1, 0), LineColumn::new(3, 12)).unwrap();
    let json = serde_json::to_string(&original).unwrap();
    let back: Span = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

#[test]
fn score_round_trips() {
    let original = Score::try_new(0.92).unwrap();
    let json = serde_json::to_string(&original).unwrap();
    let back: Score = serde_json::from_str(&json).unwrap();
    assert!((back.value() - original.value()).abs() < f64::EPSILON);
}

#[test]
fn form_kind_round_trips_all_variants() {
    for kind in [FormKind::Production, FormKind::Test, FormKind::Doctest] {
        let json = serde_json::to_string(&kind).unwrap();
        let back: FormKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }
}

#[test]
fn tier_round_trips_all_variants() {
    for tier in [Tier::AutoRefactor, Tier::ReviewFirst, Tier::Advisory] {
        let json = serde_json::to_string(&tier).unwrap();
        let back: Tier = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tier);
    }
}

#[test]
fn severity_round_trips_all_variants() {
    for sev in [Severity::High, Severity::Medium, Severity::Low] {
        let json = serde_json::to_string(&sev).unwrap();
        let back: Severity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sev);
    }
}

#[test]
fn file_path_round_trips() {
    let original = FilePath::from(PathBuf::from("src/lib.rs"));
    let json = serde_json::to_string(&original).unwrap();
    let back: FilePath = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

#[test]
fn fingerprint_round_trips() {
    let original = Fingerprint::new(0xDEAD_BEEF);
    let json = serde_json::to_string(&original).unwrap();
    let back: Fingerprint = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

#[test]
fn normalized_form_round_trips() {
    let fps: HashSet<u64> = [1_u64, 2, 3].into_iter().collect();
    let original = NormalizedForm::new(
        FormKind::Production,
        fps,
        Span::try_new(LineColumn::new(1, 0), LineColumn::new(3, 12)).unwrap(),
        17,
        3,
    );
    let json = serde_json::to_string(&original).unwrap();
    let back: NormalizedForm = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

#[test]
fn form_ref_round_trips() {
    let original = make_form_ref();
    let json = serde_json::to_string(&original).unwrap();
    let back: FormRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

#[test]
fn match_round_trips_with_reserved_slots_null() {
    let original = Match::new(vec![make_form_ref()], 0.92, Tier::ReviewFirst);
    let json = serde_json::to_string(&original).unwrap();
    let back: Match = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

#[test]
fn match_round_trips_with_populated_scores() {
    let original = Match::with_scores(
        vec![make_form_ref()],
        0.95,
        Some(0.97),
        Some(2),
        Some(0.04),
        Tier::AutoRefactor,
    );
    let json = serde_json::to_string(&original).unwrap();
    let back: Match = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

// Load-bearing wire-contract assertion: a v0.1 `Match::new(..)` MUST
// emit the three reserved score fields as explicit `null` in the
// serialized JSON. Skipping them would contradict the locked shape
// declared in `adr-nested-json-envelope.md` "Note on serde
// attributes".
#[test]
fn match_v0_1_wire_shape_emits_three_explicit_nulls() {
    let m = Match::new(vec![], 0.92, Tier::ReviewFirst);
    let json = serde_json::to_string(&m).unwrap();
    assert!(
        json.contains("\"structural_score\":null"),
        "v0.1 wire shape must emit structural_score as null, got: {json}"
    );
    assert!(
        json.contains("\"rename_count\":null"),
        "v0.1 wire shape must emit rename_count as null, got: {json}"
    );
    assert!(
        json.contains("\"rename_density\":null"),
        "v0.1 wire shape must emit rename_density as null, got: {json}"
    );
    // And the always-populated fields appear verbatim.
    assert!(json.contains("\"score\":0.92"), "got: {json}");
    assert!(json.contains("\"tier\":\"review_first\""), "got: {json}");
}

#[test]
fn match_deserializes_envelope_missing_three_reserved_fields() {
    // Wire input from a future producer that omits the reserved
    // slots entirely (or a hand-written test input). `#[serde(default)]`
    // populates `None`. This is the backward-compat side of the
    // contract.
    let json = r#"{
        "forms": [],
        "score": 0.92,
        "tier": "review_first"
    }"#;
    let m: Match = serde_json::from_str(json).expect("missing reserved slots must deserialize");
    assert_eq!(m.structural_score, None);
    assert_eq!(m.rename_count, None);
    assert_eq!(m.rename_density, None);
}

#[test]
fn summary_round_trips() {
    let mut by_tier = BTreeMap::new();
    by_tier.insert(Tier::ReviewFirst, 3);
    let mut by_kind = BTreeMap::new();
    by_kind.insert(FormKind::Production, 7);
    let original = Summary {
        total_forms: 10,
        by_tier,
        by_kind,
    };
    let json = serde_json::to_string(&original).unwrap();
    let back: Summary = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

#[test]
fn report_round_trips_empty() {
    let original = Report::empty_passed();
    let json = serde_json::to_string(&original).unwrap();
    let back: Report = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

#[test]
fn report_round_trips_with_matches() {
    let m = Match::new(vec![make_form_ref()], 0.88, Tier::ReviewFirst);
    let original = Report::new(vec![m], Summary::new(), false);
    let json = serde_json::to_string(&original).unwrap();
    let back: Report = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}
