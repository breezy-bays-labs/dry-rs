//! Insta snapshot for the GitHub Actions annotations reporter.
//!
//! Locks the exact workflow-command output emitted for a representative
//! `Report` covering all three tier mappings (`auto_refactor` →
//! `::error::`, `review_first` → `::warning::`, `advisory` →
//! `::notice::`). The snapshot is the executable contract — any drift
//! (different severity, different property-key ordering, different
//! escape semantics on a `file=` path) surfaces as a snap diff.
//!
//! Per `~/.claude/rules/exclusions.md`, the snapshot file lives next
//! to the test under `crates/dry-core/tests/snapshots/`.

use std::path::PathBuf;

use dry_core::adapters::reporters::github_annotations::render;
use dry_core::domain::{
    FilePath, FormKind, FormRef, LineColumn, Match, Report, Span, Summary, Tier,
};

fn form_ref(path: &str, start_line: u32, end_line: u32) -> FormRef {
    FormRef::new(
        FilePath::from(PathBuf::from(path)),
        Span::try_new(
            LineColumn::new(start_line, 0),
            LineColumn::new(end_line, 12),
        )
        .unwrap(),
        FormKind::Production,
    )
}

#[test]
fn github_annotations_all_three_tiers_locked() {
    let auto_match = Match::new(
        vec![
            form_ref("src/auto_a.rs", 10, 12),
            form_ref("src/auto_b.rs", 100, 105),
        ],
        0.97,
        Tier::AutoRefactor,
    );
    let review_match = Match::new(
        vec![
            form_ref("src/review_a.rs", 20, 25),
            form_ref("src/review_b.rs", 200, 210),
        ],
        0.91,
        Tier::ReviewFirst,
    );
    let advisory_match = Match::new(
        vec![
            form_ref("src/adv_a.rs", 30, 33),
            form_ref("src/adv_b.rs", 300, 305),
        ],
        0.83,
        Tier::Advisory,
    );

    let report = Report::new(
        vec![auto_match, review_match, advisory_match],
        Summary::new(),
        false,
    );
    let out = render(&report);
    insta::assert_snapshot!(out);
}

#[test]
fn github_annotations_empty_report_produces_no_output() {
    assert_eq!(render(&Report::empty_passed()), "");
}

#[test]
fn github_annotations_property_value_escapes_locked() {
    // Lock the escape semantics: `%`, `:`, `,` in `file=` get
    // percent-encoded. A regression here (e.g. only escaping `%`)
    // surfaces as a snap diff.
    let pathological = FormRef::new(
        FilePath::from(PathBuf::from("src/100%/foo:bar,baz.rs")),
        Span::try_new(LineColumn::new(1, 0), LineColumn::new(2, 0)).unwrap(),
        FormKind::Production,
    );
    let partner = form_ref("src/partner.rs", 5, 7);
    let m = Match::new(vec![pathological, partner], 0.95, Tier::AutoRefactor);
    let report = Report::new(vec![m], Summary::new(), false);
    let out = render(&report);
    insta::assert_snapshot!(out);
}
