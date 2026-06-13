//! Language-agnostic reporters for the dry structural duplication
//! detector.
//!
//! Each reporter is a small free-function module:
//!
//! - [`json::render`] — locked v0.1 wire envelope per
//!   `ops/decisions/dry-rs/adr-nested-json-envelope.md`.
//! - [`text::render`] — human-friendly terminal output grouped by tier.
//! - [`markdown::render`] — GitHub-flavored Markdown grouped by tier
//!   (askama compile-time template), suitable for PR comments / issue
//!   bodies / `report.md` (dry-rs#91).
//! - [`html::render`] — self-contained single-file vanilla HTML explorer
//!   (askama compile-time template) that inlines the full JSON envelope
//!   into a `#dry-data` island and renders interactive views client-side
//!   (dry-rs#147, epic #111).
//! - [`github_annotations::render`] — GitHub Actions workflow commands
//!   (`::error::` / `::warning::` / `::notice::`) so duplications
//!   surface inline on the PR "Files Changed" tab without GHAS / Code
//!   Scanning licensing.
//!
//! No reporter trait at v0.1 — every reporter has the same shape
//! (`render(&Report, ...) -> ...`) but the CLI dispatches via a
//! `Format` enum match in PR 8. Adding a reporter is "new module +
//! enum variant"; a trait abstraction earns its complexity when
//! polymorphism is actually exercised, which it is not here. See
//! `adr-hexagonal-layout.md` §"Three module-roster divergences from
//! scrap-rs".

pub mod github_annotations;
pub mod html;
pub mod json;
pub mod markdown;
pub mod text;

use std::collections::BTreeMap;

use crate::domain::{FormRef, Match, Report, Tier};

/// Render a form reference as `file:line:col`.
///
/// Columns are 0-indexed in [`crate::domain::Span`]; the surface display
/// is 1-based (`saturating_add(1)`). This single copy is shared by the
/// text and markdown reporters — both already applied the identical
/// `+1`, so unifying the rendering removes copy-paste without changing
/// the per-surface 0-vs-1 *semantics* the AGENTS.md note guards (that
/// note is about the column convention, not about banning a shared
/// formatter). The github-annotations reporter does NOT use this helper:
/// it emits split `file=…,line=…,col=…` workflow-command properties with
/// GHA-specific escaping, a genuinely different shape.
#[must_use]
pub fn format_form_ref(form: &FormRef) -> String {
    format!(
        "{}:{}:{}",
        form.file,
        form.span.start.line,
        form.span.start.column.saturating_add(1)
    )
}

/// Group `report.matches` by tier and sort within each tier.
///
/// Grouping uses a [`BTreeMap`] so iteration is robust to `Tier` gaining
/// a new variant (`Tier` is `#[non_exhaustive]`; a hand-rolled
/// `[AutoRefactor, ReviewFirst, Advisory]` loop would silently drop any
/// new tier). The derived `Ord` on `Tier` orders by declaration —
/// `AutoRefactor < ReviewFirst < Advisory` — the canonical display
/// order.
///
/// Within each tier, matches sort by score DESC (via [`f64::total_cmp`]
/// for a total order even on pathological inputs — the comparison
/// engine already established this idiom), then by primary [`FormRef`]
/// (file, then span start) ASC for determinism across walker orderings.
///
/// This is the single grouping+sorting pass shared by the text and
/// markdown reporters so their two surfaces stay cross-referenceable.
#[must_use]
pub fn group_and_sort_by_tier(report: &Report) -> BTreeMap<Tier, Vec<&Match>> {
    let mut groups: BTreeMap<Tier, Vec<&Match>> = BTreeMap::new();
    for m in &report.matches {
        groups.entry(m.tier).or_default().push(m);
    }
    for bucket in groups.values_mut() {
        bucket.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| match (a.forms.first(), b.forms.first()) {
                    (Some(af), Some(bf)) => af
                        .file
                        .as_path()
                        .cmp(bf.file.as_path())
                        .then_with(|| af.span.start.cmp(&bf.span.start)),
                    _ => std::cmp::Ordering::Equal,
                })
        });
    }
    groups
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::{FilePath, FormKind, LineColumn, Span, Summary};

    fn form_ref(path: &str, line: u32) -> FormRef {
        FormRef::new(
            FilePath::from(PathBuf::from(path)),
            Span::try_new(LineColumn::new(line, 0), LineColumn::new(line + 2, 12)).unwrap(),
            FormKind::Production,
        )
    }

    #[test]
    fn format_form_ref_is_one_based_column() {
        // Span column 0 in the domain renders as :1 at the surface.
        let f = form_ref("src/a.rs", 10);
        assert_eq!(format_form_ref(&f), "src/a.rs:10:1");
    }

    #[test]
    fn group_and_sort_orders_tiers_canonically() {
        let auto = Match::new(vec![form_ref("src/a.rs", 1)], 0.97, Tier::AutoRefactor);
        let review = Match::new(vec![form_ref("src/b.rs", 1)], 0.88, Tier::ReviewFirst);
        let adv = Match::new(vec![form_ref("src/c.rs", 1)], 0.81, Tier::Advisory);
        let report = Report::new(vec![adv, review, auto], Summary::new(), false);
        let groups = group_and_sort_by_tier(&report);
        let tiers: Vec<Tier> = groups.keys().copied().collect();
        assert_eq!(
            tiers,
            vec![Tier::AutoRefactor, Tier::ReviewFirst, Tier::Advisory]
        );
    }

    #[test]
    fn group_and_sort_orders_within_tier_by_score_desc() {
        let lower = Match::new(vec![form_ref("src/x.rs", 1)], 0.86, Tier::ReviewFirst);
        let higher = Match::new(vec![form_ref("src/y.rs", 1)], 0.94, Tier::ReviewFirst);
        let report = Report::new(vec![lower, higher], Summary::new(), false);
        let groups = group_and_sort_by_tier(&report);
        let bucket = &groups[&Tier::ReviewFirst];
        // Higher score sorts first; assert the ordering rather than an
        // exact float equality (clippy::float_cmp).
        assert!(bucket[0].score > bucket[1].score);
    }

    #[test]
    fn group_and_sort_ties_break_on_file_then_span() {
        // Equal score — break on file path ASC.
        let b = Match::new(vec![form_ref("src/b.rs", 5)], 0.90, Tier::ReviewFirst);
        let a = Match::new(vec![form_ref("src/a.rs", 9)], 0.90, Tier::ReviewFirst);
        let report = Report::new(vec![b, a], Summary::new(), false);
        let groups = group_and_sort_by_tier(&report);
        let bucket = &groups[&Tier::ReviewFirst];
        assert_eq!(bucket[0].forms[0].file.to_string(), "src/a.rs");
        assert_eq!(bucket[1].forms[0].file.to_string(), "src/b.rs");
    }
}
