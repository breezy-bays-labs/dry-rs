# tier_2_renamed / renamed_locals

## What this demonstrates

Two functions with the same control-flow shape, same types, same
operators — but different local variable names (`w`/`h`/`product` vs
`span`/`depth`/`total`). A consistent renaming of local bindings is
the textbook "rename refactor that the developer didn't bother to
extract".

## Why dry4rs should detect

The typed-placeholder normalization in
`adr-rust-normalization-rules.md` replaces each binding's identifier
with a slot keyed by its type (`f64`-binding-slot). After that
substitution, both function bodies emit identical fingerprint sets —
the comparison engine should cluster them.

The `rename_count` and `rename_density` columns on `Match` (currently
`null` at v0.1, filled at v0.2 per the locked multi-score envelope)
will eventually quantify this signal explicitly. v0.1 sees the pair
as a single Jaccard score.

## How similar tools handle this

Renamed-locals is the case where structural detectors (like dry4rs's
normalizer) outperform pure token-stream detectors. `similarity-rs`
also normalizes identifiers; PMD CPD does not by default.
