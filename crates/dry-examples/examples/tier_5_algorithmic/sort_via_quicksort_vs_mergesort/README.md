# tier_5_algorithmic / sort_via_quicksort_vs_mergesort

## What this demonstrates

Two sort implementations of `Vec<i32>` — quicksort (partition
around a pivot, recurse on the halves) and mergesort (split in half,
recurse, merge sorted halves). Both produce a sorted `Vec<i32>` from
any `Vec<i32>`; behaviorally interchangeable, structurally unrelated.

Like `iterative_vs_recursive_factorial`, this fixture pins the v0.1
limit: structural detectors do not — and cannot — equate algorithms
by output. A "sort" is a contract that multiple algorithms satisfy;
duplicate detection at the algorithm level is a different problem.

## Why dry4rs should NOT detect (documented v0.1 limit)

Quicksort's fingerprints carry partition-loop + extend + recursive-call
patterns. Mergesort's fingerprints carry split-loop + merge-while +
double-recursive-call patterns. The intersection is small (the
length-1 base case + a `for` loop scaffold); the union is large.
Jaccard score should remain well below the 0.85 threshold.

Detection would require either (a) semantic-equivalence reasoning,
(b) trait-bound-based interchangeability inference, or (c) static
analysis of the function output. None of these are in scope at v0.1
or any planned v0.x.

## How similar tools handle this

No structural detector catches this. The fixture documents the
boundary of the duplication-detection problem — useful for setting
honest expectations when consumers ask "will dry-rs catch all
duplication?".
