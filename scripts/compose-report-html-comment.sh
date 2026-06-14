#!/usr/bin/env bash
# Compose the `report-html` sticky PR-comment body for the
# `.github/workflows/self-test.yml` `report-html` job (dry-rs#153).
#
# The job runs dry4rs against its own production source, emits the
# self-contained HTML explorer as a build artifact AND a JSON envelope
# for stats, then posts a sticky PR comment summarizing Forms / Matches
# / per-tier counts with a Download-artifact link and an Open-on-Pages
# link.
#
# This logic lives in a script (not inline `run:`) for two reasons:
#
#   1. SECURITY (template-injection): the workflow passes ONLY trusted
#      GitHub context (the artifact run URL, the optional Pages URL)
#      via `env:`; no PR-author-controlled text (PR title, branch name,
#      commit message) ever reaches this script. Stats come from the
#      committed JSON envelope on disk, never from interpolated
#      `${{ ... }}`. Keeping the body composition out of `run:` blocks
#      removes the injection vector entirely.
#
#   2. TESTABILITY (gate-pattern negative test): the per-tier table must
#      render correctly BOTH with findings (positive) AND with zero
#      findings (negative). dry4rs's `by_tier` map OMITS tiers with a
#      zero count, so the negative path is a real branch, not a
#      formality. `scripts/test-compose-report-html-comment.sh` feeds
#      this script mocked 0-findings and N-findings envelopes and
#      asserts both render — neither branch can silently break.
#
# Usage:
#   compose-report-html-comment.sh <report.json>
#
# Environment (all OPTIONAL; trusted GitHub context only):
#   ARTIFACT_RUN_URL  — link to the workflow run hosting the uploaded
#                       `report-html` artifact (the HTML explorer). When
#                       unset, the Download line degrades to a "see the
#                       run's Artifacts section" note.
#   PAGES_URL         — live GitHub Pages preview URL for this report.
#                       PR16 ships WITHOUT a live Pages deploy; PR17
#                       (dry-rs#113-gated) flips it on. When unset, the
#                       Open-on-Pages line renders a clearly-marked
#                       "lands in PR17" placeholder so PR17 only needs
#                       to set this env var — no body rework.
#
# Output: the markdown comment body on stdout.
#
# Exit codes:
#   0 — body composed
#   1 — usage error / missing report file / jq failure

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $(basename "$0") <report.json>" >&2
    exit 1
fi

report="$1"
if [[ ! -f "$report" ]]; then
    echo "error: report file not found: $report" >&2
    exit 1
fi

# Extract stats from the v0.1 wire envelope. `// 0` defaults are
# load-bearing: `result.summary.by_tier` OMITS zero-count tiers, and a
# clean run can omit ALL of them — the negative-test path. `matches`
# is a top-level array under `result` (the summary carries no
# `matches_count`), so length it directly.
total_forms="$(jq -r '.result.summary.total_forms // 0' "$report")"
matches_count="$(jq -r '(.result.matches | length) // 0' "$report")"
auto_refactor="$(jq -r '.result.summary.by_tier.auto_refactor // 0' "$report")"
review_first="$(jq -r '.result.summary.by_tier.review_first // 0' "$report")"
advisory="$(jq -r '.result.summary.by_tier.advisory // 0' "$report")"

# Download-artifact line: prefer a direct run link when the trusted
# context supplied one, else point at the run's Artifacts section.
if [[ -n "${ARTIFACT_RUN_URL:-}" ]]; then
    download_line="[Download the HTML explorer](${ARTIFACT_RUN_URL}) (the \`report-html\` artifact on this run)."
else
    download_line="Download the \`report-html\` artifact from this workflow run's **Artifacts** section."
fi

# Open-on-Pages line: live in PR17 (dry-rs#113-gated). Until PAGES_URL
# is set, render a clearly-marked placeholder so PR17 only flips the
# env var — the body shape never changes.
if [[ -n "${PAGES_URL:-}" ]]; then
    pages_line="[Open the live explorer on GitHub Pages](${PAGES_URL})."
else
    pages_line="_Open-on-Pages: the live preview URL lands in PR17 (dry-rs#113-gated GitHub Pages publish). Until then, download the artifact above._"
fi

# Compose the body. The H2 mirrors the marocchino `header:` dedup
# marker so the sticky stays visually identifiable on the PR.
cat <<EOF
## dry-rs · report-html explorer

Self-contained HTML duplication explorer, rendered by \`dry4rs report --format html\` against \`crates/dry-core/src\` + \`crates/dry4rs/src\` (auto-discovers \`dry.toml\`). Measurement only (\`--no-fail\`) — the production-code DRY gate lives in the \`dry-self-scorecard\` sticky.

| Metric | Count |
| --- | ---: |
| Forms analyzed | ${total_forms} |
| Matches | ${matches_count} |

| Tier | Matches |
| --- | ---: |
| \`auto_refactor\` (>= 0.95) | ${auto_refactor} |
| \`review_first\` (>= 0.85) | ${review_first} |
| \`advisory\` (>= threshold) | ${advisory} |

- ${download_line}
- ${pages_line}
EOF
