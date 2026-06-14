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

use crate::adapters::reporters::{format_form_ref, group_and_sort_by_tier};
use crate::domain::Report;

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

    // Group + sort via the shared reporter helper so the text and
    // markdown surfaces stay cross-referenceable. BTreeMap iteration is
    // robust to `Tier` gaining a new variant; within-tier ordering is
    // score DESC (via `f64::total_cmp`) then primary FormRef ASC.
    for (tier, bucket) in group_and_sort_by_tier(report) {
        let _ = writeln!(out, "\n{} ({})", tier.as_str(), bucket.len());

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
            let kind = m.forms.first().map_or("<unknown>", |f| f.kind.as_str());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Match, Summary, Tier};
    use crate::test_support::make_form_ref_at as make_form_ref;

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
