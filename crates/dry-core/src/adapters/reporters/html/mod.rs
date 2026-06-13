//! HTML reporter — a self-contained single-file vanilla explorer
//! (dry-rs#147, epic #111).
//!
//! Renders the duplication report as ONE standalone `.html` file with no
//! framework, no build step, and no external assets. The whole [`Envelope`]
//! is serialized to JSON and injected ONCE into a
//! `<script id="dry-data" type="application/json">…</script>` island; the
//! page's inline ES-module `<script>` reads `#dry-data` and renders the
//! interactive views (overview, cluster list, cluster detail, tier/score
//! filters) client-side. The inline `<style>` block carries all CSS.
//!
//! This is the BASIC reference frontend — Claude Design polishes later. PR13
//! ships the bare REPORT reporter (overview + cluster views); the
//! substitution grid / d-slider / scope banner join in a later PR of epic
//! #111 as their backing wire fields (`Match.template`, `Envelope.scope`)
//! populate. The frontend degrades gracefully when `template` / `scope` /
//! `mode` / `capabilities` are absent (it MUST NOT throw on a missing
//! optional field).
//!
//! ## Single injection contract
//!
//! There is exactly ONE `{{ payload }}` interpolation — the serialized
//! envelope. Presentation is entirely client-side: the JS reads `result.*`
//! (the truthful gate, immune to view-shaping flags) and derives everything
//! else. No server-rendered match markup, so the HTML body stays a small,
//! stable shell regardless of corpus size; the payload carries the data.
//!
//! ## Reuse, not re-implementation
//!
//! A `<noscript>` overview (per-tier counts + worst score) is server-
//! rendered as a no-JavaScript fallback. Its grouping flows through the
//! SHARED [`crate::adapters::reporters::group_and_sort_by_tier`] helper —
//! the same pass the text and markdown reporters use — so the three
//! surfaces never drift and dry4rs's self-analysis does not flag the
//! grouping logic as duplicated. The reporter does NOT re-spell tier
//! grouping or sort order; it composes the shared semantic view-model.
//!
//! ## Column display is 1-based at the surface
//!
//! The injected payload carries the canonical 0-based column
//! ([`crate::domain::Span`] semantics); the JS converts to 1-based for
//! display (`+1`), matching the text / markdown / github-annotations
//! surfaces. The `<noscript>` fallback shows tier counts only (no
//! per-form columns), so the 0-vs-1 convention lives entirely in the JS.

use askama::Template;
use thiserror::Error;

use crate::adapters::reporters::group_and_sort_by_tier;
use crate::adapters::reporters::json::Envelope;
use crate::domain::Tier;

/// Errors produced by [`render`].
///
/// `#[non_exhaustive]` — adapter binaries pattern-match defensively;
/// future variants (e.g. a template-render failure mode that becomes
/// reachable when the template grows fallible filters) land without
/// breaking call sites.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HtmlError {
    /// `serde_json` failed to serialize the envelope into the `#dry-data`
    /// payload. In practice unreachable — every envelope field derives
    /// `Serialize` over owned data; the variant exists for forward-compat.
    #[error("html payload serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
    /// The askama template render failed. Unreachable in practice — the
    /// view owns every interpolated field (no borrowed lifetimes, no
    /// fallible formatters) so the compile-time-checked render is total;
    /// the variant documents the boundary.
    #[error("html template render failed: {0}")]
    Render(#[from] askama::Error),
}

