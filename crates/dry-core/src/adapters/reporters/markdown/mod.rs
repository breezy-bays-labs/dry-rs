//! Markdown reporter — a rich GitHub-flavored "sticky card".
//!
//! Renders the duplication report as a self-contained Markdown card
//! suitable for a sticky PR comment, an issue body, a dashboard, or
//! `--format markdown > report.md`. The shape:
//!
//! - a header line with the total match count, per-tier counts, and the
//!   worst score across all matches;
//! - a tier-summary table (one row per non-empty tier: count + worst
//!   score in that tier);
//! - one collapsible `<details>` block per match — emoji tier severity,
//!   score, kind, and the participating files in the `<summary>`, with
//!   the full `file:line:col` list inside.
//!
//! An empty report renders a clean "✅ No matches above threshold."
//! card.
//!
//! ## The template owns presentation; Rust supplies semantic data
//!
//! The view-model below carries DATA — scores stay `f64`, tier/kind
//! labels come from the shared [`crate::domain::Tier::as_str`] /
//! [`crate::domain::FormKind::as_str`], form references come from the
//! shared [`crate::adapters::reporters::format_form_ref`]. The askama
//! template (`crates/dry-core/templates/markdown_report.md`) owns ALL
//! structure, layout, emoji, and numeric formatting (`{:.2}` via the
//! askama `format` filter). This is askama at the right layer:
//! composition of semantic data, not concatenation of pre-rendered
//! strings.
//!
//! ## Determinism is shared with the text reporter
//!
//! Grouping + sorting flows through the shared
//! [`crate::adapters::reporters::group_and_sort_by_tier`] helper:
//! tiers iterate in canonical order via a `BTreeMap` (robust to `Tier`
//! gaining a `#[non_exhaustive]` variant), and within each tier matches
//! sort by score DESC (via `f64::total_cmp`) then primary form ASC —
//! byte-for-byte the same ordering the text reporter uses, so a reader
//! can cross-reference the two surfaces. Because buckets arrive sorted
//! DESC, each tier's worst score is its last element and the overall
//! worst score is the max of the buckets' first elements.
//!
//! ## Column display is 1-based at the surface
//!
//! [`crate::domain::Span`] columns are 0-indexed in the domain; the
//! shared `format_form_ref` converts to 1-based via `saturating_add(1)`,
//! matching the text and GitHub-annotations reporters. The per-surface
//! 0-vs-1 convention is documented in AGENTS.md.
//!
//! ## Emoji are not ANSI
//!
//! Tier severity renders as emoji (🔴 / 🟡 / 🔵), which are ordinary
//! UTF-8 — the reporter still emits zero ANSI escape bytes, so the
//! no-ANSI contract the text reporter shares is preserved here too.

use askama::Template;

use crate::adapters::reporters::{format_form_ref, group_and_sort_by_tier};
use crate::domain::{Report, Tier};

