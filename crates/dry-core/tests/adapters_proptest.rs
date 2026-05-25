//! Property tests for the language-agnostic adapters
//! (`adapters::source::enumerate`, `adapters::reporters::json::render`).
//!
//! The unit tests in each module cover representative scenarios; the
//! properties here cover broader input spaces and lock the contracts
//! that downstream PRs (PR 8 CLI surface, PR 9 self-check) and the
//! `wire_envelope_snapshot` mechanical lock rely on.

use std::collections::BTreeMap;
use std::path::PathBuf;

use dry_core::adapters::reporters::github_annotations;
use dry_core::adapters::reporters::json::{
    EnvelopeMeta, LANGUAGE_RUST, THRESHOLD_MODE_DEFAULT, TOOL_NAME_DRY4RS, render as render_json,
};
use dry_core::adapters::reporters::text;
use dry_core::adapters::source::enumerate;
use dry_core::cli::AnalysisConfig;
use dry_core::domain::{
    FilePath, FormKind, FormRef, LineColumn, Match, Report, Span, Summary, Tier,
};
use proptest::prelude::*;
use tempfile::TempDir;

fn arb_tier() -> impl Strategy<Value = Tier> {
    prop_oneof![
        Just(Tier::AutoRefactor),
        Just(Tier::ReviewFirst),
        Just(Tier::Advisory),
    ]
}

fn arb_form_kind() -> impl Strategy<Value = FormKind> {
    prop_oneof![
        Just(FormKind::Production),
        Just(FormKind::Test),
        Just(FormKind::Doctest),
    ]
}

fn arb_span() -> impl Strategy<Value = Span> {
    (1u32..=1_000, 0u32..=100, 0u32..=10_000, 0u32..=100).prop_map(
        |(start_line, start_col, line_delta, end_col)| {
            let end_line = start_line.saturating_add(line_delta);
            // When line_delta == 0, force end_col >= start_col.
            let end_col = if end_line == start_line {
                end_col.max(start_col)
            } else {
                end_col
            };
            Span::try_new(
                LineColumn::new(start_line, start_col),
                LineColumn::new(end_line, end_col),
            )
            .expect("constructed start <= end")
        },
    )
}

fn arb_form_ref() -> impl Strategy<Value = FormRef> {
    (
        prop::string::string_regex("[a-z][a-z0-9]{0,8}\\.rs").unwrap(),
        arb_span(),
        arb_form_kind(),
    )
        .prop_map(|(name, span, kind)| {
            FormRef::new(
                FilePath::from(PathBuf::from(format!("src/{name}"))),
                span,
                kind,
            )
        })
}

fn arb_match() -> impl Strategy<Value = Match> {
    (
        prop::collection::vec(arb_form_ref(), 1..=4),
        0.0f64..=1.0f64,
        arb_tier(),
    )
        .prop_map(|(forms, score, tier)| Match::new(forms, score, tier))
}

fn arb_report() -> impl Strategy<Value = Report> {
    prop::collection::vec(arb_match(), 0..=8).prop_map(|matches| {
        let mut by_tier = BTreeMap::new();
        let mut by_kind = BTreeMap::new();
        for m in &matches {
            *by_tier.entry(m.tier).or_insert(0u32) += 1;
            if let Some(f) = m.forms.first() {
                *by_kind.entry(f.kind).or_insert(0u32) += 1;
            }
        }
        let passed = matches.is_empty();
        let summary = Summary {
            total_forms: u32::try_from(matches.len() * 2).unwrap_or(u32::MAX),
            by_tier,
            by_kind,
        };
        Report::new(matches, summary, passed)
    })
}

fn fixed_meta() -> EnvelopeMeta {
    EnvelopeMeta::new(
        TOOL_NAME_DRY4RS.into(),
        "0.1.0".into(),
        LANGUAGE_RUST.into(),
        "2026-05-24T22:00:00Z".into(),
        THRESHOLD_MODE_DEFAULT.into(),
    )
}

