# edge_cases / generic_with_different_bounds

## What this demonstrates

Two generic functions that print a value to stdout. Both take one
generic parameter `T`, both call `println!`, both end with a final
"done" line. The only differences are:

- The trait bound on `T` (`Display` vs `Debug`).
- The format-string specifier (`{value}` vs `{value:?}`).

These two functions are *not* interchangeable — `Display` and `Debug`
have semantically distinct purposes, and a developer choosing between
them is making an intentional choice. But the surface structure is
nearly identical.

## What dry4rs observes

dry4rs's normalizer treats trait bounds as content tokens (per the
typed-placeholder ADR, generic parameter NAMES are normalized but
their BOUNDS contribute to the fingerprint). The println format
specifier (`{}` vs `{:?}`) is a distinct token in the fingerprint
set.

Whether the surface-level Jaccard score lands above the threshold
depends on the body-token volume vs the differing-token volume. The
captured `expected.json` records the observed verdict.

## How similar tools handle this

Trait-bound-sensitive detection is a defining feature of Rust-aware
duplication tools. Token-stream detectors treat the bounds as
incidental text; AST-aware tools treat them as part of the function's
signature semantics.

The catalogue verdict makes dry4rs's actual treatment auditable
without leaning on intuition.
