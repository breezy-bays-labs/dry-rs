# tier_5_algorithmic / iterative_vs_recursive_factorial

## What this demonstrates

Two factorial implementations producing the same output value from
the same input — but with completely different control flow. One
uses a `for` loop with a mutable accumulator; the other uses
self-recursion with a base case.

Detecting these as duplicates would require *semantic* equivalence
analysis (loop-to-recursion transformation, fixed-point reasoning).
That's outside the scope of structural duplication detection.

## Why dry4rs should NOT detect (documented v0.1 limit)

dry4rs's normalizer operates on AST structure post-typed-placeholder.
The iterative version's fingerprints are dominated by the `for`/`*=`
loop pattern; the recursive version's fingerprints are dominated by
the `if`/early-return/self-call pattern. The two fingerprint sets are
essentially disjoint; Jaccard score should be near zero.

This fixture documents the v0.1 algorithm limit honestly. A future
v0.x might add semantic-equivalence detection as an opt-in pass; this
fixture would then move from tier 5 to a "newly detected" category
and the `expected.json` would be updated via BLESS.

## How similar tools handle this

No structural duplication detector catches this case. Semantic
equivalence is a property of the runtime behavior, not the source
shape — it's the wrong tool for the job. The tier 5 fixtures are
*always-miss* by design; they exist to document the boundary of
"what duplication detection means".
