{# Markdown reporter template (dry-rs#91).

   Renders the duplication report as a rich GitHub-flavored "sticky
   card": a header line (total matches + per-tier counts + worst score),
   a tier-summary table, and one collapsible `<details>` block per
   match. An empty report renders a clean "no matches" card.

   This template OWNS all presentation — layout, the tier-severity emoji
   (🔴 auto_refactor / 🟡 review_first / 🔵 advisory), and numeric
   formatting (`{:.2}` via the askama `format` filter). The Rust view-model
   (`MarkdownReport` / `TierView` / `MatchView`) supplies only semantic
   data: raw `f64` scores, `Tier` values, and shared-helper-formatted
   `file:line:col` strings. Tier/kind labels come from the single-source
   `Tier::as_str` / `FormKind::as_str` — one vocabulary across the
   header, the table, and the per-match `<details>` summaries.

   escape = "none" because markdown special chars (`|`, `*`, `_`, `#`,
   backticks) must pass through verbatim. The reporter wraps file paths
   in backticks so underscores do not italicize inside `<summary>`. -#}
{%- if tiers.is_empty() -%}
# 🔁 Duplication Report

✅ No matches above threshold.
{%- else -%}
# 🔁 Duplication Report

**{{ total_matches }} matches** —
{%- for t in tiers %} {% match t.tier %}{% when Tier::AutoRefactor %}🔴{% when Tier::ReviewFirst %}🟡{% when Tier::Advisory %}🔵{% endmatch %} {{ t.count }} {{ t.tier.as_str() }}{% if !loop.last %} ·{% endif %}{% endfor %} · worst `{{ "{:.2}"|format(worst_overall) }}`

| Tier | Matches | Worst |
|------|--------:|------:|
{% for t in tiers -%}
| {% match t.tier %}{% when Tier::AutoRefactor %}🔴{% when Tier::ReviewFirst %}🟡{% when Tier::Advisory %}🔵{% endmatch %} {{ t.tier.as_str() }} | {{ t.count }} | {{ "{:.2}"|format(t.worst) }} |
{% endfor %}
{% for t in tiers -%}
{% for m in t.matches -%}
<details><summary>{% match m.tier %}{% when Tier::AutoRefactor %}🔴{% when Tier::ReviewFirst %}🟡{% when Tier::Advisory %}🔵{% endmatch %} {{ "{:.2}"|format(m.score) }} · {{ m.kind }} · `{{ m.primary_file }}`{% if let Some(partner) = m.partner_file %} ↔ `{{ partner }}`{% endif %}</summary>

{% for form in m.forms -%}
- `{{ form }}`
{% endfor %}
</details>

{% endfor -%}
{% endfor -%}
{%- endif -%}
