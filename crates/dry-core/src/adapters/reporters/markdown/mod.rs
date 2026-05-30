//! Markdown reporter — GitHub-flavored Markdown grouped by tier.
//!
//! Renders findings grouped by routing tier (`auto_refactor` →
//! `review_first` → `advisory`), one section per non-empty tier. Each
//! match becomes a `### Score N · kind` block followed by a fenced
//! code block listing the participating form locations
//! (`file:line:col`). No ANSI, no tables that depend on a terminal —
//! the output is plain GitHub-flavored Markdown suitable for PR
//! comments, issue bodies, dashboards, or `--format markdown >
//! report.md`.
//!
//! ## Rendering goes through an askama compile-time template
//!
//! The structure (header → tier sections → per-match blocks) lives in
//! `crates/dry-core/templates/markdown_report.md` and is type-checked
//! against the view structs below at build time (dry-rs#91). Mirrors
//! the crap4rs markdown reporter pattern (crap4rs#260). Width- and
//! precision-formatted fields (`score` as `{:.2}`, the 1-based column
//! display) are pre-computed in Rust because askama's `{{ }}`
//! interpolation does not honor Rust format specifiers — the template
//! is composition-only.
//!
//! ## Determinism mirrors the text reporter
//!
//! Tiers iterate via a `BTreeMap<Tier, …>` so the section ordering is
//! robust to `Tier` gaining a new variant (`Tier` is
//! `#[non_exhaustive]`; a hand-rolled `[AutoRefactor, ReviewFirst,
//! Advisory]` loop would silently omit any new tier). Within a tier,
//! matches sort by score DESC, then primary `FormRef` (file, then
//! span start) ASC — identical to the text reporter so a reader can
//! cross-reference the two surfaces.
//!
//! ## Column display is 1-based at the surface
//!
//! [`crate::domain::Span`] columns are 0-indexed in the domain; the
//! `file:line:col` rendering converts to 1-based via
//! `saturating_add(1)`, matching the text reporter and the GitHub
//! annotations reporter. The per-surface conversion is deliberate and
//! documented in AGENTS.md (do NOT unify these surfaces).

use std::collections::BTreeMap;

use askama::Template;

use crate::domain::{FormRef, Match, Report, Tier};

/// Render `report` as tier-grouped GitHub-flavored Markdown.
///
/// Findings group by tier (`auto_refactor` first, then `review_first`,
/// then `advisory`). Inside each tier, matches order by descending
/// score; ties break on primary form path then span start, both
/// ascending — the same ordering the text reporter uses.
///
/// An empty `report.matches` renders a single-line
/// `"No matches above threshold."` body under the report header
/// instead of an empty document; the reporter never emits a bare
/// blank string.
///
/// Output is plain UTF-8 with `\n` line endings and a trailing
/// newline (POSIX text-file convention; downstream PR-comment
/// emitters that `cat` the file rely on it).
///
/// # Panics
///
/// Never, in practice. The internal `.expect()` on the askama render
/// is unreachable: `MarkdownReport` owns every field it interpolates
/// (no borrowed lifetimes, no fallible formatters), so the compile-
/// time-checked template render is total. The `expect` documents the
/// invariant rather than guarding a real failure mode — mirroring the
/// crap4rs reporter's "render is total" rationale.
#[must_use]
pub fn render(report: &Report) -> String {
    let body = build_body(report);
    let tmpl = MarkdownReport { body };
    let mut out = tmpl
        .render()
        .expect("markdown template render is total — all fields owned");
    // POSIX text files end with `\n`. The template's trailing `{%- -%}`
    // whitespace control can strip it; restore so consumers that
    // append a heredoc/EOF delimiter on its own line (the scorecard
    // action's `cat <file>` path) parse correctly. Mirrors crap4rs.
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Assemble the template's body discriminant from the report. Empty
/// reports collapse to [`MarkdownBody::Empty`]; otherwise findings are
/// grouped into one [`TierSection`] per non-empty tier in canonical
/// tier order.
fn build_body(report: &Report) -> MarkdownBody {
    if report.matches.is_empty() {
        return MarkdownBody::Empty;
    }

    // Group by tier via BTreeMap so iteration is robust to a new
    // `Tier` variant (non_exhaustive). Derived `Ord` on `Tier` orders
    // by declaration: AutoRefactor < ReviewFirst < Advisory — the
    // canonical display order. Mirrors the text reporter.
    let mut groups: BTreeMap<Tier, Vec<&Match>> = BTreeMap::new();
    for m in &report.matches {
        groups.entry(m.tier).or_default().push(m);
    }

    let sections = groups
        .into_iter()
        .map(|(tier, mut bucket)| {
            bucket.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| match (a.forms.first(), b.forms.first()) {
                        (Some(af), Some(bf)) => af
                            .file
                            .as_path()
                            .cmp(bf.file.as_path())
                            .then_with(|| af.span.start.cmp(&bf.span.start)),
                        _ => std::cmp::Ordering::Equal,
                    })
            });
            TierSection {
                heading: tier_heading(tier),
                matches: bucket.into_iter().map(match_block).collect(),
            }
        })
        .collect();

    MarkdownBody::Filled { sections }
}

