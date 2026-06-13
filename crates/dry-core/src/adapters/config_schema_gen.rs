//! JSON Schema emitter for `<adapter>.schema.json` (dry-rs#78).
//!
//! Walks the same annotated [`Config`] schema as
//! [`crate::adapters::config_doc_gen`] and emits a JSON Schema
//! document via [`schemars::schema_for`]. Single source of truth =
//! the `///` doc comments on [`crate::domain::config`] field
//! declarations plus the `#[derive(JsonSchema)]` attribute.
//!
//! Consumer surface — `$schema`-aware editors (`VS Code`, `IntelliJ`
//! family, etc.) load the emitted file via a `# :schema` directive at
//! the top of `dry.toml` (or via the IDE-side schema mapping) and
//! produce autocomplete + inline validation against the live schema.
//!
//! Per the cross-tool config-file ADR (`adr-config-file-pattern.md`)
//! this lives in `adapters/` alongside the loader + the example
//! emitter — all three consume the domain POD types and provide the
//! I/O surface. The "documentation rots; CI doesn't" rule
//! (see memory `feedback_documentation-rots-ci-doesnt`) is the
//! load-bearing reason — the byte-identical sync test in
//! `crates/dry4rs/tests/dry_schema_sync.rs` keeps the committed
//! `dry.schema.json` aligned with the live schemars output.

use schemars::schema_for;

use crate::cli::AdapterMeta;
use crate::domain::Config;

/// Render the JSON schema for the unified [`Config`] to a
/// pretty-printed string. `meta` is reserved for future schema
/// header / `$id` use; the v0.1 implementation derives the schema
/// directly from the `JsonSchema` impl.
///
/// schemars 1.x outputs deterministic JSON (sorted `$defs`, fixed
/// key order on each schema object) so successive invocations
/// produce byte-identical results — the load-bearing property the
/// sync test relies on.
///
/// # Panics
///
/// Panics if `serde_json::to_string_pretty` fails on the schemars-
/// emitted schema — impossible by construction since schemars only
/// emits valid `serde_json::Value` shapes.
#[must_use]
pub fn render_json_schema(meta: &AdapterMeta) -> String {
    // `meta` is plumbed through for forward-compat (future schema
    // header naming the adapter binary, `$id` carrying a permanent
    // URL, etc.). v0.1 derives the schema purely from the annotated
    // Config; the binding ensures the public signature stays stable
    // as those header / $id fields land.
    let _ = meta;
    let schema = schema_for!(Config);
    serde_json::to_string_pretty(&schema)
        .expect("schemars output is always valid JSON by construction")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Language;

    const TEST_META: AdapterMeta = AdapterMeta {
        tool_name: "test-adapter",
        display_name: "TestLang",
        tool_version: "0.0.0",
        long_version: "0.0.0",
        about: "test about",
        long_about: "test long about",
        after_help: "",
        config_file_name: "test-adapter.toml",
        example_file_name: "test-adapter.example.toml",
        schema_file_name: "test-adapter.schema.json",
        extensions: &["rs"],
        language: Language::Rust,
        tool_info_uri: "https://example.test/info",
        rule_help_uri: "https://example.test/rules",
        default_excludes: &[],
        forced_excludes: &[],
    };

    #[test]
    fn render_json_schema_emits_top_level_keys() {
        let out = render_json_schema(&TEST_META);
        // Title — schemars derives this from the type name.
        assert!(
            out.contains("\"title\": \"Config\""),
            "missing title:\n{out}"
        );
        // Root $schema reference.
        assert!(out.contains("\"$schema\""), "missing $schema declaration");
        // Reusable subschema bucket.
        assert!(out.contains("\"$defs\""), "missing $defs bucket");
    }

    #[test]
    fn render_json_schema_references_every_subschema() {
        let out = render_json_schema(&TEST_META);
        for ty in [
            "GateConfig",
            "OutputConfig",
            "WalkConfig",
            "ScopeConfig",
            "LanguageConfig",
            "Format",
            "ThresholdMode",
        ] {
            assert!(
                out.contains(ty),
                "schema must reference subschema `{ty}` in $defs:\n{out}"
            );
        }
    }

    #[test]
    fn render_json_schema_is_deterministic() {
        let a = render_json_schema(&TEST_META);
        let b = render_json_schema(&TEST_META);
        assert_eq!(a, b, "schema emitter output must be byte-stable");
    }

    #[test]
    fn render_json_schema_pins_additional_properties_false() {
        // serde(deny_unknown_fields) on Config + every sub-table
        // becomes `"additionalProperties": false` in the emitted
        // schema. This is how `$schema`-aware editors surface typos
        // inline.
        let out = render_json_schema(&TEST_META);
        assert!(
            out.contains("\"additionalProperties\": false"),
            "schema must carry additionalProperties:false from deny_unknown_fields"
        );
    }
}
