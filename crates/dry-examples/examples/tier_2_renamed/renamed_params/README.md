# tier_2_renamed / renamed_params

## What this demonstrates

Two Euclidean-distance functions with identical types, identical
control flow, identical operators, and identical local variable names
(`dx`/`dy`) — but different parameter names (`x1`/`y1`/`x2`/`y2` vs
`ax`/`ay`/`bx`/`by`). The renamed parameters propagate into the body's
expressions, so the surface tokens diverge on every line that
references them.

## Why dry4rs should detect

Parameter identifiers normalize to typed binding slots in the same
pass that handles local renaming (`adr-rust-normalization-rules.md`).
The post-normalization fingerprint sets converge for both functions;
the comparison engine should hash-bucket them together.

## How similar tools handle this

Parameter-rename duplication is the canonical "refactor opportunity"
case. Tools with identifier normalization catch it; tools that hash
raw tokens do not. dry4rs's intentional v0.1 design is to catch this
class — the verdict column in `EXPECTED.md` makes it auditable.
