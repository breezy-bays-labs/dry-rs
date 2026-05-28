# tier_1_exact / identical_method_bodies

## What this demonstrates

Two methods (`Counter::step` and `Tally::step`) on different structs,
with identical bodies. Each mutates its containing struct's single
`i32` field, takes a snapshot, prints it, and returns the snapshot.
The receiver field name is the only token that differs.

## Why dry4rs should detect

dry4rs walks method bodies via the syn `ItemImpl` visitor; method
bodies emit `NormalizedForm` instances of `FormKind::Production`
exactly like free functions. The typed-placeholder normalization
strips the struct field identifiers (`self.value` vs `self.total`) to
field-access placeholders, so the two bodies should converge on
identical fingerprints.

## How similar tools handle this

Most token-based duplication detectors (PMD CPD, jscpd) catch this
case once they normalize method-vs-free-function syntax. The
documented observation lives in `EXPECTED.md` after BLESS captures
dry4rs's actual verdict — that column is empirical, not aspirational.
