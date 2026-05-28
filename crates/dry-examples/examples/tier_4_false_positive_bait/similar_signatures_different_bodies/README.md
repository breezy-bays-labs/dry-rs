# tier_4_false_positive_bait / similar_signatures_different_bodies

## What this demonstrates

Classic false-positive bait: `add(a, b) -> a + b` and
`subtract(a, b) -> a - b`. Same signature, same parameter shape,
similar body LENGTH — but the operator differs. These are not
duplicates; they're a textbook case of code that *should not* be
extracted to a shared helper.

## Why dry4rs should NOT detect

The single-token difference (`+` vs `-`) lands in the operator
fingerprint slot. Set-Jaccard sees one of the two fingerprints
diverge; assuming dry4rs's normalizer keeps operators distinct (it
does — operators are content tokens, not identifier tokens), the
score should fall well below the 0.85 review_first threshold.

If a regression suddenly flags this pair, the comparison engine
has over-normalized and is treating `+` and `-` as the same token —
a real bug. tier_4 fixtures are the regression sentinel for
"detector got more lenient".

## How similar tools handle this

A well-tuned duplication detector should NOT flag this fixture. The
catalogue's `dry4rs verdict` column should read "no match" or
"advisory at worst"; a tier 1 verdict here would be a serious
detector regression.
