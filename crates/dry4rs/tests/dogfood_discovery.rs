//! Dogfood discovery integration test (N68).
//!
//! Asserts that dry-rs's workspace-root `dry4rs.toml` is found by
//! `discover_config` + parses successfully via `load_config`.
//! Exercises the loader path on dry-rs's own dogfood file at v0.1
//! (without requiring dogfood values to differ from defaults).
//!
//! tracked: ops/decisions/org/adr-config-file-pattern.md D10
//! (dogfood requirement).
//!
//! This test sits in `crates/dry4rs/tests/`, not `crates/dry-core/
//! tests/`, because:
//! - It's a real-world integration test specific to the dry4rs
//!   binary's workspace.
//! - `CARGO_MANIFEST_DIR` resolves to `crates/dry4rs/`; walking up
//!   two levels gets us to the workspace root.
//! - The literal `"dry4rs.toml"` IS the point of this test
//!   (verifying THE dogfood file). Per per-tool ADR V3, this file
//!   is INTENTIONALLY EXCLUDED from the layer-4 ast-purity gate
//!   (`scripts/check-config-ast-purity.sh`).

use std::path::PathBuf;

use dry_core::adapters::config::{discover_config, load_config};

#[test]
fn workspace_root_dry4rs_toml_discovered_and_parses() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .expect("crates/dry4rs has a parent")
        .parent()
        .expect("crates/ has a parent");

    let found = discover_config(workspace_root, "dry4rs.toml")
        .expect("discover_config should not error on filesystem walk");

    let path = found.expect("dry4rs.toml should exist at workspace root");
    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        Some("dry4rs.toml"),
        "discovered path filename is dry4rs.toml"
    );

    let _config = load_config(&path).expect("dogfood dry4rs.toml parses cleanly");
}
