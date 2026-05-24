//! Text reporter — human-friendly terminal output.
//!
//! Renders findings grouped by tier (`auto_refactor` →
//! `review_first` → `advisory`) in a [`comfy_table`] table. The
//! reporter emits NO ANSI color codes at this layer — color belongs to
//! the CLI (`--color always|auto|never` flag landing in PR 8). This
//! keeps the output safe for `pbcopy`, GitHub PR comments, dashboard
//! ingestion, and any other consumer that doesn't strip ANSI.
//!
//! Implementation lands below; this docs block reserves the surface
//! for the snapshot tests to import.

use std::fmt::Write;

use comfy_table::{Cell, ContentArrangement, Table, presets::ASCII_MARKDOWN};

use crate::domain::{Match, Report, Tier};

/// Render `report` as a human-friendly terminal view.
///
/// Findings group by tier (`auto_refactor` first, then `review_first`,
/// then `advisory`). Inside each tier, rows are ordered by descending
/// score; ties break on file path then start line, both ascending.
///
/// The output is plain ASCII Markdown — no ANSI escapes. CLI-side
/// color decoration lands in PR 8 via the `--color` flag.
///
/// Empty `report.matches` produces a single-line summary
/// (`"No matches above threshold."`) instead of an empty table; the
/// reporter never emits a bare blank string.
#[must_use]
pub fn render(report: &Report) -> String {
    if report.matches.is_empty() {
        return "No matches above threshold.\n".into();
    }

    let mut out = String::new();

    // Group matches by tier, preserving the tier ordering
    // (AutoRefactor < ReviewFirst < Advisory by derived `Ord`).
    let mut groups: Vec<(Tier, Vec<&Match>)> = Vec::new();
    for tier in [Tier::AutoRefactor, Tier::ReviewFirst, Tier::Advisory] {
        let bucket: Vec<&Match> = report.matches.iter().filter(|m| m.tier == tier).collect();
        if !bucket.is_empty() {
            groups.push((tier, bucket));
        }
    }

    for (tier, mut bucket) in groups {
        // Stable ordering within each tier: score DESC, then primary
        // FormRef (file, span.start) ASC for determinism across
        // walker orderings.
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

        let _ = writeln!(out, "\n{} ({})", tier_heading(tier), bucket.len());

        let mut table = Table::new();
        table
            .load_preset(ASCII_MARKDOWN)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["Score", "Kind", "Forms"]);

        for m in bucket {
            let forms = m
                .forms
                .iter()
                .map(format_form_ref)
                .collect::<Vec<_>>()
                .join("\n");
            let kind = m
                .forms
                .first()
                .map_or("<unknown>", |f| format_form_kind(f.kind));
            table.add_row(vec![
                Cell::new(format!("{:.2}", m.score)),
                Cell::new(kind),
                Cell::new(forms),
            ]);
        }

        out.push_str(&table.to_string());
        out.push('\n');
    }

    out
}

// `Tier` and `FormKind` are `#[non_exhaustive]` *for downstream consumers*;
// within `dry-core` (where they are declared) every variant is visible
// and exhaustive-match is enforced. Adding a new tier or kind here is
// a deliberate, compile-time-broken event — exactly what we want.
fn tier_heading(tier: Tier) -> &'static str {
    match tier {
        Tier::AutoRefactor => "auto_refactor",
        Tier::ReviewFirst => "review_first",
        Tier::Advisory => "advisory",
    }
}

fn format_form_kind(kind: crate::domain::FormKind) -> &'static str {
    match kind {
        crate::domain::FormKind::Production => "production",
        crate::domain::FormKind::Test => "test",
        crate::domain::FormKind::Doctest => "doctest",
    }
}

fn format_form_ref(form: &crate::domain::FormRef) -> String {
    format!(
        "{}:{}:{}",
        form.file,
        form.span.start.line,
        form.span.start.column.saturating_add(1)
    )
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
    fn empty_report_renders_single_line() {
        let out = render(&Report::empty_passed());
        assert_eq!(out, "No matches above threshold.\n");
    }

    #[test]
    fn single_match_renders_header_and_table() {
        let m = Match::new(
            vec![make_form_ref("src/a.rs", 10), make_form_ref("src/b.rs", 20)],
            0.92,
            Tier::ReviewFirst,
        );
        let report = Report::new(vec![m], Summary::new(), false);
        let out = render(&report);
        assert!(out.contains("review_first (1)"), "out: {out}");
        assert!(out.contains("0.92"), "out: {out}");
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
        // ANSI escape sequences begin with ESC (0x1B) — assert no byte
        // matches.
        assert!(
            !out.bytes().any(|b| b == 0x1B),
            "text reporter must not emit ANSI: {out:?}"
        );
    }

    #[test]
    fn tiers_render_in_canonical_order() {
        // auto_refactor must come first, then review_first, then advisory.
        let auto = Match::new(vec![make_form_ref("src/a.rs", 1)], 0.97, Tier::AutoRefactor);
        let review = Match::new(vec![make_form_ref("src/b.rs", 1)], 0.88, Tier::ReviewFirst);
        let adv = Match::new(vec![make_form_ref("src/c.rs", 1)], 0.81, Tier::Advisory);
        // Intentionally pass in non-canonical order — render must sort.
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
        let higher_idx = out.find("0.94").expect("higher score line present");
        let lower_idx = out.find("0.86").expect("lower score line present");
        assert!(higher_idx < lower_idx, "{out}");
    }
}
