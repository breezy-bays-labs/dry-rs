//! GitHub Actions inline annotations reporter.
//!
//! Emits `::error` / `::warning` / `::notice` workflow-command lines so
//! dry-rs findings render inline on the PR "Files Changed" tab —
//! universal, free, no GHAS / Code Scanning licensing required. The
//! Actions runner intercepts the
//! `::workflow-command file=…,line=…,col=…,endLine=…,endColumn=…::message`
//! shape and renders an inline annotation at the named position.
//!
//! ## Tier → severity mapping
//!
//! | Tier            | GitHub Annotation |
//! |-----------------|--------------------|
//! | `auto_refactor` | `::error::`        |
//! | `review_first`  | `::warning::`      |
//! | `advisory`      | `::notice::`       |
//!
//! `advisory → ::notice::` matches the semantic meaning of the tier
//! (informational; surface as advisory, no refactor proposed) over
//! crap4rs's single-tier `::warning::` precedent. crap4rs reports only
//! threshold-exceeders so its single-severity mapping is the right
//! shape for its data; dry-rs has three named tiers where `::notice::`
//! is the semantically-aligned floor. The mapping is settled at v0.1
//! and matches the issue AC's preferred path.
//!
//! ## No env-detection at this layer
//!
//! `render` always emits the workflow commands when invoked. Detection
//! of `GITHUB_ACTIONS=true` is a CLI-flag concern (the user selects
//! `--format github-annotations`); env-based no-op'ing would couple
//! the reporter to runtime state it should not own. Per crap4rs's
//! precedent and the issue Discovery note.
//!
//! ## GHA escape rules (load-bearing)
//!
//! The workflow-command grammar has TWO escape contexts:
//!
//! - **Message data** (text after the final `::`): escape `%` → `%25`,
//!   `\r` → `%0D`, `\n` → `%0A`. `%` MUST escape first or the `%25`
//!   from CR/LF gets double-escaped.
//! - **Property values** (between `name=` and the next `,` or `::`):
//!   all of the above PLUS `:` → `%3A` and `,` → `%2C`. POSIX file
//!   paths legally contain `:` and `,`; an unescaped delimiter inside
//!   `file=` corrupts the runner's parse of the entire annotation.
//!
//! `file=` is the only dynamic property value the reporter emits;
//! `line`/`col`/`endLine`/`endColumn` are integers from validated
//! [`crate::domain::Span`]s.

use crate::domain::{Match, Report, Tier};

/// Render `report` as a stream of GitHub Actions workflow-command
/// lines.
///
/// One annotation per finding's primary [`crate::domain::FormRef`]
/// (the first form in each [`Match`]). Severity follows the
/// tier-to-severity table in the module docs.
///
/// Output is plain UTF-8 with `\n` line endings. The reporter neither
/// reads nor depends on the `GITHUB_ACTIONS` environment variable;
/// env-based dispatch is a CLI concern (the `--format` flag selecting
/// this reporter is the user's signal).
///
/// Empty `report.matches` produces an empty string.
#[must_use]
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    for m in &report.matches {
        if let Some(line) = render_match(m) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

fn render_match(m: &Match) -> Option<String> {
    let primary = m.forms.first()?;
    let other = m.forms.get(1);

    let severity = severity_for(m.tier);
    let file = gha_escape_property(&primary.file.to_string());
    let line = primary.span.start.line;
    // `Span` uses 0-indexed columns; GHA's `col` is 1-indexed.
    let col = primary.span.start.column.saturating_add(1);
    let end_line = primary.span.end.line;
    let end_col = primary.span.end.column.saturating_add(1);

    let message_raw = format_message(m.score, m.tier, other);
    let message = gha_escape_message(&message_raw);

    Some(format!(
        "::{severity} file={file},line={line},col={col},endLine={end_line},endColumn={end_col}::{message}"
    ))
}

// `Tier` is `#[non_exhaustive]` *for downstream consumers*; within
// `dry-core` (where it is declared) every variant is visible. A new
// tier landing in `domain::enums` MUST update both the severity and
// label tables — the compiler enforces it.
const fn severity_for(tier: Tier) -> &'static str {
    match tier {
        Tier::AutoRefactor => "error",
        Tier::ReviewFirst => "warning",
        Tier::Advisory => "notice",
    }
}

