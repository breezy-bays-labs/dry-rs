//! Insta snapshot for the markdown reporter (dry-rs#91).
//!
//! Locks the exact GitHub-flavored Markdown emitted for a
//! representative `Report` covering all three routing tiers
//! (`auto_refactor` / `review_first` / `advisory`). The snapshot is
//! the executable contract — any drift (tier heading change, reordered
//! within-tier sort, different `file:line:col` rendering, fenced-block
//! shape) surfaces as a snap diff.
//!
//! Mirrors `github_annotations_snapshot.rs`: a multi-tier locked
//! snapshot plus the empty-report edge case. Per
//! `~/.claude/rules/exclusions.md`, the `.snap` file lives next to the
//! test under `crates/dry-core/tests/snapshots/`.

use std::path::PathBuf;

use dry_core::adapters::reporters::markdown::render;
use dry_core::domain::{
    FilePath, FormKind, FormRef, LineColumn, Match, Report, Span, Summary, Tier,
};

fn form_ref(path: &str, kind: FormKind, start_line: u32, end_line: u32) -> FormRef {
    FormRef::new(
        FilePath::from(PathBuf::from(path)),
        Span::try_new(
            LineColumn::new(start_line, 0),
            LineColumn::new(end_line, 12),
        )
        .unwrap(),
        kind,
    )
}

#[test]
fn markdown_all_three_tiers_locked() {
    // Two matches in the same tier exercise the within-tier
    // score-DESC ordering; mixed `FormKind` exercises the kind label.
    let auto_match = Match::new(
        vec![
            form_ref("src/auto_a.rs", FormKind::Production, 10, 18),
            form_ref("src/auto_b.rs", FormKind::Production, 100, 108),
        ],
        0.97,
        Tier::AutoRefactor,
    );
    let review_high = Match::new(
        vec![
            form_ref("src/review_a.rs", FormKind::Production, 20, 25),
            form_ref("src/review_b.rs", FormKind::Production, 200, 205),
        ],
        0.93,
        Tier::ReviewFirst,
    );
    let review_low = Match::new(
        vec![
            form_ref("tests/dup_a.rs", FormKind::Test, 5, 9),
            form_ref("tests/dup_b.rs", FormKind::Test, 50, 54),
        ],
        0.88,
        Tier::ReviewFirst,
    );
    let advisory_match = Match::new(
        vec![
            form_ref("src/adv_a.rs", FormKind::Doctest, 30, 33),
            form_ref("src/adv_b.rs", FormKind::Doctest, 300, 303),
        ],
        0.83,
        Tier::Advisory,
    );

    // Intentionally out of canonical order — the reporter must group
    // and sort deterministically regardless of input ordering.
    let report = Report::new(
        vec![advisory_match, review_low, auto_match, review_high],
        Summary::new(),
        false,
    );
    let out = render(&report);
    insta::assert_snapshot!(out);
}

#[test]
fn markdown_empty_report_locked() {
    let out = render(&Report::empty_passed());
    insta::assert_snapshot!(out);
}
