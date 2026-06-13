//! v0.1 wire-envelope snapshot — the mechanical schema document.
//!
//! Per `ops/decisions/dry-rs/adr-nested-json-envelope.md` §"Wire-shape
//! lock — insta snapshot", this file is the executable form of the
//! schema. The fixture builds a representative [`Report`] (one
//! [`Match`] with one populated `score` and three explicit `null`
//! reserved slots), serializes it via
//! `dry_core::adapters::reporters::json::render`, and asserts the
//! pretty-printed JSON matches the committed `.snap`.
//!
//! Any drift fails CI:
//!
//! - Adding a field anywhere in the envelope → the snap diff surfaces;
//!   `cargo insta review` accepts after a deliberate (and ADR-justified)
//!   change. Additive optional fields do NOT bump `schema_version`.
//! - Renaming a serde key, reordering struct fields, changing type
//!   → snap diff + `schema_version` MUST bump per the ADR forward-compat
//!   table.
//! - Accidentally populating a reserved score slot at v0.1 → the
//!   serialized value (`Some(0.0)` becomes `0.0` not `null`) shows the
//!   regression.
//!
//! Determinism: the test passes a fixed `EnvelopeMeta::timestamp`
//! (`"2026-05-24T22:00:00Z"`) so the snapshot is byte-stable across
//! runs and platforms. The reporter never calls `SystemTime::now()`;
//! that would defeat the lock.

use std::collections::BTreeMap;
use std::path::PathBuf;

use dry_core::adapters::reporters::json::{
    EnvelopeMeta, LANGUAGE_RUST, THRESHOLD_MODE_DEFAULT, TOOL_NAME_DRY4RS, render,
};
use dry_core::domain::{
    FilePath, FormKind, FormRef, LineColumn, Match, Report, Span, Summary, Tier,
};

fn make_fixture_report() -> Report {
    // Two forms in one match: locks `forms: Vec<FormRef>` ordering.
    let forms = vec![
        FormRef::new(
            FilePath::from(PathBuf::from("src/a.rs")),
            Span::try_new(LineColumn::new(10, 0), LineColumn::new(15, 12)).unwrap(),
            FormKind::Production,
        ),
        FormRef::new(
            FilePath::from(PathBuf::from("src/b.rs")),
            Span::try_new(LineColumn::new(40, 0), LineColumn::new(45, 12)).unwrap(),
            FormKind::Production,
        ),
    ];
    // `Match::new` populates only `score`; the three reserved slots
    // (structural_score, rename_count, rename_density) emit as
    // explicit `null` per the locked v0.1 wire shape.
    let m = Match::new(forms, 0.92, Tier::ReviewFirst);

    // Locks the deterministic BTreeMap ordering for `by_tier` and
    // `by_kind` (per the Summary module docs — ordering is
    // declaration-order on the enums).
    let mut by_tier = BTreeMap::new();
    by_tier.insert(Tier::ReviewFirst, 1);
    let mut by_kind = BTreeMap::new();
    by_kind.insert(FormKind::Production, 1);

    let summary = Summary {
        total_forms: 2,
        by_tier,
        by_kind,
    };

    Report::new(vec![m], summary, false)
}

fn fixed_meta() -> EnvelopeMeta {
    EnvelopeMeta::new(
        TOOL_NAME_DRY4RS.into(),
        "0.1.0".into(),
        LANGUAGE_RUST.into(),
        // Fixed timestamp keeps the snapshot deterministic. The
        // adapter binary's run-loop wrapper supplies a real UTC
        // timestamp; tests supply this constant.
        "2026-05-24T22:00:00Z".into(),
        THRESHOLD_MODE_DEFAULT.into(),
    )
}

#[test]
fn wire_envelope_v0_1_locked_shape() {
    let report = make_fixture_report();
    let json = render(&report, fixed_meta()).expect("envelope must serialize");
    insta::assert_snapshot!(json);
}

#[test]
fn wire_envelope_omits_view_delta_diagnostics_when_unused() {
    // At v0.1 the run loop never populates `view`, `delta`, or
    // `diagnostics`. They are reserved for v0.3+ (view filters land
    // with CLI in PR 8; delta + diagnostics per the roadmap). The
    // wire shape MUST omit them entirely (skip_serializing_if), not
    // emit them as `null` — that is the contract the ADR's
    // forward-compat table relies on.
    //
    // The same omission contract holds for the later additive fields
    // (`title` / `subtitle`, dry-rs#78; `scope`, dry-rs#124; `mode` /
    // `capabilities`, dry-rs#147): when the library-facing `Envelope::new`
    // constructor leaves every additive field `None`, the serialized object
    // is byte-identical to the v0.1 snapshot. This is the explicit
    // all-additive-fields-None case the build plan calls for — the run loop
    // populates `scope` (and the HTML reporter `mode` + `capabilities`), but
    // the constructor path (used by reporter unit tests + library callers)
    // omits them all.
    let json = render(&Report::empty_passed(), fixed_meta()).unwrap();
    assert!(
        !json.contains("\"view\""),
        "view must be absent at v0.1, got: {json}"
    );
    assert!(
        !json.contains("\"delta\""),
        "delta must be absent at v0.1, got: {json}"
    );
    assert!(
        !json.contains("\"diagnostics\""),
        "diagnostics must be absent at v0.1, got: {json}"
    );
    assert!(
        !json.contains("\"title\""),
        "title must be omitted when None, got: {json}"
    );
    assert!(
        !json.contains("\"subtitle\""),
        "subtitle must be omitted when None, got: {json}"
    );
    assert!(
        !json.contains("\"scope\""),
        "scope must be omitted when None (v0.1 byte-identical-when-off), got: {json}"
    );
    assert!(
        !json.contains("\"mode\""),
        "mode must be omitted when None (v0.1 byte-identical-when-off), got: {json}"
    );
    assert!(
        !json.contains("\"capabilities\""),
        "capabilities must be omitted when None (v0.1 byte-identical-when-off), got: {json}"
    );
}

#[test]
fn wire_envelope_schema_version_is_locked_at_one() {
    // The ADR pins `schema_version: 1` for the initial published
    // schema. Any breaking change (rename, removal, type change)
    // bumps this; additive changes do NOT. Reading the integer out
    // of the envelope verifies the constant is wired correctly.
    let json = render(&Report::empty_passed(), fixed_meta()).unwrap();
    assert!(
        json.contains("\"schema_version\": 1"),
        "schema_version must be 1 at v0.1, got: {json}"
    );
}
