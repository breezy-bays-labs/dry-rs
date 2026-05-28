---
last_refreshed: 2026-05-28
---

# dry-examples — cross-tool benchmark

Empirical side-by-side comparison of dry4rs (this repo) vs
similarity-rs (closest competitor in the Rust duplication-detection
space) against the curated dry-examples corpus.

Refresh cadence: manual, pre-release. Trigger the
`refresh-bench.yml` workflow_dispatch from the Actions tab, or run
`bash crates/dry-examples/examples/bench.sh >
crates/dry-examples/examples/bench-output.md` locally with
similarity-rs installed.

Per ADR-8, this artifact is NOT auto-refreshed on push or PR — it
backs marketing claims, not gates. The `last_refreshed:` field at
the top of this file feeds the corpus-smoke job's passive staleness
nag (`::warning::` after 30 days).

## Catalogue

| Tier | Fixture | dry4rs | similarity-rs |
|------|---------|--------|---------------|
| edge_cases | `async_vs_sync_same_body/` | review_first @ 0.882 | 83.82% |
| edge_cases | `cross_file_duplicate/` | review_first @ 0.9 | no match |
| edge_cases | `doctest_duplicates/` | no match | no match |
| edge_cases | `generic_with_different_bounds/` | no match | 83.17% |
| edge_cases | `macro_expansion_duplicate/` | no match | no match |
| edge_cases | `trait_impl_vs_free_fn/` | no match | 75.76% |
| tier_1_exact | `identical_async_fns/` | review_first @ 0.913 | 92.62% |
| tier_1_exact | `identical_method_bodies/` | no match | 99.25% |
| tier_1_exact | `identical_signatures/` | review_first @ 0.857 | 76.59% |
| tier_2_renamed | `renamed_locals/` | review_first @ 0.857 | 89.39% |
| tier_2_renamed | `renamed_params/` | review_first @ 0.866 | 75.58% |
| tier_2_renamed | `renamed_struct_fields/` | no match | 82.75% |
| tier_3_reordered | `rearranged_method_chain/` | no match | 81.99% |
| tier_3_reordered | `reordered_match_arms/` | no match | 88.94% |
| tier_3_reordered | `swapped_let_bindings/` | no match | 90.76% |
| tier_4_false_positive_bait | `common_error_handling/` | no match | 80.60% |
| tier_4_false_positive_bait | `shared_idiom/` | no match | 57.99% |
| tier_4_false_positive_bait | `similar_signatures_different_bodies/` | no match | no match |
| tier_5_algorithmic | `iterative_vs_recursive_factorial/` | no match | 76.70% |
| tier_5_algorithmic | `sort_via_quicksort_vs_mergesort/` | no match | 83.80% |

## Reading the verdicts

- **dry4rs column** mirrors each fixture's `expected.json` highest-
  Jaccard match: `<tier> @ <score>` (3-decimal float) or
  `no match`. Scores below the 0.85 v0.1 review_first floor surface
  as `no match` because the report path filters them out before the
  `result.matches` array.
- **similarity-rs column** is the highest reported similarity
  percentage across all pairs in the fixture, or `no match` when
  no pairs surface, or `TBD` when similarity-rs is not installed at
  refresh time. similarity-rs reports pairwise function-to-function
  similarities; the highest-pair percentage is the most informative
  single number for cross-tool comparison.

## Notes

- similarity-rs is invoked at `--threshold 0.0` so every pairwise
  comparison surfaces, ensuring the column always carries the
  highest-similarity datum even when both tools agree on
  non-detection.
- Each fixture's directory is COPIED to a temp location before
  similarity-rs analyzes it because the corpus crate's `.ignore`
  file (`*`) would otherwise hide every fixture from similarity-rs's
  walker. dry4rs's `--include-ignored` is the per-tool moral
  equivalent.
- Cross-file fixtures (today: only `edge_cases/cross_file_duplicate/`
  with `producer.rs` + `consumer.rs`) are passed as directories,
  not single files, so both tools see the full fixture surface.

## Cross-references

- Fixture catalogue: [`EXPECTED.md`](../EXPECTED.md)
- Crate README: [`README.md`](../README.md)
- ADR-8 (workflow_dispatch + staleness nag):
  `ops/decisions/dry-rs/adr-dry-examples-corpus.md`
- similarity-rs: <https://crates.io/crates/similarity-rs>
