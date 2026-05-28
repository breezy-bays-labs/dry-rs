# edge_cases / cross_file_duplicate

## What this demonstrates

The corpus's only **multi-file** fixture (per ADR-6). Two files —
`producer.rs` and `consumer.rs` — each define a `pack_*` function
with structurally identical bodies (`format!` header, `to_string`
body, final `format!` join). The functions live in separate `.rs`
files; their duplication is invisible to a single-file analyzer.

The harness invokes dry4rs with the DIRECTORY path
(`examples/edge_cases/cross_file_duplicate/`), not a single file.
dry4rs's walker recursively reads both `.rs` files, normalizes each
function, and clusters duplicates via the hash-bucket pass — the
cross-file detection path.

## Why dry4rs should detect

dry4rs's hash-bucket pass clusters forms by `fingerprint_set` hash
across the full input set, not per-file. The two `pack_*` functions
should hit the same bucket and surface as a single match with two
`forms[]` entries pointing at different files. The `forms[].file`
values are the load-bearing assertion: this fixture proves the
detector "sees" multiple files.

If a regression segregates per-file analysis (or breaks the walker's
multi-file traversal), this fixture's `expected.json` diverges
visibly.

## How similar tools handle this

Cross-file detection is what separates serious duplication tools
from grep-based linters. Both `similarity-rs` and dry4rs handle the
multi-file case; the catalogue records the per-tool verdicts.
