//! HTML reporter — a self-contained single-file vanilla explorer
//! (dry-rs#147, epic #111).
//!
//! Renders the duplication report as ONE standalone `.html` file with no
//! framework, no build step, and no external assets. The whole [`Envelope`]
//! is serialized to JSON, **base64-encoded**, and injected ONCE into a
//! `<script id="dry-data" type="application/json">…</script>` island; the
//! page's inline ES-module `<script>` reads `#dry-data`, base64-decodes it,
//! and renders the interactive views (overview, cluster list, cluster
//! detail, tier/score filters) client-side. The inline `<style>` block
//! carries all CSS.
//!
//! This is the reference frontend — Claude Design polishes later. PR13
//! shipped the bare REPORT reporter (overview + cluster views); the SHOWCASE
//! (dry-rs#149) wires the remaining views the backing wire fields enable:
//! the per-cluster anti-unification **template skeleton** (the
//! [`crate::domain::TemplateNode`] tree with numbered hole markers), the
//! **substitution grid** (one row per member, one column per hole, variadic
//! cells holding multiple lexemes), the **d-slider** (monotonically hides
//! holes whose `divergence.differing < d`), and the read-only **scope
//! banner** (from `Envelope.scope`, graying the crate axes when
//! `crate_aware == false`). The frontend degrades gracefully when `template`
//! / `scope` / `mode` / `capabilities` are absent — an exact-dup cluster
//! (no holes / null template) shows the concrete shared form and no empty
//! grid, and a missing optional field never throws.
//!
//! ## Single injection contract — base64 island, default escaping
//!
//! There is exactly ONE template interpolation of report data — the
//! base64-encoded envelope (`payload_b64`). Base64's alphabet
//! (`[A-Za-z0-9+/=]`) contains zero HTML-special characters, so askama's
//! DEFAULT escaping is a byte-level no-op and NO escape-bypass filter is
//! needed: the island can never carry a close-script break-out, and there is
//! no raw-injection surface for a code-review bot to flag. Every OTHER
//! template variable (`title`, `subtitle`, the noscript fields) is
//! auto-escaped by askama's default — so config-sourced `title` / `subtitle`
//! (echoed from the analyzed repo's `dry.toml`, untrusted) cannot inject
//! markup. Presentation is entirely client-side: the JS base64-decodes the
//! island, reads `result.*` (the truthful gate, immune to view-shaping
//! flags), and derives everything else. The HTML body stays a small, stable
//! shell regardless of corpus size.
//!
//! ## Client-side rendering uses DOM construction, not `innerHTML`
//!
//! The inline JS builds the overview + cluster cards with DOM-API element
//! construction (`document.createElement` + `textContent` + `append` via a
//! tiny `h(...)` helper) — NO `innerHTML` / `outerHTML` / `document.write`
//! anywhere. All text flows through `createTextNode` / `textContent`, which
//! cannot interpret markup, so user-controlled values (file paths, the
//! `title` carried in the payload, etc.) are inert by construction — no
//! manual `esc()` discipline to get wrong. This keeps the client surface
//! XSS-free at the root, not by sanitizing-after-the-fact.
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
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
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
    // Single injection contract: the entire envelope is serialized to JSON
    // (compact — the page is machine-consumed by the JS) and base64-encoded.
    // Base64's `[A-Za-z0-9+/=]` alphabet has zero HTML-special chars, so the
    // island injects with askama's DEFAULT escaping (a no-op) — no
    // escape-bypass filter, no close-script break-out, no raw-injection surface.
    let payload_b64 = BASE64.encode(serde_json::to_string(envelope)?);
    let view = build_view(envelope, payload_b64);
    let mut out = view.render()?;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// Assemble the HTML view-model from `envelope` + the base64-encoded
/// `payload_b64`.
///
/// The `<noscript>` overview reuses [`group_and_sort_by_tier`] — the single
/// cross-reporter grouping pass — so the per-tier counts match the text and
/// markdown surfaces exactly. The interactive views are NOT server-rendered;
/// the JS base64-decodes the island and builds them from the payload.
fn build_view(envelope: &Envelope, payload_b64: String) -> HtmlReport {
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
        payload_b64,
    }
}

