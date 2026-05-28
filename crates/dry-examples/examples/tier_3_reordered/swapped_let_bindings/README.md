# tier_3_reordered / swapped_let_bindings

## What this demonstrates

Two functions that compute identical results from identical statements
permuted in a different order. The first binds `wx` then `wy`; the
second binds `wy` then `wx`. The summation, the println, and the
return all match.

This is the boundary case for Jaccard-based duplication detection:
two forms have *identical fingerprint sets* but differ in statement
order. Set-based hashing — which dry4rs's `fingerprint_set` is —
treats these as equivalent.

## Why dry4rs may or may not detect

Set-Jaccard ignores ordering by construction. The two functions
should hash-bucket together under dry4rs's first-pass clustering.
Whether the surface-level `score` lands at 1.0 (exact bucket match)
or below (because the normalizer encodes statement-position
information somewhere) is empirical — the captured `expected.json`
documents the observed behavior.

If a future v0.x detector adds ordering-sensitive structural
comparison (e.g., a tree-edit-distance pass), reordered fixtures
distinguish between the set-equality verdict and the structural
verdict. v0.1's `structural_score` slot on `Match` (currently `null`)
is reserved for exactly this.

## How similar tools handle this

`similarity-rs` uses tree-edit distance and treats reordered
statements as structurally different. dry4rs's set-Jaccard is more
permissive on this case — a tool-neutral fixture would say "this is
an opinionated detection boundary".
