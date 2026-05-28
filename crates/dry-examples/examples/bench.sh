#!/usr/bin/env bash
# Cross-tool bench script for the dry-examples corpus.
#
# For each fixture under examples/<tier>/<fixture>/:
#   - Run dry4rs report --format json --include-ignored against the
#     fixture directory; parse the envelope; emit highest-Jaccard
#     match score + tier.
#   - Run similarity-rs (if `command -v similarity-rs` succeeds);
#     parse stdout for the highest-similarity duplicate pair; emit
#     percentage.
#   - Emit one markdown table row per fixture.
#
# The corpus crate ships a `.ignore` (`*`) file that opts the
# directory out of dry4rs's default walk; the harness uses
# `--include-ignored` to re-walk. similarity-rs honors `.ignore` too
# and there's no `--include-ignored` flag — so this script COPIES
# each fixture to a `$TMPDIR/dry-rs-bench/<fixture>/` before invoking
# similarity-rs, sidestepping the `.ignore` filter at the source.
#
# Output goes to stdout; redirect to bench-output.md to refresh the
# committed artifact:
#
#   bash crates/dry-examples/examples/bench.sh > crates/dry-examples/examples/bench-output.md
#
# Or trigger the refresh-bench.yml workflow_dispatch to do the same
# in CI and open a PR.
#
# Graceful degradation: if similarity-rs is not in PATH, the
# similarity-rs column emits "TBD"; the script does NOT attempt to
# install it.
#
# Per ADR-8, this script is INTENTIONALLY not wired to per-PR CI —
# the artifact backs marketing claims, not gates. The
# `last_refreshed:` frontmatter field below carries the stamping date
# that the corpus-smoke job uses for its passive staleness nag.

set -euo pipefail

# Script lives at `crates/dry-examples/examples/bench.sh`; the corpus
# root is the parent of the parent of the script's directory.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLES_DIR="$SCRIPT_DIR"
CRATE_DIR="$(cd "$EXAMPLES_DIR/.." && pwd)"
REPO_ROOT="$(cd "$CRATE_DIR/../.." && pwd)"

# Refresh date stamped into the markdown frontmatter — sets the
# 30-day staleness clock the corpus-smoke job reads. Today's date in
# UTC.
TODAY="$(date -u +%Y-%m-%d)"

# similarity-rs presence is detected once; per-fixture invocations
# skip cleanly when absent.
SIMILARITY_AVAILABLE=0
if command -v similarity-rs >/dev/null 2>&1; then
    SIMILARITY_AVAILABLE=1
fi

# Workspace temp dir for similarity-rs invocations (the corpus's
# `.ignore` file would otherwise make every fixture invisible to it).
WORK_DIR="$(mktemp -d -t dry-rs-bench.XXXXXX)"
trap 'rm -rf "$WORK_DIR"' EXIT

# ────────────────────────────────────────────────────────────────────
# dry4rs invocation
# ────────────────────────────────────────────────────────────────────
#
# Returns either `<tier> @ <score>` or `no match` for the highest-
# Jaccard pair the tool reports against this fixture. Scores are
# rendered with 3 decimal places (matching EXPECTED.md's verdict
# column convention).
dry4rs_verdict() {
    local fixture_dir="$1"
    local relative
    relative="${fixture_dir#$CRATE_DIR/}"
    local envelope
    envelope=$(
        cd "$CRATE_DIR" &&
        cargo run --quiet --locked -p dry4rs -- \
            report \
            --format json \
            --no-fail \
            --include-ignored \
            "${relative}/" 2>/dev/null
    )
    # `jq -r` on the highest-score match. Empty matches array → "no match".
    echo "$envelope" | jq -r '
      if (.result.matches | length) == 0 then
        "no match"
      else
        .result.matches
        | sort_by(-.score)[0]
        | "\(.tier) @ \(.score | . * 1000 | floor / 1000)"
      end
    '
}

