# tier_4_false_positive_bait / common_error_handling

## What this demonstrates

Two unrelated parser functions that share the
`if let Err(e) = parsable.parse::<T>() { ... }` Rust idiom. The
similarity is the language pattern, not the semantics: one parses
`i32`, the other `i64`; one returns 0 on failure, the other -1.

This kind of shallow idiom recurs across hundreds of files in any
nontrivial Rust codebase. Flagging it as duplication would generate
zero-signal noise.

## Why dry4rs should NOT detect (or should flag advisory at most)

The shared structure is the `if let Err` arm + the `eprintln!` arm,
which would emit ~3 shared fingerprints. The diverging tokens (the
parsed type, the error-arm message, the return value, the fallback
literal) carry the body's actual intent.

Whether the score lands at 0.0 (entirely diverged), in the advisory
range (0.5-0.85), or in review_first (above 0.85) is empirical —
the v0.1 threshold of 0.85 is the gate consumers care about. A
verdict at `review_first` or higher would be a false positive; a
verdict at `advisory` or lower is acceptable.

## How similar tools handle this

`similarity-rs` typically applies a minimum-AST-node threshold that
filters this class. dry4rs's set-Jaccard relies on the differing-
token-volume to outweigh the shared idiom; the empirical verdict
records the actual outcome.