proptest! {
    // Property: `json::render` never panics on any well-formed
    // Report. The envelope is constructed by `Envelope::new` and
    // serialized via `serde_json::to_string_pretty`; every domain type
    // derives Serialize over owned data, so `Ok(_)` is the only
    // outcome we ever see in practice. The property locks that no
    // future Report shape (e.g. one with NaN floats sneaking in via
    // a relaxed score validator) flips this contract silently.
    #[test]
    fn json_render_is_total_over_well_formed_reports(report in arb_report()) {
        let json = render_json(&report, fixed_meta()).expect("render must succeed");
        // The envelope's locked top-level keys appear regardless of
        // payload (the structural lock the wire-envelope snapshot also
        // tests, generalized across the input space).
        prop_assert!(json.contains("\"schema_version\": 1"));
        prop_assert!(json.contains("\"tool\": \"dry4rs\""));
        prop_assert!(json.contains("\"result\":"));
    }

    // Property: a `Match` with reserved score slots all `None`
    // (the v0.1 emit shape) always serializes the three reserved
    // fields as `null` — never omitted, never as `0.0` or `0`. This
    // is the load-bearing wire contract from
    // `adr-nested-json-envelope.md` "Note on serde attributes".
    #[test]
    fn match_with_v0_1_reserved_slots_emits_three_explicit_nulls(
        forms in prop::collection::vec(arb_form_ref(), 1..=4),
        score in 0.0f64..=1.0f64,
        tier in arb_tier(),
    ) {
        let m = Match::new(forms, score, tier);
        let report = Report::new(vec![m], Summary::new(), false);
        let json = render_json(&report, fixed_meta()).unwrap();
        prop_assert!(json.contains("\"structural_score\": null"), "json: {}", json);
        prop_assert!(json.contains("\"rename_count\": null"), "json: {}", json);
        prop_assert!(json.contains("\"rename_density\": null"), "json: {}", json);
    }

    // Property: text reporter never panics and never emits ANSI
    // escape codes (ESC byte 0x1B). Color belongs to the CLI layer
    // (PR 8); the reporter must stay ANSI-clean for `pbcopy`,
    // GitHub PR comments, and other ANSI-stripping consumers.
    #[test]
    fn text_render_never_emits_ansi(report in arb_report()) {
        let out = text::render(&report);
        prop_assert!(!out.bytes().any(|b| b == 0x1B), "ANSI byte in output: {:?}", out);
    }

    // Property: github-annotations reporter emits exactly one
    // workflow-command line per Match (zero matches => empty string).
    // Each line starts with one of `::error`, `::warning`, `::notice`
    // (the three locked tier mappings) and carries the five required
    // property keys.
    #[test]
    fn github_annotations_one_line_per_match(report in arb_report()) {
        let out = github_annotations::render(&report);
        if report.matches.is_empty() {
            prop_assert_eq!(out, "");
        } else {
            let lines: Vec<&str> = out.lines().collect();
            prop_assert_eq!(lines.len(), report.matches.len());
            for line in &lines {
                prop_assert!(
                    line.starts_with("::error ") ||
                    line.starts_with("::warning ") ||
                    line.starts_with("::notice "),
                    "unexpected severity prefix: {}", line
                );
                for key in ["file=", "line=", "col=", "endLine=", "endColumn="] {
                    prop_assert!(line.contains(key), "missing {} in: {}", key, line);
                }
            }
        }
    }
}

// `enumerate` determinism over a single fixture corpus — two
// back-to-back calls must produce identical Vec<FilePath>. The unit
// test in `adapters::source::tests` covers a small corpus; here we
// generate up to 16 file names from a deterministic seed via
// `proptest::strategy::Strategy` to broaden coverage.
proptest! {
    #[test]
    fn enumerate_is_deterministic_across_runs(
        names in prop::collection::hash_set(
            prop::string::string_regex("[a-z][a-z0-9_]{0,6}\\.rs").unwrap(),
            1..=16,
        )
    ) {
        let dir = TempDir::new().unwrap();
        for name in &names {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        let config = AnalysisConfig::new([dir.path().to_path_buf()])
            .with_extensions(["rs"]);
        let first = enumerate(&config).unwrap();
        let second = enumerate(&config).unwrap();
        prop_assert_eq!(first.files, second.files);
    }
}

// Non-proptest deterministic check: empty Report deserialization
// path is exercised by the JSON reporter. Property tests above lock
// the success path; this one locks the no-allocation/no-domain-state
// roundtrip — empty Report still produces a valid envelope.
#[test]
fn json_render_succeeds_on_empty_report() {
    let json = render_json(&Report::empty_passed(), fixed_meta()).unwrap();
    assert!(json.contains("\"matches\": []"));
}