/// Top-level HTML view-model. Carries the base64-encoded JSON `payload_b64`
/// (the single `{{ payload_b64 }}` injection) plus a small server-rendered
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
    /// The base64-encoded serialized [`Envelope`] injected into `#dry-data`.
    /// Injected with askama's DEFAULT escaping (no escape-bypass filter):
    /// base64's `[A-Za-z0-9+/=]` alphabet has zero HTML-special chars, so
    /// escaping is a byte-level no-op and the island can never carry a
    /// close-script break-out. Every OTHER template variable (`title`,
    /// `subtitle`, the noscript fields) is likewise auto-escaped, so
    /// config-sourced `title` / `subtitle` (echoed from the analyzed repo's
    /// `dry.toml`, untrusted) cannot inject markup. The JS base64-decodes
    /// this island (`atob` + `TextDecoder`, UTF-8-safe) and parses the JSON
    /// client-side.
    payload_b64: String,
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

    /// Extract the `#dry-data` island text from rendered HTML.
    fn island_b64(html: &str) -> &str {
        let open = "type=\"application/json\">";
        let start = html.find(open).expect("island open tag") + open.len();
        let end = start + html[start..].find("</script>").expect("island close");
        &html[start..end]
    }

    /// Decode the base64 island back into the parsed JSON envelope.
    fn decode_island(html: &str) -> serde_json::Value {
        let b64 = island_b64(html).trim();
        let bytes = BASE64.decode(b64).expect("island must be valid base64");
        let json = String::from_utf8(bytes).expect("decoded island must be UTF-8");
        serde_json::from_str(&json).expect("decoded island must be valid JSON")
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
        // The island is base64 — its alphabet has NO `<`, so no `</script>`
        // break-out is structurally possible.
        let raw_island = island_b64(&html);
        assert!(
            !raw_island.contains('<'),
            "base64 island must contain no `<`: {raw_island}"
        );
        // Decode + confirm it carries the envelope.
        let parsed = decode_island(&html);
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
        // surface).
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
        // No ACTIVE markup may appear anywhere in the rendered document. The
        // template positions render the payloads entity-encoded, and the
        // base64 island contains zero `<`, so neither the opening NOR the
        // closing tag form appears raw.
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "raw <script>…</script> must never appear: {html}"
        );
        assert!(
            !html.contains("<b>xss</b>"),
            "raw <b>…</b> must never appear: {html}"
        );

        // The base64 payload decodes back to the envelope; the untrusted
        // title survives there as DATA (the JS decodes + builds DOM via
        // textContent, so it is inert by construction — never markup).
        let parsed = decode_island(&html);
        assert_eq!(parsed["schema_version"], 1);
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
        let parsed = decode_island(&html);
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
        // No matches -> empty matches array in the decoded payload.
        let parsed = decode_island(&html);
        assert_eq!(parsed["result"]["matches"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn island_is_pure_base64_no_break_out_possible() {
        // A file path containing `</script>` (pathological but possible in a
        // corpus) cannot break the island: base64 encoding has no `<` in its
        // alphabet, so a `</script>` in the source data becomes inert base64
        // and still round-trips through decode.
        let mut env = Envelope::new(report_with_matches(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::report());
        env.title = Some("a</script>b".to_string());
        let html = render(&env).unwrap();
        let raw_island = island_b64(&html);
        assert!(
            !raw_island.contains('<'),
            "base64 island must contain no `<`: {raw_island}"
        );
        let parsed = decode_island(&html);
        assert_eq!(parsed["title"], "a</script>b", "value round-trips");
    }

    /// Build a 2-member near-dup match carrying a template with one
    /// `SubExpr` hole (a pure rename: `total` vs `sum`) plus one `Variadic`
    /// hole (member 0 binds two elements, member 1 binds one) — the showcase
    /// data the substitution grid + d-slider render.
    fn match_with_template() -> Match {
        use crate::domain::{
            DistinctValue, Divergence, Hole, HoleId, HoleKind, SubElement, Substitution, Template,
            TemplateNode,
        };
        fn sp(line: u32) -> Span {
            Span::try_new(LineColumn::new(line, 4), LineColumn::new(line, 8)).unwrap()
        }
        let root = TemplateNode::Fixed {
            label: "Block".into(),
            children: vec![
                TemplateNode::Hole(HoleId::new(0)),
                TemplateNode::Fixed {
                    label: "ExprLocal".into(),
                    children: vec![],
                    leaf_lexeme: Some("x".into()),
                },
                TemplateNode::Hole(HoleId::new(1)),
            ],
            leaf_lexeme: None,
        };
        let holes = vec![
            Hole::new(
                HoleId::new(0),
                HoleKind::SubExpr,
                vec![
                    Substitution::new(vec![SubElement::new("total".into(), 111, sp(12))]),
                    Substitution::new(vec![SubElement::new("sum".into(), 222, sp(24))]),
                ],
                Divergence::new(
                    2,
                    1,
                    2,
                    vec![
                        DistinctValue::new(111, "total".into(), 1),
                        DistinctValue::new(222, "sum".into(), 1),
                    ],
                ),
            ),
            Hole::new(
                HoleId::new(1),
                HoleKind::Variadic,
                vec![
                    Substitution::new(vec![
                        SubElement::new("a".into(), 333, sp(13)),
                        SubElement::new("b".into(), 444, sp(13)),
                    ]),
                    Substitution::new(vec![SubElement::new("c".into(), 555, sp(25))]),
                ],
                Divergence::new(3, 2, 2, vec![]),
            ),
        ];
        let template = Template::new(root, holes);
        Match::new(
            vec![form_ref("src/a.rs", 10), form_ref("src/b.rs", 22)],
            0.88,
            Tier::ReviewFirst,
        )
        .with_template(template)
    }

    fn report_with_template() -> Report {
        let mut by_tier = BTreeMap::new();
        by_tier.insert(Tier::ReviewFirst, 1);
        let mut by_kind = BTreeMap::new();
        by_kind.insert(FormKind::Production, 2);
        let summary = Summary {
            total_forms: 2,
            by_tier,
            by_kind,
        };
        Report::new(vec![match_with_template()], summary, false)
    }

    #[test]
    fn showcase_capabilities_ride_the_island() {
        // The showcase HTML path flips every capability flag true — the
        // frontend now renders the template skeleton, substitution grid,
        // d-slider, and scope banner. The flags travel in the payload so the
        // JS can gate its views on them.
        let env = Envelope::new(report_with_template(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::showcase());
        let html = render(&env).unwrap();
        let parsed = decode_island(&html);
        assert_eq!(parsed["capabilities"]["overview"], true);
        assert_eq!(parsed["capabilities"]["clusters"], true);
        assert_eq!(parsed["capabilities"]["substitution_grid"], true);
        assert_eq!(parsed["capabilities"]["d_slider"], true);
        assert_eq!(parsed["capabilities"]["scope_banner"], true);
    }

    #[test]
    fn template_payload_carries_skeleton_holes_and_variadic_cell() {
        // The full template (root skeleton + per-hole substitutions, incl.
        // the variadic member binding two elements) survives base64 + JSON
        // round-trip into the island, so the client JS has everything it
        // needs to draw the skeleton + grid.
        let env = Envelope::new(report_with_template(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::showcase());
        let html = render(&env).unwrap();
        let parsed = decode_island(&html);
        let tmpl = &parsed["result"]["matches"][0]["template"];
        assert_eq!(tmpl["root"]["node"], "fixed");
        assert_eq!(tmpl["holes"].as_array().unwrap().len(), 2);
        // Hole 1 is variadic: member 0 binds two elements.
        let var_member0 = &tmpl["holes"][1]["substitutions"][0]["elements"];
        assert_eq!(var_member0.as_array().unwrap().len(), 2);
        assert_eq!(tmpl["holes"][1]["kind"], "variadic");
        // The reserved score slots are now DERIVED from the template (a
        // pure-rename SubExpr hole lifts structural_score above raw score).
        let m = &parsed["result"]["matches"][0];
        assert!(m["structural_score"].as_f64().unwrap() >= m["score"].as_f64().unwrap());
    }

    #[test]
    fn render_includes_dslider_and_scope_dom_hooks() {
        // The template carries the JS markers the showcase mounts into: the
        // d-slider control and the scope banner. They are CSS / JS surface
        // (the DOM is built client-side), so assert the static template ships
        // the supporting style hooks + the scope-banner section anchor.
        let mut env = Envelope::new(report_with_template(), fixed_meta())
            .with_presentation(Mode::Report, Capabilities::showcase());
        env.scope = Some(crate::adapters::reporters::json::ScopeApplied {
            within_crate: true,
            across_crate: true,
            within_module: true,
            across_module: false,
            crate_aware: false,
        });
        let html = render(&env).unwrap();
        // Scope banner travels in the payload (read client-side).
        let parsed = decode_island(&html);
        assert_eq!(parsed["scope"]["crate_aware"], false);
        assert_eq!(parsed["scope"]["across_module"], false);
        // The template ships the showcase CSS classes the JS attaches.
        assert!(html.contains("skeleton"), "skeleton CSS hook expected");
        assert!(
            html.contains("d-slider") || html.contains("dslider"),
            "{html}"
        );
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
