//! Starship-style annotated example emitter for `<adapter>.example.toml`.
//!
//! Walks the [`Config`] schema's `///` doc comments via
//! [`DocumentedFields`] and emits a fully-annotated TOML reference.
//! [`toml::to_string_pretty`] handles value serialization;
//! [`toml_edit::DocumentMut`] carries per-key leading comments via
//! `Key::leaf_decor_mut().set_prefix`.
//!
//! Single source of truth = the `///` doc comments on
//! [`crate::domain::config`] field declarations. A sync test
//! (`dry-core/tests/doc_gen_sync.rs`) asserts the committed
//! `<tool>.example.toml` is byte-identical to
//! [`render_example_config`]'s output for the production
//! [`AdapterMeta`]. Adding a new field to any config struct breaks
//! the build inside `build_exhaustive_example_config` (compile-time
//! exhaustive destructure) until both the emitter is updated AND the
//! example regenerated. The bytes-identical sync test then fails loud
//! until the committed file is refreshed.
//!
//! Per the cross-tool config-file ADR (`adr-config-file-pattern.md`),
//! this lives in `adapters/` alongside the loader — both consume the
//! domain POD types and provide the I/O surface. The
//! "documentation rots; CI doesn't" rule (see memory
//! `feedback_documentation-rots-ci-doesnt`) is the load-bearing
//! reason this exists.

use documented::DocumentedFields;
use toml_edit::{DocumentMut, Table};

use crate::cli::{AdapterMeta, Format, ThresholdMode};
use crate::domain::config::{Config, GateConfig, OutputConfig, WalkConfig};

/// Renders the exhaustive annotated `<adapter>.example.toml` reference.
///
/// The output is what `init` writes to disk; a sync test asserts the
/// committed file is byte-identical to this function's output for the
/// production [`AdapterMeta`].
///
/// `meta` supplies the tool name (header / regen instructions), the
/// example file name (header), the config file name (header contrast
/// — "this is not the loaded config"), and the default extension
/// list (rendered into `[walk].extensions`).
///
/// # Compile-time guards
///
/// Adding a field to any config struct breaks the build at
/// `build_exhaustive_example_config`'s struct construction
/// (exhaustive — no `..`). The corresponding `annotate_*_table`
/// helper also fails (its destructure of the struct's
/// [`Default`] value mirrors the schema shape). Both are intentional
/// — the multi-point compile error makes it impossible to land a
/// schema change without updating the emitter.
///
/// # Panics
///
/// Panics if `toml::to_string_pretty` cannot serialize the
/// exhaustive [`Config`] (impossible — every field has a
/// `Serialize` impl; the call exists to surface a programming
/// error if a future field is added with a non-serializable type)
/// or if the serde-emitted TOML fails to round-trip through
/// `toml_edit` (also impossible by construction — both crates
/// share the canonical TOML 1.0 grammar). Both `.expect()` sites
/// surface programmer-error conditions, NOT user-input conditions.
#[must_use]
pub fn render_example_config(meta: &AdapterMeta) -> String {
    let example = build_exhaustive_example_config(meta);

    let body =
        toml::to_string_pretty(&example).expect("Config is always serde-serializable to TOML");
    let mut doc: DocumentMut = body
        .parse()
        .expect("serde-emitted TOML round-trips through toml_edit");

    doc.decor_mut().set_prefix(header_comment(meta));

    annotate_gate_table(&mut doc);
    annotate_output_table(&mut doc);
    annotate_walk_table(&mut doc);

    doc.to_string()
}

/// Builds the exhaustive example [`Config`]: every `Option` = `Some`,
/// every collection non-empty.
///
/// The struct construction below is **exhaustive** (no `..`) — adding
/// a field to [`Config`], [`GateConfig`], [`OutputConfig`], or
/// [`WalkConfig`] breaks this fn's compile until the new field is
/// wired in with a concrete example value. That's the load-bearing
/// rot-prevention guard.
fn build_exhaustive_example_config(meta: &AdapterMeta) -> Config {
    let gate = GateConfig {
        threshold: Some(0.85),
        threshold_mode: Some(ThresholdMode::Default),
    };
    let output = OutputConfig {
        format: Some(Format::Text),
    };
    let walk = WalkConfig {
        include_ignored: Some(false),
        extensions: Some(meta.extensions_owned()),
    };
    Config { gate, output, walk }
}

/// File-level header comment with regen instructions.
fn header_comment(meta: &AdapterMeta) -> String {
    format!(
        "# {file} — exhaustive annotated reference for the {tool} config schema.\n\
         #\n\
         # Generated deterministically by `{tool} init` from the schema's\n\
         # `///` doc comments. To regenerate after a schema change:\n\
         #\n\
         #     {tool} init --force\n\
         #\n\
         # A sync test keeps this file byte-identical to the emitter's\n\
         # output. Adding a config field breaks the build at\n\
         # `dry_core::adapters::config_doc_gen::build_exhaustive_example_config`\n\
         # (compile-time exhaustive destructure) until both the emitter\n\
         # is updated and this file is regenerated.\n\
         #\n\
         # Distinct from the minimal `{config}` that {tool} actually\n\
         # loads: this file is the canonical option reference, NOT\n\
         # user config.\n\
         #\n\n",
        file = meta.example_file_name,
        tool = meta.tool_name,
        config = meta.config_file_name,
    )
}

