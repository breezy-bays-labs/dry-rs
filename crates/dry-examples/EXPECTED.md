# dry-examples — corpus catalogue

Cross-fixture verdict table for the dry-rs duplication-detection
corpus. One row per fixture; columns capture what dry4rs surfaces at
the v0.1 detection contract and what competitor tools surface on the
same input (when known).

**Sort discipline** (ADR-4): rows are sorted by
`(tier_number, fixture_path_lex)` with `edge_cases` last. The CI lint
in `tests/snapshots.rs::expected_md_table_is_sorted` enforces this.
Adding a new fixture mid-table re-sorts the section; merge conflicts
on this file resolve mechanically.

**Verdict-column semantics** (ADR-7): the values below mirror what
`BLESS=1 cargo test -p dry-examples` captures into each fixture's
`expected.json`. These are **observed** — not aspirational —
verdicts. A future detector change that moves a verdict requires
re-BLESS plus a corresponding column update in the same PR. The
catalogue is documentation; the `expected.json` files are the
load-bearing regression contract.

**Tool-neutral?**: marked `yes` when both dry4rs and similarity-rs
land on the same verdict family (both detect / both miss); marked
`no` when verdicts diverge. The similarity-rs column populates
empirically via `examples/bench.sh` (Stage 2.7).

## Catalogue

| Tier | Path | One-line description | dry4rs verdict | similarity-rs verdict | tool-neutral? |
|------|------|----------------------|----------------|------------------------|----------------|
| tier_1_exact | `tier_1_exact/identical_async_fns/` | Two async fns with identical bodies | review_first @ 0.913 | TBD | TBD |
| tier_1_exact | `tier_1_exact/identical_method_bodies/` | Two methods on different structs, identical bodies | no match (documented limit) | TBD | TBD |
| tier_1_exact | `tier_1_exact/identical_signatures/` | Two free fns, identical bodies, renamed identifiers | review_first @ 0.857 | TBD | TBD |
| tier_2_renamed | `tier_2_renamed/renamed_locals/` | Same body shape, renamed local bindings | review_first @ 0.857 | TBD | TBD |
| tier_2_renamed | `tier_2_renamed/renamed_params/` | Same body shape, renamed parameter identifiers | review_first @ 0.867 | TBD | TBD |
| tier_2_renamed | `tier_2_renamed/renamed_struct_fields/` | Same constructor shape, renamed struct fields | no match (documented limit) | TBD | TBD |
| tier_3_reordered | `tier_3_reordered/rearranged_method_chain/` | Builder calls in different order | no match | TBD | TBD |
| tier_3_reordered | `tier_3_reordered/reordered_match_arms/` | Identical match arms in different source order | no match | TBD | TBD |
| tier_3_reordered | `tier_3_reordered/swapped_let_bindings/` | Identical statements in permuted order | no match | TBD | TBD |
| tier_4_false_positive_bait | `tier_4_false_positive_bait/common_error_handling/` | Two unrelated fns sharing `if let Err(e)` idiom | no match (correct) | TBD | TBD |
| tier_4_false_positive_bait | `tier_4_false_positive_bait/shared_idiom/` | Two unrelated fns sharing `for i in 0..10` | no match (correct) | TBD | TBD |
| tier_4_false_positive_bait | `tier_4_false_positive_bait/similar_signatures_different_bodies/` | Same signature, `add` vs `subtract` bodies | no match (correct) | TBD | TBD |
| tier_5_algorithmic | `tier_5_algorithmic/iterative_vs_recursive_factorial/` | Same outcome, different control flow | no match (v0.1 limit) | TBD | TBD |
| tier_5_algorithmic | `tier_5_algorithmic/sort_via_quicksort_vs_mergesort/` | Same outcome, different algorithm | no match (v0.1 limit) | TBD | TBD |
| edge_cases | `edge_cases/async_vs_sync_same_body/` | One async, one sync, identical body | review_first @ 0.882 | TBD | TBD |
| edge_cases | `edge_cases/cross_file_duplicate/` | Multi-file duplication (producer.rs + consumer.rs) | review_first @ 0.900 (cross-file) | TBD | TBD |
| edge_cases | `edge_cases/doctest_duplicates/` | Two fns with identical doctest blocks | no match | TBD | TBD |
| edge_cases | `edge_cases/generic_with_different_bounds/` | Same body, `Display` vs `Debug` bound | no match | TBD | TBD |
| edge_cases | `edge_cases/macro_expansion_duplicate/` | `macro_rules!` expanding to two dup fns | no match (pre-expansion blindness) | TBD | TBD |
| edge_cases | `edge_cases/trait_impl_vs_free_fn/` | Method body vs free-fn body, same content | no match | TBD | TBD |

## Reading the verdicts

- **`review_first @ <score>`**: dry4rs surfaced this fixture as a
  match in the `review_first` tier with the given Jaccard score.
- **`auto_refactor @ <score>`**: same, in the `auto_refactor` tier
  (>= 0.95).
- **`advisory @ <score>`**: same, in the `advisory` tier (below the
  v0.1 0.85 review_first floor; surfaces only with `--threshold`
  override).
- **`no match`**: dry4rs surfaced zero matches against this fixture.
  May be correct (tier 4 FP-bait, tier 5 algorithmic limit) or a
  documented limit (some tier 1 / tier 2 cases that surprise on
  empirical capture).
- **`no match (correct)`**: tier 4 FP-bait — non-detection is the
  intended behavior; a regression that flagged this would be a real
  bug.
- **`no match (v0.1 limit)`**: tier 5 algorithmic — structural
  detection cannot equate algorithms by output; documented limit of
  the duplication-detection problem.
- **`no match (documented limit)`**: some tier 1 / tier 2 fixtures
  surface as misses (e.g., `identical_method_bodies` doesn't cluster
  the two `step` methods on different structs). The behavior is
  recorded here so future detector changes that close the gap have
  a measurable BEFORE/AFTER.

## Cross-references

- Per-fixture rationale: each fixture's `README.md` (what it
  demonstrates / why dry4rs should/should not detect / how similar
  tools handle this).
- Snapshot harness: `tests/snapshots.rs` runs each fixture per PR.
- Bless workflow: `BLESS=1 cargo test -p dry-examples` regenerates
  every `expected.json`.
- Cross-tool bench: `examples/bench-output.md` is the empirical
  side-by-side vs similarity-rs (refreshed via the
  `refresh-bench.yml` workflow_dispatch).
- ADR: `ops/decisions/dry-rs/adr-dry-examples-corpus.md`
  (ADR-4 sort discipline, ADR-7 observational vs normative).
