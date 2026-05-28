# tier_4_false_positive_bait / shared_idiom

## What this demonstrates

Two unrelated functions that share the `for i in 0..10 { ... }` loop
shape. `sum_squares()` accumulates `i * i` into a counter;
`print_messages()` prints a string with `i` interpolated. Different
return types, different body intent — but the surface scaffolding is
identical.

`for i in 0..10` is one of the most common Rust idioms; flagging this
as duplication would generate noise on essentially every codebase.

## Why dry4rs should NOT detect

The functions' fingerprint sets diverge on the loop bodies — one
contains arithmetic + assignment, the other contains a `println!`
macro invocation. Set-Jaccard intersects shared tokens (the `for`
loop scaffold + the integer range), but the union dominates because
the body tokens are disjoint. Score should land below the threshold.

This is the canonical "shared shallow idiom" case. A detector that
flagged it would be unusable in real codebases.

## How similar tools handle this

Token-stream detectors with low minimum-token thresholds (e.g., CPD
at small `--minimum-tokens`) catch this and spam findings. AST-edit
detectors (`similarity-rs`) and structural detectors (dry4rs) should
not. The catalogue records both verdicts.
