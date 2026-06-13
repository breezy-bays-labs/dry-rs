//! JSON reporter — renders a [`Report`] as the locked v0.1 wire
//! envelope.
//!
//! Per `ops/decisions/dry-rs/adr-nested-json-envelope.md`, this module
//! owns the construction of the nested envelope:
//!
//! ```jsonc
//! {
//!   "schema_version": 1,
//!   "tool":          "dry4rs",
//!   "tool_version":  "0.1.0",
//!   "language":      "rust",
//!   "timestamp":     "...",
//!   "threshold_mode": "default",
//!   "result":        { /* truthful-gate domain::Report */ },
//!   "view":          { /* shapeable display, omitted at v0.1 */ }
//! }
//! ```
//!
//! The shape is mechanically locked by
//! `crates/dry-core/tests/wire_envelope_snapshot.rs`. Any
//! intentional change must update the snapshot AND bump
//! `schema_version` (additive optional fields excepted — see the ADR's
//! forward-compat table).

mod envelope;

pub use envelope::{
    Envelope, EnvelopeMeta, LANGUAGE_RUST, SCHEMA_VERSION, ScopeApplied, THRESHOLD_MODE_DEFAULT,
    TOOL_NAME_DRY4RS, ViewProjection,
};

use thiserror::Error;

use crate::domain::Report;

/// Errors produced by [`render`].
///
/// `#[non_exhaustive]` — adapter binaries pattern-match defensively;
/// future variants (e.g. a JSON-schema validation failure when the
/// reporter ever round-trips the envelope through a validator) land
/// without breaking call sites.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum JsonError {
    /// `serde_json` failed to serialize the envelope. In practice this
    /// is unreachable — every domain type derives `Serialize` over
    /// owned data; the variant exists for forward-compat.
    #[error("json serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Render `report` as the v0.1 wire envelope, pretty-printed.
///
/// `meta` supplies the timestamp + tool identity threaded in by the
/// adapter binary's run-loop wrapper. The caller-supplied timestamp
/// (NOT pulled from `SystemTime::now()` inside the reporter) is the
/// load-bearing detail that makes the wire-envelope snapshot
/// byte-stable.
///
/// # Errors
///
/// Returns [`JsonError::Serialize`] when `serde_json` fails to
/// serialize the envelope. Domain types are owned + `Serialize`-clean,
/// so this is unreachable in practice; the variant exists for
/// completeness.
pub fn render(report: &Report, meta: EnvelopeMeta) -> Result<String, JsonError> {
    let envelope = Envelope::new(report.clone(), meta);
    Ok(serde_json::to_string_pretty(&envelope)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FilePath, FormKind, FormRef, LineColumn, Match, Span, Summary, Tier};

    fn fixed_meta() -> EnvelopeMeta {
        EnvelopeMeta::new(
            TOOL_NAME_DRY4RS.into(),
            "0.1.0".into(),
            LANGUAGE_RUST.into(),
            "2026-05-24T22:00:00Z".into(),
            THRESHOLD_MODE_DEFAULT.into(),
        )
    }

    #[test]
    fn render_emits_pretty_json_with_locked_top_level_keys() {
        let report = Report::empty_passed();
        let json = render(&report, fixed_meta()).unwrap();
        // Top-level locked keys (ADR §"Top-level envelope"). Pretty
        // print places each on its own indented line; substring match
        // is enough to assert ordering and presence.
        assert!(json.contains("\"schema_version\": 1"), "{json}");
        assert!(json.contains("\"tool\": \"dry4rs\""), "{json}");
        assert!(json.contains("\"tool_version\": \"0.1.0\""), "{json}");
        assert!(json.contains("\"language\": \"rust\""), "{json}");
        assert!(json.contains("\"threshold_mode\": \"default\""), "{json}");
        assert!(
            json.contains("\"timestamp\": \"2026-05-24T22:00:00Z\""),
            "{json}"
        );
        assert!(json.contains("\"result\": {"), "{json}");
    }

    #[test]
    fn render_omits_view_delta_diagnostics_when_none() {
        let json = render(&Report::empty_passed(), fixed_meta()).unwrap();
        assert!(!json.contains("\"view\""), "view must be omitted: {json}");
        assert!(!json.contains("\"delta\""), "delta must be omitted: {json}");
        assert!(
            !json.contains("\"diagnostics\""),
            "diagnostics must be omitted: {json}"
        );
    }

    #[test]
    fn render_serializes_match_with_three_null_reserved_slots() {
        // Load-bearing wire-shape lock: v0.1 Match must serialize three
        // reserved score slots as explicit `null` (NOT omitted), per
        // adr-nested-json-envelope.md "Note on serde attributes".
        let m = Match::new(
            vec![FormRef::new(
                FilePath::from(std::path::PathBuf::from("src/foo.rs")),
                Span::try_new(LineColumn::new(1, 0), LineColumn::new(3, 12)).unwrap(),
                FormKind::Production,
            )],
            0.92,
            Tier::ReviewFirst,
        );
        let report = Report::new(vec![m], Summary::new(), false);
        let json = render(&report, fixed_meta()).unwrap();
        assert!(json.contains("\"structural_score\": null"), "{json}");
        assert!(json.contains("\"rename_count\": null"), "{json}");
        assert!(json.contains("\"rename_density\": null"), "{json}");
    }
}