/// Attaches doc-comment prefixes to `[gate]` and its fields.
///
/// The exhaustive destructure of a default [`GateConfig`] inside is
/// the compile-time guard against adding a field without wiring its
/// annotation: a new field on [`GateConfig`] fails to match this
/// pattern.
fn annotate_gate_table(doc: &mut DocumentMut) {
    let GateConfig {
        threshold: _,
        threshold_mode: _,
    } = GateConfig::default();

    let prefix = section_prefix::<Config>("gate");
    let table = doc["gate"]
        .as_table_mut()
        .expect("toml_edit emits [gate] as a table");
    table.decor_mut().set_prefix(prefix);

    attach_field_doc::<GateConfig>(table, "threshold");
    attach_field_doc::<GateConfig>(table, "threshold_mode");
}

/// Attaches doc-comment prefixes to `[output]` and its fields.
///
/// Compile-time guard mirrors [`annotate_gate_table`].
fn annotate_output_table(doc: &mut DocumentMut) {
    let OutputConfig { format: _ } = OutputConfig::default();

    let prefix = section_prefix::<Config>("output");
    let table = doc["output"]
        .as_table_mut()
        .expect("toml_edit emits [output] as a table");
    table.decor_mut().set_prefix(prefix);

    attach_field_doc::<OutputConfig>(table, "format");
}

/// Attaches doc-comment prefixes to `[walk]` and its fields.
///
/// Compile-time guard mirrors [`annotate_gate_table`].
fn annotate_walk_table(doc: &mut DocumentMut) {
    let WalkConfig {
        include_ignored: _,
        extensions: _,
    } = WalkConfig::default();

    let prefix = section_prefix::<Config>("walk");
    let table = doc["walk"]
        .as_table_mut()
        .expect("toml_edit emits [walk] as a table");
    table.decor_mut().set_prefix(prefix);

    attach_field_doc::<WalkConfig>(table, "include_ignored");
    attach_field_doc::<WalkConfig>(table, "extensions");
}

/// Builds the leading comment block for a top-level table section,
/// sourced from `Config`'s field-level doc on the section's field.
fn section_prefix<F: DocumentedFields>(field_name: &str) -> String {
    let docs = F::get_field_docs(field_name)
        .expect("config schema fields have doc comments by construction");
    format!("\n{}", comment_block(docs))
}

/// Attaches the field-level doc comment as a leading comment block
/// on the key.
fn attach_field_doc<F: DocumentedFields>(table: &mut Table, field_name: &str) {
    let docs = F::get_field_docs(field_name).expect("field documented by DocumentedFields derive");
    let prefix = comment_block(docs);
    let mut key = table
        .key_mut(field_name)
        .expect("field present in serialized output");
    key.leaf_decor_mut().set_prefix(prefix);
}

/// Wraps a multi-line doc-comment string into a TOML comment block.
/// Each non-empty line gets a leading `# `; blank lines become `#`.
fn comment_block(docs: &str) -> String {
    let mut out = String::new();
    for line in docs.lines() {
        if line.is_empty() {
            out.push_str("#\n");
        } else {
            out.push_str("# ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
        extensions: &["rs"],
        tool_info_uri: "https://example.test/info",
        rule_help_uri: "https://example.test/rules",
        default_excludes: &[],
        forced_excludes: &[],
    };

    #[test]
    fn render_example_config_emits_every_section() {
        let out = render_example_config(&TEST_META);
        assert!(out.contains("[gate]"), "missing [gate] section");
        assert!(out.contains("[output]"), "missing [output] section");
        assert!(out.contains("[walk]"), "missing [walk] section");
    }

    #[test]
    fn render_example_config_emits_every_field() {
        let out = render_example_config(&TEST_META);
        for field in [
            "threshold",
            "threshold_mode",
            "format",
            "include_ignored",
            "extensions",
        ] {
            assert!(out.contains(field), "missing field `{field}`");
        }
    }

    #[test]
    fn render_example_config_attaches_field_doc_comments() {
        let out = render_example_config(&TEST_META);
        assert!(
            out.contains("# Jaccard similarity threshold."),
            "threshold field's doc comment missing from output:\n{out}"
        );
        assert!(
            out.contains("# Output format (`text` / `json`)"),
            "format field's doc comment missing from output:\n{out}"
        );
    }

    #[test]
    fn render_example_config_header_names_the_tool() {
        let out = render_example_config(&TEST_META);
        assert!(
            out.starts_with("# test-adapter.example.toml"),
            "header missing or wrong:\n{}",
            out.lines().next().unwrap_or("(empty)")
        );
        assert!(
            out.contains("test-adapter init"),
            "regen instructions don't name the tool"
        );
    }

    #[test]
    fn render_example_config_round_trips_with_every_option_some() {
        let out = render_example_config(&TEST_META);
        let parsed: Config = toml::from_str(&out).expect("annotated output parses as a Config");

        assert!(
            parsed.gate.threshold.is_some(),
            "gate.threshold must be Some in exhaustive example"
        );
        assert!(
            parsed.gate.threshold_mode.is_some(),
            "gate.threshold_mode must be Some"
        );
        assert!(parsed.output.format.is_some(), "output.format must be Some");
        assert!(
            parsed.walk.include_ignored.is_some(),
            "walk.include_ignored must be Some"
        );
        let exts = parsed
            .walk
            .extensions
            .as_ref()
            .expect("walk.extensions must be Some");
        assert!(!exts.is_empty(), "walk.extensions must be non-empty");
    }

    #[test]
    fn render_example_config_is_deterministic() {
        let a = render_example_config(&TEST_META);
        let b = render_example_config(&TEST_META);
        assert_eq!(a, b, "emitter output must be byte-stable");
    }
}