const fn tier_label(tier: Tier) -> &'static str {
    match tier {
        Tier::AutoRefactor => "auto_refactor",
        Tier::ReviewFirst => "review_first",
        Tier::Advisory => "advisory",
    }
}

fn format_message(score: f64, tier: Tier, other: Option<&crate::domain::FormRef>) -> String {
    let label = tier_label(tier);
    match other {
        Some(o) => format!(
            "Duplicate of {file}:{line} (score={score:.2}, tier={label})",
            file = o.file,
            line = o.span.start.line,
        ),
        None => format!("Duplicate cluster (score={score:.2}, tier={label})"),
    }
}

/// Message-data escape per the GitHub Actions workflow-command spec.
/// Applied to text AFTER the final `::` in a workflow command.
///
/// Order matters: `%` is escaped first so the `%25` introduced for
/// subsequent CR/LF substitutions does not get re-escaped.
fn gha_escape_message(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Property-value escape per the GitHub Actions workflow-command spec.
/// Applied to text in `name=value` positions between the prefix and
/// the final `::`.
///
/// Extends [`gha_escape_message`] with `:` → `%3A` and `,` → `%2C`.
/// POSIX file paths legally contain both delimiters; an unescaped one
/// inside `file=` would terminate the property list and corrupt the
/// annotation parse.
fn gha_escape_property(s: &str) -> String {
    gha_escape_message(s)
        .replace(':', "%3A")
        .replace(',', "%2C")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FilePath, FormKind, FormRef, LineColumn, Match, Span, Summary, Tier};

    fn make_form_ref(path: &str, start_line: u32, end_line: u32) -> FormRef {
        FormRef::new(
            FilePath::from(std::path::PathBuf::from(path)),
            Span::try_new(
                LineColumn::new(start_line, 0),
                LineColumn::new(end_line, 12),
            )
            .unwrap(),
            FormKind::Production,
        )
    }

    fn match_with_tier(tier: Tier) -> Match {
        Match::new(
            vec![
                make_form_ref("src/a.rs", 10, 12),
                make_form_ref("src/b.rs", 20, 22),
            ],
            0.92,
            tier,
        )
    }

    #[test]
    fn empty_report_renders_empty_string() {
        assert_eq!(render(&Report::empty_passed()), "");
    }

    #[test]
    fn auto_refactor_emits_error_severity() {
        let report = Report::new(
            vec![match_with_tier(Tier::AutoRefactor)],
            Summary::new(),
            false,
        );
        let out = render(&report);
        assert!(out.starts_with("::error "), "out: {out:?}");
    }

    #[test]
    fn review_first_emits_warning_severity() {
        let report = Report::new(
            vec![match_with_tier(Tier::ReviewFirst)],
            Summary::new(),
            false,
        );
        let out = render(&report);
        assert!(out.starts_with("::warning "), "out: {out:?}");
    }

    #[test]
    fn advisory_emits_notice_severity() {
        let report = Report::new(vec![match_with_tier(Tier::Advisory)], Summary::new(), false);
        let out = render(&report);
        assert!(out.starts_with("::notice "), "out: {out:?}");
    }

    #[test]
    fn annotation_includes_all_required_property_keys() {
        let report = Report::new(
            vec![match_with_tier(Tier::ReviewFirst)],
            Summary::new(),
            false,
        );
        let out = render(&report);
        for key in ["file=", "line=", "col=", "endLine=", "endColumn="] {
            assert!(out.contains(key), "missing {key} in: {out}");
        }
    }

    #[test]
    fn line_and_column_are_one_indexed_with_explicit_value() {
        // Span uses 0-indexed cols, 1-indexed lines (per
        // span.rs module docs). GHA wants 1-indexed for both.
        // Span starts at line 10 col 0; expect `line=10,col=1`.
        let report = Report::new(
            vec![match_with_tier(Tier::ReviewFirst)],
            Summary::new(),
            false,
        );
        let out = render(&report);
        assert!(out.contains("line=10"), "out: {out}");
        assert!(out.contains("col=1"), "out: {out}");
        assert!(out.contains("endLine=12"), "out: {out}");
        assert!(out.contains("endColumn=13"), "out: {out}");
    }

    #[test]
    fn message_carries_score_and_tier_label() {
        let report = Report::new(
            vec![match_with_tier(Tier::ReviewFirst)],
            Summary::new(),
            false,
        );
        let out = render(&report);
        assert!(out.contains("score=0.92"), "out: {out}");
        assert!(out.contains("tier=review_first"), "out: {out}");
    }

    #[test]
    fn message_includes_partner_file_when_match_has_multiple_forms() {
        let report = Report::new(
            vec![match_with_tier(Tier::ReviewFirst)],
            Summary::new(),
            false,
        );
        let out = render(&report);
        // Primary at src/a.rs, partner at src/b.rs — message names
        // the partner so a reviewer can navigate without leaving the
        // inline diff.
        assert!(out.contains("Duplicate of src/b.rs:20"), "out: {out}");
    }

    #[test]
    fn each_annotation_is_a_single_line() {
        let report = Report::new(
            vec![
                match_with_tier(Tier::AutoRefactor),
                match_with_tier(Tier::ReviewFirst),
                match_with_tier(Tier::Advisory),
            ],
            Summary::new(),
            false,
        );
        let out = render(&report);
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines.len(), 3, "expected 3 annotations, got: {out}");
        for line in &lines {
            assert!(
                line.starts_with("::"),
                "annotation must start with ::: {line}"
            );
            assert!(!line[2..].contains('\n'), "no embedded newline: {line}");
        }
    }

    #[test]
    fn percent_in_path_is_property_escaped() {
        let mut m = match_with_tier(Tier::ReviewFirst);
        m.forms[0] = FormRef::new(
            FilePath::from(std::path::PathBuf::from("src/100%-coverage.rs")),
            Span::try_new(LineColumn::new(1, 0), LineColumn::new(2, 0)).unwrap(),
            FormKind::Production,
        );
        let report = Report::new(vec![m], Summary::new(), false);
        let out = render(&report);
        assert!(
            out.contains("file=src/100%25-coverage.rs"),
            "percent must escape to %25 in property value: {out}"
        );
    }

    #[test]
    fn colon_in_path_is_property_escaped() {
        let mut m = match_with_tier(Tier::ReviewFirst);
        m.forms[0] = FormRef::new(
            FilePath::from(std::path::PathBuf::from("a:b/file.rs")),
            Span::try_new(LineColumn::new(1, 0), LineColumn::new(2, 0)).unwrap(),
            FormKind::Production,
        );
        let report = Report::new(vec![m], Summary::new(), false);
        let out = render(&report);
        assert!(
            out.contains("file=a%3Ab/file.rs"),
            "colon in property value must escape to %3A: {out}"
        );
    }

    #[test]
    fn comma_in_path_is_property_escaped() {
        let mut m = match_with_tier(Tier::ReviewFirst);
        m.forms[0] = FormRef::new(
            FilePath::from(std::path::PathBuf::from("a,b/file.rs")),
            Span::try_new(LineColumn::new(1, 0), LineColumn::new(2, 0)).unwrap(),
            FormKind::Production,
        );
        let report = Report::new(vec![m], Summary::new(), false);
        let out = render(&report);
        assert!(
            out.contains("file=a%2Cb/file.rs"),
            "comma in property value must escape to %2C: {out}"
        );
    }

    #[test]
    fn message_data_only_escapes_percent_cr_lf() {
        // The message-data escape MUST NOT touch `:` or `,` — those
        // are legal in message content (e.g. `tier=review_first` and
        // any colon-bearing partner file in `Duplicate of <file>:<line>`).
        let m = Match::new(
            vec![
                make_form_ref("src/with:colon.rs", 10, 12),
                make_form_ref("src/partner.rs", 20, 22),
            ],
            0.95,
            Tier::AutoRefactor,
        );
        let report = Report::new(vec![m], Summary::new(), false);
        let out = render(&report);
        // The message itself contains `:` between file and line ("Duplicate of src/partner.rs:20")
        // and should NOT escape those.
        assert!(
            out.contains("Duplicate of src/partner.rs:20"),
            "message data must NOT escape colons: {out}"
        );
        // Property value DID escape its colon (`src/with:colon.rs` →
        // `src/with%3Acolon.rs`).
        assert!(
            out.contains("file=src/with%3Acolon.rs"),
            "property value must escape colons: {out}"
        );
    }
}
