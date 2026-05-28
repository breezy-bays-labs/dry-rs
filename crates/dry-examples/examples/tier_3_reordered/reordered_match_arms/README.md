# tier_3_reordered / reordered_match_arms

## What this demonstrates

Two functions that match on the same enum with identical arm bodies
but different source order. Because match arms with non-overlapping
patterns are semantically equivalent under any permutation, the two
functions are behaviorally identical.

This fixture stresses dry4rs's normalizer at the match-arm level:
does the fingerprint include arm position, or only the
pattern-to-body mapping? The set-based fingerprint should be
position-invariant.

## Why dry4rs may or may not detect

Match arms emit one fingerprint per `(pattern, body)` pair if the
normalizer collapses them; the surrounding `match` syntax contributes
the same fingerprint regardless of arm order. Hash-bucket clustering
should pair these two functions.

A tree-edit-distance detector would NOT treat arm permutation as
identity — the syntax trees structurally differ at the match-arm
nodes. This is an opinionated detection boundary, exactly what
tier 3 documents.

## How similar tools handle this

`similarity-rs` (AST-edit-distance based) typically considers
reordered match arms as different bodies. The dry4rs/similarity-rs
verdict split on this fixture is informative for choosing between
the tools' design philosophies.
