#!/usr/bin/env bash
# tracked: ops/decisions/org/adr-config-file-pattern.md (D7)
#
# Layer-4 ast-purity gate for `dry-core::adapters::config` (loader)
# + `dry-core::tests/config.rs` (loader tests). Per ADR D7, both
# files MUST NOT contain double-quoted adapter-binary-name literals:
#
#   "dry-rs.toml" | "dry4ts.toml" | "dry4rs" | "dry4ts"
#
# Adapter-name plumbing flows exclusively through `discover_config`'s
# `file_name: &str` parameter (supplied by `&meta.config_file_name`
# at the binary boundary). Tests synthesize their own fixture via
# `TEST_META.config_file_name = "test-adapter.toml"` (see
# `crates/dry-core/tests/common/mod.rs`).
#
# Scope is intentionally narrow — the gate covers the loader source
# AND the loader integration tests, NOTHING ELSE. In particular:
# - `crates/dry-core/tests/cli_args.rs` is EXCLUDED. Those tests
#   call `parse_test_args(&["dry4rs", ...])` which uses the literal
#   as a synthetic CLI argv (per ADR V3 exception documented in
#   `ops/decisions/dry-rs/adr-dry4rs-config-file.md`).
# - `crates/dry4rs/tests/dogfood_discovery.rs` is EXCLUDED. That
#   test asserts the literal `"dry-rs.toml"` IS the workspace-root
#   dogfood file — the literal is the point of the test.
# - The dry4rs binary (`crates/dry4rs/src/main.rs`) is EXCLUDED.
#   That's the ADAPTER, where adapter-name literals belong.
#
# Sibling-coherent with scrap-rs#37 (layer-4 ast-purity gate for
# `scrap-core::cli::config`). Cross-tool ADR D7.
#
# Exit codes:
#   0 — no rejected literals found
#   1 — at least one rejected literal found (annotated to stderr)

set -euo pipefail

# Regex of rejected double-quoted literals. The pattern matches the
# EXACT literal — bare adapter name OR adapter-name + config suffix.
# We anchor each alternative with double quotes so substring matches
# inside identifiers don't false-positive (e.g., `dry4rs::main`,
# the rust module path, doesn't have surrounding quotes).
REJECTED='"dry-rs\.toml"|"dry4ts\.toml"|"dry4rs"|"dry4ts"'

FILES=(
    crates/dry-core/src/adapters/config.rs
    crates/dry-core/tests/config.rs
)

found_any=0
for file in "${FILES[@]}"; do
    if [[ ! -f "$file" ]]; then
        # File doesn't exist yet — this is fine during the pipeline
        # build (e.g., before Stage 2 lands). Skip silently.
        continue
    fi
    # First-pass match: any line carrying the rejected literal token.
    # Comment lines (`//`, `//!`, `///`, block comments) are EXCLUDED
    # — doc comments referencing the rule by name are NOT code.
    # Rationale: ADR D7 forbids the literal in CODE; documentation
    # discussing the rule must be able to name the rejected token.
    # The exclusion is anchored to lines whose first non-whitespace
    # chars are `//` (line comments and `///` / `//!` doc comments).
    # Block-comment forms `/* */` are not currently used in the loader
    # source, so are not handled (a follow-up if they appear).
    if matches=$(grep -nE "$REJECTED" "$file" 2>/dev/null \
                 | grep -vE '^[0-9]+:[[:space:]]*//' || true); then
        if [[ -n "$matches" ]]; then
            echo "::error file=$file::layer-4 ast-purity gate: adapter-binary-name literal found in loader scope" >&2
            echo "$matches" | while IFS=: read -r lineno content; do
                echo "  $file:$lineno  $content" >&2
            done
            found_any=1
        fi
    fi
done

if [[ "$found_any" -ne 0 ]]; then
    cat >&2 <<'MSG'

Reject reason: per cross-tool ADR `org/adr-config-file-pattern.md` D7,
`dry-core::adapters::config` + its tests MUST NOT contain double-quoted
adapter-binary-name literals. All adapter-name plumbing flows through
`discover_config`'s `file_name: &str` parameter (supplied by
`&meta.config_file_name` at the binary boundary).

Fix: replace literal `"dry-rs.toml"` with `TEST_META.config_file_name`
(via `crates/dry-core/tests/common/mod.rs`) or thread an explicit
`&str` through the call site.

If the literal IS legitimately required (e.g., it documents the
canonical dogfood file name in a non-loader context), file an ADR
amendment via `ops/decisions/dry-rs/adr-dry4rs-config-file.md` rather
than weakening this gate.
MSG
    exit 1
fi

echo "layer-4 ast-purity gate: clean (${#FILES[@]} file(s) scanned)"
