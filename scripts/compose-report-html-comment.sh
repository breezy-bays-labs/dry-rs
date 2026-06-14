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
#                       unset, the Download cell degrades to a "run's
#                       Artifacts section" note.
#   PAGES_URL         — live GitHub Pages preview URL for this report
#                       (dry-rs#156: `…github.io/<repo>/pr-<N>/`). Set on
#                       PR events; the `View ↗` cell becomes a one-click
#                       live link. When unset (push-to-main, or a fork PR
#                       whose read-only token can't publish), the `View ↗`
#                       cell renders a clearly-marked "download instead"
#                       note — no body rework.
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

# The cute-dbt-style action cells (mirrors
# `breezy-bays-labs/cute-dbt` `report-preview.yml`'s
# `| Report | View | Download |` table). Each is built from TRUSTED
# context only — no PR-author text is ever interpolated.
#
# View ↗ cell: a one-click live GitHub Pages preview when PAGES_URL is
# set (PR events, same-repo). When unset (push-to-main, or a fork PR
# whose read-only token can't publish), it degrades to a clearly-marked
# "download instead" note rather than a dead 404 link.
if [[ -n "${PAGES_URL:-}" ]]; then
    view_cell="[👁 View ↗](${PAGES_URL})"
else
    view_cell="_download ↓_"
fi

# Download cell: prefer a direct run link when the trusted context
# supplied one, else point at the run's Artifacts section.
if [[ -n "${ARTIFACT_RUN_URL:-}" ]]; then
    download_cell="[⬇ Download](${ARTIFACT_RUN_URL})"
else
    download_cell="_run's **Artifacts** section_"
fi

# Compose the body. The H2 mirrors the marocchino `header:` dedup
# marker so the sticky stays visually identifiable on the PR. The
# `| Report | View | Download |` action table (cute-dbt-style) leads;
# the Forms / Matches + per-tier stats follow.
cat <<EOF
## dry-rs · report-html explorer

Self-contained HTML duplication explorer, rendered by \`dry4rs report --format html\` against \`crates/dry-core/src\` + \`crates/dry4rs/src\` (auto-discovers \`dry.toml\`). Measurement only (\`--no-fail\`) — the production-code DRY gate lives in the \`dry-self-scorecard\` sticky.

| Report | View | Download |
|---|---|---|
| \`index.html\` (explorer) | ${view_cell} | ${download_cell} |

| Metric | Count |
| --- | ---: |
| Forms analyzed | ${total_forms} |
| Matches | ${matches_count} |

| Tier | Matches |
| --- | ---: |
| \`auto_refactor\` (>= 0.95) | ${auto_refactor} |
| \`review_first\` (>= 0.85) | ${review_first} |
| \`advisory\` (>= threshold) | ${advisory} |

**👁 View ↗** opens the explorer in your browser in one click — published to this repo's GitHub Pages under \`/pr-<N>/\`. **⬇ Download** fetches the same self-contained HTML as a workflow artifact (auth-gated; works fully offline). Either way the explorer makes zero external resource requests.

_The Pages preview may take ~1 min to update after this comment posts. On PRs from forks the View link is unavailable (read-only token) — use Download. If this repo's GitHub Pages site is private (Enterprise), the View link is reachable only to authorized users; otherwise a published report is reachable by anyone with the URL._
EOF