/// Render `report` as a rich GitHub-flavored Markdown card.
///
/// Findings group by tier (`auto_refactor` first, then `review_first`,
/// then `advisory`). Inside each tier, matches order by descending
/// score; ties break on primary form path then span start, both
/// ascending — the same ordering the text reporter uses.
///
/// An empty `report.matches` renders a single "✅ No matches above
/// threshold." card instead of an empty document; the reporter never
/// emits a bare blank string.
///
/// Output is plain UTF-8 with `\n` line endings and a trailing newline
/// (POSIX text-file convention; downstream PR-comment emitters that
/// `cat` the file rely on it).
///
/// # Panics
///
/// Never, in practice. The internal `.expect()` on the askama render
/// is unreachable: [`MarkdownReport`] owns every field it interpolates
/// (no borrowed lifetimes, no fallible formatters), so the
/// compile-time-checked template render is total. The `expect`
/// documents the invariant rather than guarding a real failure mode.
#[must_use]
pub fn render(report: &Report) -> String {
    let view = build_view(report);
    let mut out = view
        .render()
        .expect("markdown template render is total — all fields owned");
    // POSIX text files end with `\n`. The template's trailing `{%- -%}`
    // whitespace control can strip it; restore so consumers that append
    // a heredoc/EOF delimiter on its own line (the scorecard action's
    // `cat <file>` path) parse correctly.
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Assemble the semantic view-model from `report`.
///
/// Grouping + sorting is delegated to the shared
/// [`group_and_sort_by_tier`] helper (the single cross-reporter pass).
/// Per-tier and overall worst scores are read off the already-sorted
/// buckets — no second sort.
fn build_view(report: &Report) -> MarkdownReport {
    let groups = group_and_sort_by_tier(report);

    let total_matches: usize = groups.values().map(Vec::len).sum();

    let mut worst_overall: Option<f64> = None;
    let tiers: Vec<TierView> = groups
        .into_iter()
        .map(|(tier, bucket)| {
            // Buckets arrive sorted by score DESC, so the first element
            // is the worst (highest) score in the tier and the last is
            // the lowest. Track the overall worst as the max of the
            // per-tier worsts.
            let worst = bucket.first().map_or(0.0, |m| m.score);
            worst_overall = Some(worst_overall.map_or(worst, |w| w.max(worst)));
            let matches = bucket.iter().map(|m| MatchView::from_match(m)).collect();
            TierView {
                tier,
                count: bucket.len(),
                worst,
                matches,
            }
        })
        .collect();

    MarkdownReport {
        total_matches,
        worst_overall: worst_overall.unwrap_or(0.0),
        tiers,
    }
}

/// Top-level markdown view-model. Carries semantic data only; the
/// template owns every presentation decision (layout, emoji, numeric
/// formatting).
#[derive(Template)]
#[template(path = "markdown_report.md", escape = "none")]
struct MarkdownReport {
    /// Total number of matches across all tiers.
    total_matches: usize,
    /// Worst (highest) score across all matches; `0.0` when empty.
    worst_overall: f64,
    /// One entry per non-empty tier, in canonical tier order.
    tiers: Vec<TierView>,
}

/// One tier's worth of findings — the tier itself (for the template's
/// emoji + label mapping), its count, its worst score, and its sorted
/// matches.
struct TierView {
    /// The routing tier; the template derives its emoji and label.
    tier: Tier,
    /// Number of matches in this tier.
    count: usize,
    /// Worst (highest) score in this tier.
    worst: f64,
    /// Matches, already sorted by score DESC then primary form ASC.
    matches: Vec<MatchView>,
}

/// One match as semantic data: its tier (for the per-match emoji), raw
/// `f64` score, kind label, primary + partner file labels for the
/// `<summary>`, and the full `file:line:col` list for the body.
struct MatchView {
    /// The routing tier; the template derives the per-match emoji.
    tier: Tier,
    /// Raw Jaccard score; the template formats it `{:.2}`.
    score: f64,
    /// Form-kind label of the primary form (`production` / `test` /
    /// `doctest`) via the shared [`crate::domain::FormKind::as_str`].
    kind: &'static str,
    /// `file:line:col` strings, one per participating form, in the
    /// order they appear on the `Match`.
    forms: Vec<String>,
    /// Bare file path of the primary form (no line/col) for the compact
    /// `<summary>` heading.
    primary_file: String,
    /// Bare file path of the partner form, if the match has a second
    /// form — drives the `a ↔ b` summary affordance.
    partner_file: Option<String>,
}

impl MatchView {
    fn from_match(m: &crate::domain::Match) -> Self {
        let kind = m.forms.first().map_or("unknown", |f| f.kind.as_str());
        let primary_file = m
            .forms
            .first()
            .map_or_else(String::new, |f| f.file.to_string());
        let partner_file = m.forms.get(1).map(|f| f.file.to_string());
        Self {
            tier: m.tier,
            score: m.score,
            kind,
            forms: m.forms.iter().map(format_form_ref).collect(),
            primary_file,
            partner_file,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FilePath, FormKind, FormRef, LineColumn, Match, Span, Summary, Tier};

    fn make_form_ref(path: &str, line: u32) -> FormRef {
        FormRef::new(
            FilePath::from(std::path::PathBuf::from(path)),
            Span::try_new(LineColumn::new(line, 0), LineColumn::new(line + 2, 12)).unwrap(),
            FormKind::Production,
        )
    }

    // These inline tests assert beautification-SURVIVING invariants
    // (presence of refs, ordering, no-ANSI, empty-card text). The exact
    // rendered layout is locked by the insta snapshots in
    // `tests/markdown_snapshot.rs` — duplicating exact strings here would
    // make them brittle clones of those snapshots.

    #[test]
    fn empty_report_renders_no_matches_card() {
        let out = render(&Report::empty_passed());
        assert!(out.contains("Duplication Report"), "out: {out}");
        assert!(out.contains("No matches above threshold."), "out: {out}");
        assert!(!out.contains("auto_refactor"), "out: {out}");
        assert!(out.ends_with('\n'), "must end with newline: {out:?}");
    }

    #[test]
    fn single_match_lists_participating_forms() {
        let m = Match::new(
            vec![make_form_ref("src/a.rs", 10), make_form_ref("src/b.rs", 20)],
            0.92,
            Tier::ReviewFirst,
        );
        let report = Report::new(vec![m], Summary::new(), false);
        let out = render(&report);
        // Columns are 1-based at the surface (span col 0 -> :1).
        assert!(out.contains("src/a.rs:10:1"), "out: {out}");
        assert!(out.contains("src/b.rs:20:1"), "out: {out}");
        // The score is rendered somewhere in the card.
        assert!(out.contains("0.92"), "out: {out}");
        // Tier label sourced from `Tier::as_str` (single vocabulary).
        assert!(out.contains("review_first"), "out: {out}");
        // Kind label sourced from `FormKind::as_str`.
        assert!(out.contains("production"), "out: {out}");
    }

    #[test]
    fn output_contains_no_ansi_escape_codes() {
        // Emoji severity markers are UTF-8, NOT ANSI — the no-ANSI
        // contract the text reporter shares must still hold.
        let m = Match::new(
            vec![make_form_ref("src/a.rs", 10)],
            0.95,
            Tier::AutoRefactor,
        );
        let report = Report::new(vec![m], Summary::new(), false);
        let out = render(&report);
        assert!(
            !out.bytes().any(|b| b == 0x1B),
            "markdown reporter must not emit ANSI: {out:?}"
        );
    }

    #[test]
    fn tiers_render_in_canonical_order() {
        let auto = Match::new(vec![make_form_ref("src/a.rs", 1)], 0.97, Tier::AutoRefactor);
        let review = Match::new(vec![make_form_ref("src/b.rs", 1)], 0.88, Tier::ReviewFirst);
        let adv = Match::new(vec![make_form_ref("src/c.rs", 1)], 0.81, Tier::Advisory);
        // Pass in non-canonical order — render must group + sort.
        let report = Report::new(vec![adv, review, auto], Summary::new(), false);
        let out = render(&report);
        let auto_idx = out.find("auto_refactor").expect("auto present");
        let review_idx = out.find("review_first").expect("review present");
        let adv_idx = out.find("advisory").expect("advisory present");
        assert!(auto_idx < review_idx, "{out}");
        assert!(review_idx < adv_idx, "{out}");
    }

    #[test]
    fn within_a_tier_higher_score_comes_first() {
        let lower = Match::new(vec![make_form_ref("src/x.rs", 1)], 0.86, Tier::ReviewFirst);
        let higher = Match::new(vec![make_form_ref("src/y.rs", 1)], 0.94, Tier::ReviewFirst);
        let report = Report::new(vec![lower, higher], Summary::new(), false);
        let out = render(&report);
        let higher_idx = out.find("0.94").expect("higher score present");
        let lower_idx = out.find("0.86").expect("lower score present");
        assert!(higher_idx < lower_idx, "{out}");
    }

    #[test]
    fn total_match_count_appears_in_header() {
        let a = Match::new(vec![make_form_ref("src/a.rs", 1)], 0.90, Tier::ReviewFirst);
        let b = Match::new(vec![make_form_ref("src/b.rs", 1)], 0.89, Tier::ReviewFirst);
        let c = Match::new(vec![make_form_ref("src/c.rs", 1)], 0.97, Tier::AutoRefactor);
        let report = Report::new(vec![a, b, c], Summary::new(), false);
        let out = render(&report);
        assert!(out.contains("3 matches"), "expected total count: {out}");
    }
}
