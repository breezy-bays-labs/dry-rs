#!/usr/bin/env bash
# shellcheck disable=SC2016
# (file-wide) The expected-substring literals in the assertions below
# are single-quoted ON PURPOSE: they contain markdown code-span
# backticks (e.g. `auto_refactor`) and must NOT undergo shell
# expansion — the composer emits them verbatim. SC2016 ("expressions
# don't expand in single quotes") is the intended behavior, not a bug.
#
# Gate-pattern negative test for `compose-report-html-comment.sh`
# (dry-rs#153).
#
# The `report-html` sticky body must render correctly BOTH with findings
# (positive) AND with zero findings (negative). dry4rs's `by_tier` map
# OMITS zero-count tiers, so a clean run produces an envelope with NO
# `auto_refactor` / `review_first` / `advisory` keys (and possibly an
# empty `matches` array) — the negative branch is REAL, not a
# formality. Per the project's gate-pattern-negative-test rule, a
# positive-only test can silently break the empty-state rendering.
#
# This test feeds the composer four mocked envelopes:
#   1. POSITIVE — findings across all three tiers + a populated
#      Pages/artifact context (PR17 placeholder flipped live).
#   2. NEGATIVE — zero findings: empty `matches`, `by_tier: {}`,
#      no artifact/Pages context (the PR16 placeholder branch).
#   3. PARTIAL  — some tiers present, others omitted (the real-world
#      shape — exercises every `// 0` default independently).
#   4. PR17-LIVE — non-empty PAGES_URL flips the Open-on-Pages line to
#      a live link, proving PR17 only needs to set the env var.
#
# Run locally:  bash scripts/test-compose-report-html-comment.sh
# Also runs as a CI step in the `report-html` job.
#
# Exit codes:
#   0 — all assertions pass
#   1 — an assertion failed (diagnostic on stderr)

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
composer="${here}/compose-report-html-comment.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() {
    echo "FAIL: $1" >&2
    exit 1
}

assert_contains() {
    # $1 = haystack, $2 = needle, $3 = label
    case "$1" in
        *"$2"*) ;;
        *) fail "$3 — expected to find: $2" ;;
    esac
}

assert_not_contains() {
    # $1 = haystack, $2 = needle, $3 = label
    case "$1" in
        *"$2"*) fail "$3 — expected NOT to find: $2" ;;
        *) ;;
    esac
}

# --- 1. POSITIVE: findings across all three tiers ---------------------
cat >"$tmp/positive.json" <<'JSON'
{
  "result": {
    "matches": [{}, {}, {}, {}, {}],
    "summary": {
      "total_forms": 890,
      "by_tier": { "auto_refactor": 11, "review_first": 14, "advisory": 3 }
    }
  }
}
JSON
out_pos="$(ARTIFACT_RUN_URL="https://example.test/run/1" PAGES_URL="" "$composer" "$tmp/positive.json")"
assert_contains "$out_pos" "## dry-rs · report-html explorer" "positive: header"
assert_contains "$out_pos" "| Forms analyzed | 890 |" "positive: forms"
assert_contains "$out_pos" "| Matches | 5 |" "positive: matches count"
assert_contains "$out_pos" '| `auto_refactor` (>= 0.95) | 11 |' "positive: auto_refactor"
assert_contains "$out_pos" '| `review_first` (>= 0.85) | 14 |' "positive: review_first"
assert_contains "$out_pos" '| `advisory` (>= threshold) | 3 |' "positive: advisory"
assert_contains "$out_pos" "https://example.test/run/1" "positive: artifact link"
assert_contains "$out_pos" "lands in PR17" "positive: pages placeholder (PAGES_URL empty)"

# --- 2. NEGATIVE: zero findings, omitted tiers, no context ------------
cat >"$tmp/negative.json" <<'JSON'
{
  "result": {
    "matches": [],
    "summary": {
      "total_forms": 1234,
      "by_tier": {}
    }
  }
}
JSON
out_neg="$(ARTIFACT_RUN_URL="" PAGES_URL="" "$composer" "$tmp/negative.json")"
assert_contains "$out_neg" "## dry-rs · report-html explorer" "negative: header"
assert_contains "$out_neg" "| Forms analyzed | 1234 |" "negative: forms"
assert_contains "$out_neg" "| Matches | 0 |" "negative: zero matches"
assert_contains "$out_neg" '| `auto_refactor` (>= 0.95) | 0 |' "negative: auto_refactor defaults to 0"
assert_contains "$out_neg" '| `review_first` (>= 0.85) | 0 |' "negative: review_first defaults to 0"
assert_contains "$out_neg" '| `advisory` (>= threshold) | 0 |' "negative: advisory defaults to 0"
assert_contains "$out_neg" "Artifacts** section" "negative: artifact-section fallback"
assert_contains "$out_neg" "lands in PR17" "negative: pages placeholder"
# Negative branch must not crash or emit a bare 'null' from jq.
assert_not_contains "$out_neg" "| Forms analyzed | null |" "negative: no null leak"

# --- 3. PARTIAL: some tiers present, others omitted -------------------
cat >"$tmp/partial.json" <<'JSON'
{
  "result": {
    "matches": [{}, {}],
    "summary": {
      "total_forms": 42,
      "by_tier": { "review_first": 2 }
    }
  }
}
JSON
out_part="$(ARTIFACT_RUN_URL="" PAGES_URL="" "$composer" "$tmp/partial.json")"
assert_contains "$out_part" "| Matches | 2 |" "partial: matches"
assert_contains "$out_part" '| `auto_refactor` (>= 0.95) | 0 |' "partial: missing auto_refactor -> 0"
assert_contains "$out_part" '| `review_first` (>= 0.85) | 2 |' "partial: present review_first"
assert_contains "$out_part" '| `advisory` (>= threshold) | 0 |' "partial: missing advisory -> 0"

# --- 4. PR17-LIVE: PAGES_URL set flips the Open-on-Pages line ---------
out_live="$(ARTIFACT_RUN_URL="https://example.test/run/9" PAGES_URL="https://pages.test/pr/9/" "$composer" "$tmp/positive.json")"
assert_contains "$out_live" "https://pages.test/pr/9/" "pr17-live: live pages link"
assert_not_contains "$out_live" "lands in PR17" "pr17-live: placeholder gone when PAGES_URL set"

echo "OK: compose-report-html-comment.sh renders all branches (positive, negative, partial, pr17-live)"
