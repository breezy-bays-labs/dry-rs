# tier_1_exact / identical_signatures

## What this demonstrates

Two free functions with identical signatures (one `i32` parameter, one
`i32` return), identical bodies (`let result = <param> + 1; println!;
result`), and different names + parameter identifiers. The structural
content is the same; the surface tokens differ only in identifier
names.

## Why dry4rs should detect

dry4rs's syn-based normalizer strips identifier names to typed
placeholders before fingerprinting (see
`adr-rust-normalization-rules.md`). After normalization, the two
function bodies hash identically modulo their `fingerprint_set`s; the
comparison engine's hash-bucket pass clusters them on the first scan.

## How similar tools handle this

`similarity-rs` (its closest competitor) detects identical-body
function pairs at default settings. This fixture is the baseline
"both tools should catch" case — both tools' verdicts populate
`EXPECTED.md`'s catalogue columns after `bench.sh` runs.

The exact verdict (`auto_refactor` vs `review_first` vs the Jaccard
score) is **observational** — see ADR-7. Whatever `BLESS=1` captures
becomes the regression contract; the `expected.json` round-trips on
every CI run.
