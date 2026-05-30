{# Markdown reporter template (dry-rs#91).

   Renders the tier-grouped duplication report produced by
   `markdown::render`. Findings group into up to three tier sections
   (`auto_refactor` / `review_first` / `advisory`); within each section
   one block per match carries a `score`, a `kind`, and a fenced code
   block listing the participating form locations (`file:line:col`).

   `score` is pre-formatted as `{:.2}` in Rust — askama's `{{ }}`
   interpolation does not honor Rust format specifiers; the template is
   composition-only. The same applies to the 1-based column display
   already baked into each `FormLine`.

   escape = "none" because markdown special chars (`|`, `*`, `_`, `#`,
   backticks) must pass through verbatim. The reporter owns any
   escaping its data needs.

   Mirrors the crap4rs markdown template structure (crap4rs#260):
   a top-level header, a `match` over the body discriminant, and a
   `for` loop per section. -#}
# Duplication Report

{% match body -%}
{%- when MarkdownBody::Empty -%}
No matches above threshold.
{%- when MarkdownBody::Filled with { sections } -%}
{% for section in sections -%}
## {{ section.heading }} ({{ section.matches.len() }})

{% for m in section.matches -%}
### Score {{ m.score }} · {{ m.kind }}

```
{% for form in m.forms -%}
{{ form }}
{% endfor -%}
```

{% endfor -%}
{% endfor -%}
{%- endmatch -%}
