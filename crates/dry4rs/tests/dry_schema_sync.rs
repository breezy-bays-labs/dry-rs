//! Sync test for the committed workspace-root `dry.schema.json`
//! against the live schemars emitter (dry-rs#78).
//!
//! Single assertion, load-bearing:
//!
//! - `committed_matches_emitter_output_byte_for_byte` — the file at
//!   workspace root is byte-identical to
//!   `render_json_schema(&DRY4RS_META)`. The committed file IS the
//!   function's output by construction; if a schema change drifts
//!   the emitter, this test fails loud and surfaces the regen
//!   command.
//!
//! Mirrors `dry_example_sync.rs` (dry-rs#77) — the same
//! "documentation rots; CI doesn't" rule applies to the JSON
//! schema artifact. Both files are regenerated together via
//! `cargo run -p dry4rs --release -- init --force` per the
//! CONTRIBUTING.md schema discipline section.

use dry4rs::DRY4RS_META;
use dry4rs::adapters::config_schema_gen::render_json_schema;

/// The committed workspace-root schema, included at build time.
///
/// Path is relative to this file: walk up from `crates/dry4rs/tests/`
/// to workspace root.
const COMMITTED_SCHEMA: &str = include_str!("../../../dry.schema.json");

#[test]
fn committed_matches_emitter_output_byte_for_byte() {
    let emitted = render_json_schema(&DRY4RS_META);
    if COMMITTED_SCHEMA != emitted {
        panic!(
            "`dry.schema.json` is out of sync with the schemars emitter.\n\
             \n\
             Regenerate from the workspace root:\n\
             \n\
             \u{20}\u{20}\u{20}\u{20}cargo run -p dry4rs --release -- init --force\n\
             \n\
             This regenerates BOTH `dry.example.toml` AND \
             `dry.schema.json` together. Commit both files in the \
             same PR.\n\
             \n\
             Diff (committed vs. emitter):\n\
             ---\n\
             Committed ({} bytes):\n{committed}\n\
             ---\n\
             Emitter ({} bytes):\n{emitted}\n",
            COMMITTED_SCHEMA.len(),
            emitted.len(),
            committed = COMMITTED_SCHEMA,
            emitted = emitted,
        );
    }
}

#[test]
fn committed_schema_carries_expected_top_level_shape() {
    // Belt-and-suspenders structural check: even when the bytes
    // match, verify the published schema carries the JSON-schema
    // top-level keys consumers ($schema-aware editors) parse.
    assert!(
        COMMITTED_SCHEMA.contains("\"$schema\""),
        "committed schema must declare $schema"
    );
    assert!(
        COMMITTED_SCHEMA.contains("\"title\": \"Config\""),
        "committed schema must title the root type as `Config`"
    );
    assert!(
        COMMITTED_SCHEMA.contains("\"$defs\""),
        "committed schema must carry a $defs bucket for subschemas"
    );
    assert!(
        COMMITTED_SCHEMA.contains("LanguageConfig"),
        "committed schema must include LanguageConfig in $defs (dry-rs#78)"
    );
    assert!(
        COMMITTED_SCHEMA.contains("\"additionalProperties\": false"),
        "committed schema must enforce additionalProperties:false (deny_unknown_fields)"
    );
}