/// Render `envelope` as a self-contained single-file HTML explorer.
///
/// The full envelope is serialized to JSON and injected once into the
/// `#dry-data` island; the inline ES-module script renders the interactive
/// views from `result.*`. A `<noscript>` overview is server-rendered as a
/// no-JavaScript fallback via the shared
/// [`group_and_sort_by_tier`] helper.
///
/// Output is UTF-8 with a trailing newline (POSIX text-file convention,
/// matching the other reporters).
///
/// # Errors
///
/// Returns [`HtmlError::Serialize`] if the envelope fails to serialize to
/// JSON, or [`HtmlError::Render`] if the askama template render fails.
/// Both are unreachable in practice (owned `Serialize` data; total
/// compile-checked template) but surfaced as typed errors rather than
/// panics so the run-loop dispatch can degrade cleanly.
pub fn render(envelope: &Envelope) -> Result<String, HtmlError> {
    // Single injection contract: the entire envelope becomes the payload.
    // Compact (not pretty) — the page is machine-consumed by the JS, and a
    // compact island keeps the file small on large corpora.
    let payload = sanitize_island(&serde_json::to_string(envelope)?);
    let view = build_view(envelope, payload);
    let mut out = view.render()?;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// Neutralize a serialized JSON payload for embedding inside a
/// `<script type="application/json">` island.
///
/// The one break-out vector that matters: a literal `</script>` sequence
/// inside a JSON string would prematurely close the host `<script>`
/// element (the HTML tokenizer scans for `</script` regardless of the
/// `type` attribute). We rewrite every `</` to `<\/`. `\/` is a legal JSON
/// string escape that parses back to `/`, so the round-tripped data is
/// byte-for-byte identical after `JSON.parse`, while the embedded byte
/// stream can never contain `</script>`. An HTML comment opener (`<!--`)
/// cannot terminate a `<script>` element, so no further rewrite is needed.
fn sanitize_island(json: &str) -> String {
    json.replace("</", "<\\/")
}

/// Assemble the HTML view-model from `envelope` + the pre-serialized
/// `payload`.
///
/// The `<noscript>` overview reuses [`group_and_sort_by_tier`] — the single
/// cross-reporter grouping pass — so the per-tier counts match the text and
/// markdown surfaces exactly. The interactive views are NOT server-rendered;
/// the JS builds them from the injected payload.
fn build_view(envelope: &Envelope, payload: String) -> HtmlReport {
    let groups = group_and_sort_by_tier(&envelope.result);
    let total_matches: usize = groups.values().map(Vec::len).sum();

    let mut worst_overall: Option<f64> = None;
    let tiers: Vec<TierView> = groups
        .into_iter()
        .map(|(tier, bucket)| {
            // Buckets arrive sorted by score DESC, so the first element is
            // the worst (highest) score in the tier.
            let worst = bucket.first().map_or(0.0, |m| m.score);
            worst_overall = Some(worst_overall.map_or(worst, |w| w.max(worst)));
            TierView {
                tier,
                count: bucket.len(),
                worst,
            }
        })
        .collect();

    HtmlReport {
        title: envelope
            .title
            .clone()
            .unwrap_or_else(|| "Duplication Report".to_string()),
        subtitle: envelope.subtitle.clone(),
        total_matches,
        total_forms: envelope.result.summary.total_forms,
        worst_overall: worst_overall.unwrap_or(0.0),
        tiers,
        payload,
    }
}

/// Top-level HTML view-model. Carries the pre-serialized JSON `payload`
/// (the single `{{ payload }}` injection) plus a small server-rendered
/// `<noscript>` overview (per-tier counts). The template owns all CSS / JS /
/// layout; this struct supplies only semantic data.
#[derive(Template)]
#[template(path = "html_report.html")]
struct HtmlReport {
    /// Page + overview heading. Falls back to "Duplication Report" when
    /// `[output].title` (and the `Envelope.title` echo) is unset.
    title: String,
    /// Optional second header line, from `Envelope.subtitle`.
    subtitle: Option<String>,
    /// Total matches across all tiers — the `<noscript>` headline figure.
    total_matches: usize,
    /// Total forms surveyed this run (`result.summary.total_forms`).
    total_forms: u32,
    /// Worst (highest) score across all matches; `0.0` when empty.
    worst_overall: f64,
    /// One entry per non-empty tier, canonical order — the `<noscript>`
    /// per-tier table rows.
    tiers: Vec<TierView>,
    /// The serialized [`Envelope`] injected into `#dry-data`. This is the
    /// SOLE `|safe` (raw) injection in the template — every other variable
    /// (`title`, `subtitle`, the `<noscript>` fields) is auto-escaped by
    /// askama's default HTML escaping, so config-sourced `title` /
    /// `subtitle` (echoed from the analyzed repo's `dry.toml`, untrusted)
    /// cannot inject markup. `payload` is marked `|safe` because it is
    /// already pre-sanitized by [`sanitize_island`] (which rewrites `</` to
    /// `<\/`, neutralizing the `</script>` break-out) AND because
    /// HTML-escaping JSON would corrupt it — the value lives inside a
    /// `type="application/json"` island the browser parses, not executes.
    payload: String,
}

/// One tier's `<noscript>` summary row — the tier (for the template's emoji
/// + label mapping), its match count, and its worst score.
struct TierView {
    /// The routing tier; the template derives its emoji + label via
    /// [`Tier::as_str`].
    tier: Tier,
    /// Number of matches in this tier.
    count: usize,
    /// Worst (highest) score in this tier.
    worst: f64,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::adapters::reporters::json::{
        Capabilities, EnvelopeMeta, LANGUAGE_RUST, Mode, THRESHOLD_MODE_DEFAULT, TOOL_NAME_DRY4RS,
    };
    use crate::domain::{
        FilePath, FormKind, FormRef, LineColumn, Match, Report, Span, Summary, Tier,
    };

    fn fixed_meta() -> EnvelopeMeta {
        EnvelopeMeta::new(
            TOOL_NAME_DRY4RS.into(),
            "0.1.0".into(),
            LANGUAGE_RUST.into(),
            "2026-05-24T22:00:00Z".into(),
            THRESHOLD_MODE_DEFAULT.into(),
        )
    }

    fn form_ref(path: &str, line: u32) -> FormRef {
        FormRef::new(
            FilePath::from(PathBuf::from(path)),
            Span::try_new(LineColumn::new(line, 0), LineColumn::new(line + 2, 12)).unwrap(),
            FormKind::Production,
        )
    }

    fn report_with_matches() -> Report {
        let auto = Match::new(
            vec![form_ref("src/a.rs", 10), form_ref("src/b.rs", 100)],
            0.97,
            Tier::AutoRefactor,
        );
        let review = Match::new(
            vec![form_ref("src/c.rs", 20), form_ref("src/d.rs", 200)],
            0.88,
            Tier::ReviewFirst,
        );
        let mut by_tier = BTreeMap::new();
        by_tier.insert(Tier::AutoRefactor, 1);
        by_tier.insert(Tier::ReviewFirst, 1);
        let mut by_kind = BTreeMap::new();
        by_kind.insert(FormKind::Production, 2);
        let summary = Summary {
            total_forms: 4,
            by_tier,
            by_kind,
        };
        Report::new(vec![auto, review], summary, false)
    }

    #[test]
    fn render_emits_single_file_shell() {
        let env = Envelope::new(report_with_matches(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::report());
        let html = render(&env).unwrap();
        assert!(html.starts_with("<!doctype html>") || html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<style>"), "inline CSS expected");
        assert!(
            html.contains("type=\"module\""),
            "inline ES-module script expected"
        );
        assert!(html.ends_with('\n'), "POSIX trailing newline: {html:?}");
    }

    #[test]
    fn render_injects_data_island_with_valid_json() {
        let env = Envelope::new(report_with_matches(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::report());
        let html = render(&env).unwrap();
        assert!(
            html.contains("id=\"dry-data\""),
            "data island must be present: {html}"
        );
        // Extract the island payload and confirm it parses + carries the
        // envelope's result.
        let open = "type=\"application/json\">";
        let start = html.find(open).expect("island open tag") + open.len();
        let end = start + html[start..].find("</script>").expect("island close");
        let payload = &html[start..end];
        let parsed: serde_json::Value =
            serde_json::from_str(payload).expect("island payload must be valid JSON");
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["mode"], "report");
        assert_eq!(parsed["result"]["matches"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["capabilities"]["overview"], true);
        assert_eq!(parsed["capabilities"]["substitution_grid"], false);
    }

    #[test]
    fn render_escapes_untrusted_title_and_subtitle() {
        // SECURITY REGRESSION (Qodo XSS): `title` / `subtitle` echo the
        // analyzed repo's `[output].title` / `[output].subtitle` from its
        // `dry.toml` — attacker-controllable when analyzing an untrusted
        // repo. They MUST be HTML-escaped (askama default) so a config like
        // `title = "<script>alert(1)</script>"` cannot inject markup into
        // the generated report (critical for the CI-artifact / Pages REPORT
        // surface). Only the pre-sanitized JSON payload is `|safe`.
        let mut env = Envelope::new(report_with_matches(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::report());
        env.title = Some("<script>alert(1)</script>".to_string());
        env.subtitle = Some("<b>xss</b>".to_string());
        let html = render(&env).unwrap();

        // The `<` / `>` of the untrusted title + subtitle are entity-encoded
        // by askama's default HTML escaping (askama 0.16 emits NUMERIC
        // entities — `&#60;` / `&#62;` — which render as inert text exactly
        // like `&lt;` / `&gt;`). Assert the markup-significant chars are
        // encoded in BOTH the <title> and the <h1>/subtitle positions.
        let lt = "&#60;"; // `<`
        let gt = "&#62;"; // `>`
        assert!(
            html.contains(&format!("{lt}script{gt}alert(1){lt}/script{gt}")),
            "title must be HTML-escaped: {html}"
        );
        assert!(
            html.contains(&format!("{lt}b{gt}xss{lt}/b{gt}")),
            "subtitle must be HTML-escaped: {html}"
        );
        // No ACTIVE markup may appear in the rendered document. The full
        // `<script>…</script>` / `<b>…</b>` pairs must be absent: the
        // template positions render them entity-encoded, and the JSON
        // island's copy has its `</script>` neutralized to `<\/script>` by
        // `sanitize_island`, so the closing-tag form never appears raw.
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "raw <script>…</script> must never appear: {html}"
        );
        assert!(
            !html.contains("<b>xss</b>"),
            "raw <b>…</b> must never appear: {html}"
        );

        // The `|safe` payload still round-trips as valid raw JSON.
        let open = "type=\"application/json\">";
        let start = html.find(open).expect("island open tag") + open.len();
        let end = start + html[start..].find("</script>").expect("island close");
        let parsed: serde_json::Value =
            serde_json::from_str(&html[start..end]).expect("payload must be valid JSON");
        assert_eq!(parsed["schema_version"], 1);
        // The untrusted title survives in the JSON payload as data (the JS
        // reads it via JSON.parse + esc() before any DOM write).
        assert_eq!(parsed["title"], "<script>alert(1)</script>");
    }

    #[test]
    fn render_has_overview_and_cluster_dom_hooks() {
        let env = Envelope::new(report_with_matches(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::report());
        let html = render(&env).unwrap();
        // DOM containers the JS mounts into.
        assert!(html.contains("id=\"overview\""), "{html}");
        assert!(html.contains("id=\"clusters\""), "{html}");
    }

    #[test]
    fn render_noscript_overview_uses_shared_grouping() {
        let env = Envelope::new(report_with_matches(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::report());
        let html = render(&env).unwrap();
        assert!(html.contains("<noscript>"), "noscript fallback expected");
        // Tier labels come from the shared `Tier::as_str` vocabulary.
        assert!(html.contains("auto_refactor"), "{html}");
        assert!(html.contains("review_first"), "{html}");
    }

    #[test]
    fn render_degrades_when_mode_and_scope_absent() {
        // Constructor-path envelope: mode / capabilities / scope all None.
        // The reporter MUST still render (the JS treats absent optionals as
        // defaults) and the payload omits the absent keys.
        let env = Envelope::new(report_with_matches(), fixed_meta());
        let html = render(&env).unwrap();
        assert!(html.contains("id=\"dry-data\""), "{html}");
        let open = "type=\"application/json\">";
        let start = html.find(open).expect("island open tag") + open.len();
        let end = start + html[start..].find("</script>").expect("island close");
        let payload = &html[start..end];
        let parsed: serde_json::Value = serde_json::from_str(payload).expect("valid JSON");
        assert!(parsed.get("mode").is_none(), "mode omitted when None");
        assert!(parsed.get("scope").is_none(), "scope omitted when None");
        assert!(
            parsed.get("capabilities").is_none(),
            "capabilities omitted when None"
        );
    }

    #[test]
    fn render_empty_report_is_clean() {
        let env = Envelope::new(Report::empty_passed(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::report());
        let html = render(&env).unwrap();
        assert!(html.contains("id=\"dry-data\""), "{html}");
        assert!(html.contains("id=\"overview\""), "{html}");
        // No matches -> empty matches array in the payload.
        let open = "type=\"application/json\">";
        let start = html.find(open).expect("island open tag") + open.len();
        let end = start + html[start..].find("</script>").expect("island close");
        let parsed: serde_json::Value =
            serde_json::from_str(&html[start..end]).expect("valid JSON");
        assert_eq!(parsed["result"]["matches"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn sanitize_island_neutralizes_close_script_and_round_trips() {
        // A file path containing `</script>` (pathological but possible in
        // a corpus) must not break the island. After sanitization the byte
        // stream has no `</script>`, yet JSON.parse-equivalent recovers the
        // original value.
        let raw = r#"{"file":"a</script>b"}"#;
        let safe = sanitize_island(raw);
        assert!(
            !safe.contains("</script>"),
            "must not contain raw close tag"
        );
        assert!(!safe.contains("</"), "all </ rewritten: {safe}");
        let parsed: serde_json::Value = serde_json::from_str(&safe).expect("still valid JSON");
        assert_eq!(parsed["file"], "a</script>b", "value round-trips");
    }

    #[test]
    fn render_no_ansi_escape_bytes() {
        let env = Envelope::new(report_with_matches(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::report());
        let html = render(&env).unwrap();
        assert!(
            !html.bytes().any(|b| b == 0x1B),
            "html reporter must not emit ANSI"
        );
    }
}
