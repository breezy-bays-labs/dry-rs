# edge_cases / doctest_duplicates

## What this demonstrates

Two functions with both identical bodies and identical doctests. Each
doctest is a `let result = 1 + 1; assert_eq!(result, 2);` block —
the same code copy-pasted between two functions' rustdoc.

Doctests are second-class duplication: a developer often copy-pastes
a doctest when adding a similar function. Whether dry4rs's normalizer
surfaces doctest bodies as separate forms (with
`FormKind::Doctest`) — or hides them entirely — is the question this
fixture pins.

## What dry4rs observes (depends on adapter behavior)

The `adr-normalized-form-schema.md` ADR sketches a `FormKind`
discriminator at v0.1, with `Production`, `Test`, and potentially
`Doctest`. Whether the syn adapter parses doctest fences and surfaces
them as forms is a normalizer-implementation detail.

The recorded `expected.json` documents the actual v0.1 behavior — if
zero forms surface from doctests, the fixture documents that
limitation. If forms surface, the captured count + tier is the
contract.

## How similar tools handle this

Doctest handling varies wildly across tools. Most production
duplication detectors ignore doctests entirely. dry-rs's eventual
position on this surface area lives empirically in the catalogue.
