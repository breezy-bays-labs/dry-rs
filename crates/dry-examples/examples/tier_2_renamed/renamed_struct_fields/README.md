# tier_2_renamed / renamed_struct_fields

## What this demonstrates

Two factory functions that build different structs (`Point` and
`Vec2`) with the same shape — both are 2-field `f64` records — but
the field names differ (`x`/`y` vs `horizontal`/`vertical`). The
factory bodies double their inputs, bind them to locals, and construct
the struct.

## Why dry4rs should detect

Field-name identifiers normalize through the same typed-placeholder
mechanism (`adr-rust-normalization-rules.md`). The field access /
struct construction syntax (`StructName { field: value, ... }`) is the
same after the normalizer strips both the struct path and the field
names; the surviving structure is "construct a 2-field record from
two `f64` locals scaled from two `f64` params".

This is the noisiest of the tier_2 fixtures — the renaming pervades
both the type definition and the construction expression. v0.2's
`rename_count` field on `Match` (currently `null`) will eventually
let consumers see the rename pressure explicitly.

## How similar tools handle this

Struct-field renaming is harder for token-stream detectors because
the type name and field names appear in multiple syntactic positions.
The catalogue column records dry4rs's actual behavior.