fn match_block(m: &Match) -> MatchBlock {
    let kind = m
        .forms
        .first()
        .map_or("unknown", |f| format_form_kind(f.kind));
    MatchBlock {
        score: format!("{:.2}", m.score),
        kind,
        forms: m.forms.iter().map(format_form_ref).collect(),
    }
}

// `Tier` and `FormKind` are `#[non_exhaustive]` *for downstream
// consumers*; within `dry-core` (where they are declared) every
// variant is visible and exhaustive-match is enforced. Adding a new
// tier or kind here is a deliberate, compile-time-broken event.
const fn tier_heading(tier: Tier) -> &'static str {
    match tier {
        Tier::AutoRefactor => "auto_refactor",
        Tier::ReviewFirst => "review_first",
        Tier::Advisory => "advisory",
    }
}

const fn format_form_kind(kind: crate::domain::FormKind) -> &'static str {
    match kind {
        crate::domain::FormKind::Production => "production",
        crate::domain::FormKind::Test => "test",
        crate::domain::FormKind::Doctest => "doctest",
    }
}

/// Render a form reference as `file:line:col`. Columns are 0-indexed
/// in the domain; the surface display is 1-based (`saturating_add(1)`),
/// matching the text and GitHub-annotations reporters.
fn format_form_ref(form: &FormRef) -> String {
    format!(
        "{}:{}:{}",
        form.file,
        form.span.start.line,
        form.span.start.column.saturating_add(1)
    )
}

/// Top-level markdown template. The body discriminant carries the
/// already-grouped, already-sorted sections; the template only
/// composes structure.
#[derive(Template)]
#[template(path = "markdown_report.md", escape = "none")]
struct MarkdownReport {
    body: MarkdownBody,
}

/// Body discriminant. `Empty` renders a single advisory line; `Filled`
/// carries one section per non-empty tier in canonical order.
enum MarkdownBody {
    Empty,
    Filled { sections: Vec<TierSection> },
}

/// One tier's worth of findings — a heading label and its ordered
/// match blocks.
struct TierSection {
    heading: &'static str,
    matches: Vec<MatchBlock>,
}

/// One match rendered as a score/kind header plus a fenced list of
/// participating form locations.
struct MatchBlock {
    /// Pre-formatted `{:.2}` Jaccard score.
    score: String,
    /// Form-kind label of the primary form (`production` / `test` /
    /// `doctest`).
    kind: &'static str,
    /// `file:line:col` strings, one per participating form, in the
    /// order they appear on the `Match`.
    forms: Vec<String>,
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

    #[test]
    fn empty_report_renders_no_matches_line() {
        let out = render(&Report::empty_passed());
        assert!(out.contains("# Duplication Report"), "out: {out}");
        assert!(out.contains("No matches above threshold."), "out: {out}");
        assert!(!out.contains("auto_refactor"), "out: {out}");
        assert!(out.ends_with('\n'), "must end with newline: {out:?}");
    }

    #[test]
    fn single_match_renders_tier_section_and_fenced_block() {
        let m = Match::new(
            vec![make_form_ref("src/a.rs", 10), make_form_ref("src/b.rs", 20)],
            0.92,
            Tier::ReviewFirst,
        );
        let report = Report::new(vec![m], Summary::new(), false);
        let out = render(&report);
        assert!(out.contains("## review_first (1)"), "out: {out}");
        assert!(out.contains("### Score 0.92 · production"), "out: {out}");
        // Fenced code block boundaries.
        assert!(out.contains("```"), "expected fenced block: {out}");
        // Columns are 1-based at the surface (span col 0 -> :1).
        assert!(out.contains("src/a.rs:10:1"), "out: {out}");
        assert!(out.contains("src/b.rs:20:1"), "out: {out}");
    }

    #[test]
    fn output_contains_no_ansi_escape_codes() {
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
    fn section_count_reflects_number_of_matches_in_tier() {
        let a = Match::new(vec![make_form_ref("src/a.rs", 1)], 0.90, Tier::ReviewFirst);
        let b = Match::new(vec![make_form_ref("src/b.rs", 1)], 0.89, Tier::ReviewFirst);
        let report = Report::new(vec![a, b], Summary::new(), false);
        let out = render(&report);
        assert!(out.contains("## review_first (2)"), "out: {out}");
    }
}
