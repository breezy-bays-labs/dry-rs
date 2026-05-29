//! Sync + round-trip tests for the committed workspace-root
//! `dry.example.toml` against the live Starship-pattern doc-gen
//! emitter (dry-rs#77).
//!
//! Two assertions, both load-bearing:
//!
//! 1. `committed_matches_emitter_output_byte_for_byte` — the file at
//!    workspace root is byte-identical to
//!    `render_example_config(&DRY4RS_META)`. The committed file IS
//!    the function's output by construction; if a schema change drifts
//!    the emitter, this test fails loud and surfaces the regen command.
//! 2. `parsed_example_has_every_option_some_every_collection_non_empty`
//!    — the file parses cleanly as `Config` AND every `Option<T>` is
//!    `Some`, every collection is non-empty. This proves the example
//!    is *exhaustive*, not an empty stub — without this, the byte
//!    test would still pass on a degenerate emitter that produces an
//!    almost-empty file.
//!
//! Per the "documentation rots; CI doesn't" rule (memory
//! `feedback_documentation-rots-ci-doesnt`), both tests run on every
//! CI to keep the canonical option reference in sync with the schema.

use dry4rs::DRY4RS_META;
use dry4rs::adapters::config_doc_gen::render_example_config;
use dry4rs::domain::Config;

/// The committed workspace-root example, included at build time.
///
/// Path is relative to this file: walk up from `crates/dry4rs/tests/`
/// to workspace root.
const COMMITTED_EXAMPLE: &str = include_str!("../../../dry.example.toml");

#[test]
fn committed_matches_emitter_output_byte_for_byte() {
    let emitted = render_example_config(&DRY4RS_META);
    if COMMITTED_EXAMPLE != emitted {
        panic!(
            "`dry.example.toml` is out of sync with the doc-gen emitter.\n\
             \n\
             Regenerate from the workspace root:\n\
             \n\
             \u{20}\u{20}\u{20}\u{20}cargo run -p dry4rs --release -- init --force\n\
             \n\
             Diff (committed vs. emitter):\n\
             ---\n\
             Committed ({} bytes):\n{committed}\n\
             ---\n\
             Emitter ({} bytes):\n{emitted}\n",
            COMMITTED_EXAMPLE.len(),
            emitted.len(),
            committed = COMMITTED_EXAMPLE,
            emitted = emitted,
        );
    }
}

#[test]
fn parsed_example_has_every_option_some_every_collection_non_empty() {
    let parsed: Config = toml::from_str(COMMITTED_EXAMPLE)
        .expect("committed dry.example.toml must parse as a Config");

    assert!(
        parsed.gate.threshold.is_some(),
        "exhaustive example: gate.threshold must be Some"
    );
    assert!(
        parsed.gate.threshold_mode.is_some(),
        "exhaustive example: gate.threshold_mode must be Some"
    );
    assert!(
        parsed.output.format.is_some(),
        "exhaustive example: output.format must be Some"
    );
    assert!(
        parsed.walk.include_ignored.is_some(),
        "exhaustive example: walk.include_ignored must be Some"
    );
    let exts = parsed
        .walk
        .extensions
        .as_ref()
        .expect("exhaustive example: walk.extensions must be Some");
    assert!(
        !exts.is_empty(),
        "exhaustive example: walk.extensions must be non-empty"
    );
}