# ────────────────────────────────────────────────────────────────────
# similarity-rs invocation
# ────────────────────────────────────────────────────────────────────
#
# Returns `<percentage>%` for the highest-similarity pair, `no match`
# when none surface, or `TBD` when the binary isn't available.
#
# Copies the fixture to a temp dir to sidestep the corpus crate's
# `.ignore` file (similarity-rs has no equivalent of dry4rs's
# `--include-ignored`).
similarity_verdict() {
    local fixture_dir="$1"
    if [ "$SIMILARITY_AVAILABLE" -ne 1 ]; then
        echo "TBD"
        return
    fi
    local fixture_name
    fixture_name="$(basename "$fixture_dir")"
    local tier_name
    tier_name="$(basename "$(dirname "$fixture_dir")")"
    local scratch="$WORK_DIR/${tier_name}_${fixture_name}"
    mkdir -p "$scratch"
    cp -R "$fixture_dir/." "$scratch/"
    local out
    out=$(similarity-rs --threshold 0.0 "$scratch" 2>&1 || true)
    # similarity-rs prints lines like:
    #   /path/to/main.rs:6-10 function add_one <-> /path/to/main.rs:12-16 function increment
    #   Similarity: 99.25%
    # Parse the highest "Similarity: <pct>%" line.
    local pct
    pct=$(echo "$out" \
        | awk '/Similarity:/ { gsub(/%/, "", $2); print $2 }' \
        | sort -g \
        | tail -n 1)
    if [ -z "$pct" ]; then
        echo "no match"
    else
        echo "${pct}%"
    fi
}

# ────────────────────────────────────────────────────────────────────
# Markdown emission
# ────────────────────────────────────────────────────────────────────

cat <<HEADER
---
last_refreshed: $TODAY
---

# dry-examples — cross-tool benchmark

Empirical side-by-side comparison of dry4rs (this repo) vs
similarity-rs (closest competitor in the Rust duplication-detection
space) against the curated dry-examples corpus.

Refresh cadence: manual, pre-release. Trigger the
\`refresh-bench.yml\` workflow_dispatch from the Actions tab, or run
\`bash crates/dry-examples/examples/bench.sh >
crates/dry-examples/examples/bench-output.md\` locally with
similarity-rs installed.

Per ADR-8, this artifact is NOT auto-refreshed on push or PR — it
backs marketing claims, not gates. The \`last_refreshed:\` field at
the top of this file feeds the corpus-smoke job's passive staleness
nag (\`::warning::\` after 30 days).

## Catalogue

| Tier | Fixture | dry4rs | similarity-rs |
|------|---------|--------|---------------|
HEADER

# Iterate tier dirs first (sorted), then fixtures within each.
for tier_dir in $(find "$EXAMPLES_DIR" -mindepth 1 -maxdepth 1 -type d | sort); do
    tier_name="$(basename "$tier_dir")"
    for fixture_dir in $(find "$tier_dir" -mindepth 1 -maxdepth 1 -type d | sort); do
        fixture_name="$(basename "$fixture_dir")"
        # Skip directories that have no .rs files (defensive against
        # future organizational subdirs).
        if ! find "$fixture_dir" -maxdepth 1 -name "*.rs" -type f | grep -q .; then
            continue
        fi
        dry_v=$(dry4rs_verdict "$fixture_dir")
        sim_v=$(similarity_verdict "$fixture_dir")
        echo "| ${tier_name} | \`${fixture_name}/\` | ${dry_v} | ${sim_v} |"
    done
done

cat <<FOOTER

## Reading the verdicts

- **dry4rs column** mirrors each fixture's \`expected.json\` highest-
  Jaccard match: \`<tier> @ <score>\` (3-decimal float) or
  \`no match\`. Scores below the 0.85 v0.1 review_first floor surface
  as \`no match\` because the report path filters them out before the
  \`result.matches\` array.
- **similarity-rs column** is the highest reported similarity
  percentage across all pairs in the fixture, or \`no match\` when
  no pairs surface, or \`TBD\` when similarity-rs is not installed at
  refresh time. similarity-rs reports pairwise function-to-function
  similarities; the highest-pair percentage is the most informative
  single number for cross-tool comparison.

## Notes

- similarity-rs is invoked at \`--threshold 0.0\` so every pairwise
  comparison surfaces, ensuring the column always carries the
  highest-similarity datum even when both tools agree on
  non-detection.
- Each fixture's directory is COPIED to a temp location before
  similarity-rs analyzes it because the corpus crate's \`.ignore\`
  file (\`*\`) would otherwise hide every fixture from similarity-rs's
  walker. dry4rs's \`--include-ignored\` is the per-tool moral
  equivalent.
- Cross-file fixtures (today: only \`edge_cases/cross_file_duplicate/\`
  with \`producer.rs\` + \`consumer.rs\`) are passed as directories,
  not single files, so both tools see the full fixture surface.

## Cross-references

- Fixture catalogue: [\`EXPECTED.md\`](../EXPECTED.md)
- Crate README: [\`README.md\`](../README.md)
- ADR-8 (workflow_dispatch + staleness nag):
  \`ops/decisions/dry-rs/adr-dry-examples-corpus.md\`
- similarity-rs: <https://crates.io/crates/similarity-rs>
FOOTER
